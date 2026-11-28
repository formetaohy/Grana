use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use grana_geometry::{Digest, PackedMesh};
use grana_scene::{Drifts, Scene};

use crate::channels::Channels;
use crate::run::{SurfaceEvent, TraceRequest, trace};

const RETAINED: usize = 4;
const RETAINED_BYTES: u64 = 512 << 20;

#[derive(Clone)]
pub struct Surface {
    channel: usize,
    time: u32,
    mesh: Arc<PackedMesh>,
    colors: Arc<Vec<[f32; 3]>>,
    revision: u64,
}

impl Surface {
    fn of(
        channel: usize,
        time: u32,
        stamp: u64,
        mesh: Arc<PackedMesh>,
        colors: Arc<Vec<[f32; 3]>>,
    ) -> Self {
        assert_eq!(
            colors.len(),
            mesh.object_count(),
            "a traced surface names one colour per label"
        );
        let mut revision = Digest::new();
        revision.mix(stamp);
        revision.mix(channel as u64);
        revision.mix(u64::from(time));
        revision.mix(mesh.vertex_count() as u64);
        revision.mix(mesh.triangle_count() as u64);
        Self {
            channel,
            time,
            mesh,
            colors,
            revision: revision.finish(),
        }
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn mesh(&self) -> &Arc<PackedMesh> {
        &self.mesh
    }

    pub fn colors(&self) -> &[[f32; 3]] {
        &self.colors
    }

    pub fn shared_colors(&self) -> &Arc<Vec<[f32; 3]>> {
        &self.colors
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn bytes(&self) -> u64 {
        self.mesh.byte_length() as u64 + (self.colors.len() * size_of::<[f32; 3]>()) as u64
    }
}

pub fn wanted(scene: &Scene, channel: usize) -> bool {
    scene.channels.get(channel).is_some_and(|held| held.enabled)
        && scene
            .dataset
            .channels()
            .get(channel)
            .is_some_and(|info| info.color_table().is_some())
}

pub fn stamp(scene: &Scene) -> u64 {
    let level = scene.dataset.pyramid().level(0);
    let extent = level.extent();
    let basis = level.basis().matrix();
    let mut digest = Digest::new();
    for column in 0..3 {
        for value in basis.col(column).to_array() {
            digest.real(value);
        }
    }
    for value in level.origin() {
        digest.real(value);
    }
    for value in [extent.x(), extent.y(), extent.z()] {
        digest.mix(u64::from(value));
    }
    digest.mix(scene.drifts().digest());
    for mask in scene.mask_stamps() {
        digest.mix(mask);
    }
    digest.finish()
}

pub enum Outcome {
    Traced(Surface),
    Empty,
    Cancelled,
}

pub struct Cache {
    channels: Arc<Channels>,
    drifts: Drifts,
    stamp: u64,
    generation: AtomicU64,
    held: RwLock<Held>,
    tracing: Mutex<()>,
}

#[derive(Default)]
struct Held {
    surfaces: Vec<Surface>,
}

impl Cache {
    pub fn new(channels: Arc<Channels>, scene: &Scene) -> Self {
        Self {
            channels,
            drifts: scene.drifts().clone(),
            stamp: stamp(scene),
            generation: AtomicU64::new(0),
            held: RwLock::new(Held::default()),
            tracing: Mutex::new(()),
        }
    }

    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    pub fn holds(&self, channels: &Arc<Channels>) -> bool {
        Arc::ptr_eq(&self.channels, channels)
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    pub fn peek(&self, channel: usize, time: u32) -> Option<Surface> {
        self.read().find(channel, time).cloned()
    }

    pub fn newest(&self, channel: usize) -> Option<Surface> {
        self.read().newest(channel).cloned()
    }

    pub fn get(
        &self,
        channel: usize,
        time: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Outcome, String> {
        let _sole = self
            .tracing
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(held) = self.peek(channel, time) {
            return Ok(Outcome::Traced(held));
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(Outcome::Cancelled);
        }
        let level = self.channels.pyramid().level(0);
        let request = TraceRequest::new(channel, time, level, self.drifts.clone());
        let mut traced = None;
        trace(&self.channels, request, cancel, &mut |event| match event {
            SurfaceEvent::Progress { done, total } => progress(done, total),
            SurfaceEvent::Surface { mesh, colors } => traced = Some((mesh, colors)),
        })?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(Outcome::Cancelled);
        }
        let Some((mesh, colors)) = traced else {
            return Ok(Outcome::Empty);
        };
        let surface = Surface::of(channel, time, self.stamp, mesh, colors);
        let mut held = self.held.write().unwrap_or_else(|error| error.into_inner());
        held.insert(surface.clone(), time);
        self.generation.fetch_add(1, Ordering::Relaxed);
        Ok(Outcome::Traced(surface))
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Held> {
        self.held.read().unwrap_or_else(|error| error.into_inner())
    }
}

impl Held {
    fn find(&self, channel: usize, time: u32) -> Option<&Surface> {
        self.surfaces
            .iter()
            .find(|held| held.channel() == channel && held.time() == time)
    }

    fn newest(&self, channel: usize) -> Option<&Surface> {
        self.surfaces
            .iter()
            .rev()
            .find(|held| held.channel() == channel)
    }

    fn insert(&mut self, surface: Surface, time: u32) {
        self.surfaces
            .retain(|held| !(held.channel() == surface.channel() && held.time() == surface.time()));
        self.surfaces.push(surface);
        self.retain(time);
    }

    fn retain(&mut self, time: u32) {
        let mut counts: HashMap<usize, usize> = HashMap::new();
        let mut bytes = 0u64;
        let mut kept = Vec::with_capacity(self.surfaces.len());
        for surface in self.surfaces.iter().rev() {
            let current = surface.time() == time;
            let count = counts.entry(surface.channel()).or_default();
            let size = surface.bytes();
            if current || (*count < RETAINED && bytes + size <= RETAINED_BYTES) || kept.is_empty() {
                *count += 1;
                bytes += size;
                kept.push(surface.clone());
            }
        }
        kept.reverse();
        self.surfaces = kept;
    }
}
