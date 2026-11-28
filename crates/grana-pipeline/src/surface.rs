mod disk;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use grana_geometry::{Digest, PackedMesh};
use grana_scene::{Drifts, Scene};
use grana_volume::ChannelInfo;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    memory_hits: u64,
    disk_hits: u64,
    traced: u64,
}

impl CacheStats {
    pub fn memory_hits(self) -> u64 {
        self.memory_hits
    }

    pub fn disk_hits(self) -> u64 {
        self.disk_hits
    }

    pub fn traced(self) -> u64 {
        self.traced
    }
}

pub struct Cache {
    channels: Arc<Channels>,
    drifts: Drifts,
    stamp: u64,
    generation: AtomicU64,
    memory_hits: AtomicU64,
    disk_hits: AtomicU64,
    traced: AtomicU64,
    held: RwLock<Held>,
    tracing: Mutex<()>,
    disk: Option<disk::Store>,
}

#[derive(Default)]
struct Held {
    surfaces: Vec<Surface>,
    empty: HashSet<(usize, u32)>,
}

impl Cache {
    pub fn new(channels: Arc<Channels>, scene: &Scene) -> Self {
        Self::with_disk_cache(channels, scene, disk::default_root())
    }

    pub fn with_disk_cache(channels: Arc<Channels>, scene: &Scene, root: Option<PathBuf>) -> Self {
        let stamp = stamp(scene);
        let disk = channels.identity().and_then(|namespace| {
            root.as_deref()
                .and_then(|root| disk::Store::open(root, namespace, stamp))
        });
        Self {
            channels,
            drifts: scene.drifts().clone(),
            stamp,
            generation: AtomicU64::new(0),
            memory_hits: AtomicU64::new(0),
            disk_hits: AtomicU64::new(0),
            traced: AtomicU64::new(0),
            held: RwLock::new(Held::default()),
            tracing: Mutex::new(()),
            disk,
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

    pub fn statistics(&self) -> CacheStats {
        CacheStats {
            memory_hits: self.memory_hits.load(Ordering::Relaxed),
            disk_hits: self.disk_hits.load(Ordering::Relaxed),
            traced: self.traced.load(Ordering::Relaxed),
        }
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
            self.memory_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(Outcome::Traced(held));
        }
        if self.read().empty(channel, time) {
            self.memory_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(Outcome::Empty);
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(Outcome::Cancelled);
        }
        let level = self.channels.pyramid().level(0);
        let depth = level.extent().z();
        let source = match self.disk.is_some() {
            true => self
                .channels
                .source_stamp(time, channel, 0..depth)
                .map_err(|error| error.to_string())?,
            false => None,
        };
        let context = self.context(channel);
        let key = source.map(|source| disk::Key {
            context,
            channel,
            time,
            source,
        });
        if let (Some(store), Some(key)) = (&self.disk, key) {
            match store.load(key) {
                Some(disk::Record::Surface { mesh, colors }) => {
                    let surface =
                        Surface::of(channel, time, self.stamp, Arc::new(mesh), Arc::new(colors));
                    let mut held = self.held.write().unwrap_or_else(|error| error.into_inner());
                    held.insert(surface.clone(), time);
                    self.disk_hits.fetch_add(1, Ordering::Relaxed);
                    self.generation.fetch_add(1, Ordering::Relaxed);
                    return Ok(Outcome::Traced(surface));
                }
                Some(disk::Record::Empty) => {
                    let mut held = self.held.write().unwrap_or_else(|error| error.into_inner());
                    held.insert_empty(channel, time);
                    self.disk_hits.fetch_add(1, Ordering::Relaxed);
                    return Ok(Outcome::Empty);
                }
                None => {}
            }
        }
        let request = TraceRequest::new(channel, time, level, self.drifts.clone());
        let mut traced = None;
        trace(&self.channels, request, cancel, &mut |event| match event {
            SurfaceEvent::Progress { done, total } => progress(done, total),
            SurfaceEvent::Surface { mesh, colors } => traced = Some((mesh, colors)),
        })?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(Outcome::Cancelled);
        }
        self.traced.fetch_add(1, Ordering::Relaxed);
        let Some((mesh, colors)) = traced else {
            if let (Some(store), Some(key)) = (&self.disk, key) {
                store.save_empty(key);
            }
            self.held
                .write()
                .unwrap_or_else(|error| error.into_inner())
                .insert_empty(channel, time);
            return Ok(Outcome::Empty);
        };
        if let (Some(store), Some(key)) = (&self.disk, key) {
            store.save_surface(key, &mesh, &colors);
        }
        let surface = Surface::of(channel, time, self.stamp, mesh, colors);
        let mut held = self.held.write().unwrap_or_else(|error| error.into_inner());
        held.insert(surface.clone(), time);
        self.generation.fetch_add(1, Ordering::Relaxed);
        Ok(Outcome::Traced(surface))
    }

    fn context(&self, channel: usize) -> u64 {
        let mut digest = Digest::new();
        digest.mix(self.channels.layout().digest());
        digest.mix(channel as u64);
        if let Some(info) = self.channels.pyramid().channels().get(channel) {
            channel_info(info, &mut digest);
        }
        digest.finish()
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Held> {
        self.held.read().unwrap_or_else(|error| error.into_inner())
    }
}

fn channel_info(info: &ChannelInfo, digest: &mut Digest) {
    digest.mix(info.name().len() as u64);
    for byte in info.name().as_bytes() {
        digest.mix(u64::from(*byte));
    }
    for component in info.tint() {
        digest.real(component);
    }
    let statistics = info.statistics();
    digest.real(statistics.intensity().min());
    digest.real(statistics.intensity().max());
    digest.mix(u64::from(statistics.distinct()));
    digest.mix(u64::from(statistics.semantics().code()));
    digest.real(statistics.background());
    digest.mix(u64::from(info.format().code()));
    digest.flag(info.color_table().is_some());
    if let Some(table) = info.color_table() {
        digest.mix(table.colors().len() as u64);
        for color in table.colors() {
            for component in color {
                digest.real(*component);
            }
        }
    }
    digest.flag(info.no_data().is_some());
    if let Some(no_data) = info.no_data() {
        digest.mix(no_data.value().to_bits());
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

    fn empty(&self, channel: usize, time: u32) -> bool {
        self.empty.contains(&(channel, time))
    }

    fn insert_empty(&mut self, channel: usize, time: u32) {
        self.empty.insert((channel, time));
    }

    fn insert(&mut self, surface: Surface, time: u32) {
        self.empty.remove(&(surface.channel(), surface.time()));
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

pub fn cache_root() -> Option<PathBuf> {
    disk::default_root()
}

pub fn clear_cache() -> std::io::Result<u64> {
    cache_root().map_or(Ok(0), |root| disk::clear(&root))
}

pub fn clear_cache_at(root: &Path) -> std::io::Result<u64> {
    disk::clear(root)
}
