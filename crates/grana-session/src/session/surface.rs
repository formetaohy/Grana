use std::collections::HashMap;
use std::sync::Arc;

use grana_exec::Role;
use grana_pipeline::Surface;
use grana_pipeline::channels::Channels;
use grana_pipeline::run::{SurfaceEvent, TraceRequest, trace};
use grana_scene::Scene;

use super::Session;
use crate::task::Progress;

const SURFACE: &str = "Tracing surfaces";
const RETAINED: usize = 4;
const RETAINED_BYTES: u64 = 512 << 20;

struct Trace {
    channel: usize,
    time: u32,
    stamp: u64,
    task: Progress<Result<Piece, String>>,
}

struct Piece {
    mesh: Arc<grana_geometry::PackedMesh>,
    colors: Arc<Vec<[f32; 3]>>,
}

#[derive(Default)]
pub(super) struct Surfaces {
    held: Vec<Surface>,
    shown: Vec<Surface>,
    tracing: Option<Trace>,
    failed: HashMap<(usize, u32), String>,
    shown_key: (u32, u64),
    stamp: u64,
    revision: u64,
}

impl Surfaces {
    pub(super) fn shown(&self) -> &[Surface] {
        &self.shown
    }

    pub(super) fn busy(&self) -> bool {
        self.tracing.is_some()
    }

    pub(super) fn progress(&self) -> Option<(&str, usize, usize)> {
        let tracing = self.tracing.as_ref()?;
        let (done, total) = tracing.task.progress();
        Some((SURFACE, done, total))
    }

    pub(super) fn clear(&mut self) {
        self.held.clear();
        self.shown.clear();
        self.tracing = None;
        self.failed.clear();
        self.shown_key = (0, 0);
        self.stamp = 0;
    }

    fn current(&self, channel: usize, time: u32) -> Option<&Surface> {
        self.held
            .iter()
            .find(|held| held.channel() == channel && held.time() == time)
    }

    fn newest(&self, channel: usize) -> Option<&Surface> {
        self.held
            .iter()
            .rev()
            .find(|held| held.channel() == channel)
    }

    fn settled(&self, scene: &Scene) -> bool {
        if self.tracing.is_some() {
            return false;
        }
        let time = scene.time();
        (0..scene.channels.len()).all(|channel| {
            let Some(held) = scene.channels.get(channel) else {
                return true;
            };
            !held.enabled
                || !wanted(scene, channel)
                || self.current(channel, time).is_some()
                || self.failed.contains_key(&(channel, time))
        })
    }

    fn refresh(&mut self, scene: &Scene) {
        let key = (scene.time(), self.revision);
        if self.shown_key == key {
            return;
        }
        self.shown_key = key;
        let time = scene.time();
        self.shown.clear();
        for channel in 0..scene.channels.len() {
            if !wanted(scene, channel) {
                continue;
            }
            let chosen = self.current(channel, time).or_else(|| {
                match self.failed.contains_key(&(channel, time)) {
                    true => None,
                    false => self.newest(channel),
                }
            });
            if let Some(surface) = chosen {
                self.shown.push(surface.clone());
            }
        }
    }

    fn store(&mut self, channel: usize, time: u32, stamp: u64, piece: Piece) -> bool {
        if self.stamp != stamp {
            return false;
        }
        self.revision += 1;
        let surface = Surface::new(channel, time, piece.mesh, piece.colors, self.revision);
        self.held
            .retain(|held| !(held.channel() == channel && held.time() == time));
        self.held.push(surface);
        self.retain(time);
        true
    }

    fn retain(&mut self, time: u32) {
        let mut counts: HashMap<usize, usize> = HashMap::new();
        let mut bytes = 0u64;
        let mut kept = Vec::with_capacity(self.held.len());
        for surface in self.held.iter().rev() {
            let current = surface.time() == time;
            let count = counts.entry(surface.channel()).or_default();
            let size = surface.mesh().byte_length() as u64;
            if (current || (*count < RETAINED && bytes + size <= RETAINED_BYTES)) || kept.is_empty()
            {
                *count += 1;
                bytes += size;
                kept.push(surface.clone());
            }
        }
        kept.reverse();
        self.held = kept;
    }

    fn poll(&mut self, stamp: u64, scene: &Scene) -> bool {
        let Some(tracing) = self.tracing.as_mut() else {
            return false;
        };
        match tracing.task.poll() {
            crate::task::Poll::Event(Ok(piece)) => {
                let channel = tracing.channel;
                let time = tracing.time;
                let held = tracing.stamp;
                self.tracing = None;
                if !self.store(channel, time, held, piece) || held != stamp {
                    return true;
                }
                grana_log::info!(
                    target: "grana::session",
                    channel,
                    time,
                    triangles = self.held.last().map_or(0, |surface| surface.mesh().triangle_count()),
                    "the channel surface is ready"
                );
                self.refresh(scene);
                true
            }
            crate::task::Poll::Event(Err(detail)) => {
                let channel = tracing.channel;
                let time = tracing.time;
                let held = tracing.stamp;
                self.tracing = None;
                if held == stamp {
                    grana_log::warn!(
                        target: "grana::session",
                        channel,
                        time,
                        %detail,
                        "the channel surface failed"
                    );
                    self.failed.insert((channel, time), detail);
                    self.refresh(scene);
                }
                true
            }
            crate::task::Poll::Empty => false,
            crate::task::Poll::Closed => {
                self.tracing = None;
                true
            }
        }
    }

    fn ensure(&mut self, scene: &Scene, channels: Option<Arc<Channels>>) {
        let Some(channels) = channels else {
            return;
        };
        self.refresh(scene);
        if self.tracing.is_some() {
            return;
        }
        let time = scene.time();
        if self.failed.len() > 32 {
            self.failed.retain(|(_, failed), _| *failed == time);
        }
        let Some(channel) = (0..scene.channels.len()).find(|channel| {
            let Some(held) = scene.channels.get(*channel) else {
                return false;
            };
            held.enabled
                && wanted(scene, *channel)
                && self.current(*channel, time).is_none()
                && !self.failed.contains_key(&(*channel, time))
        }) else {
            return;
        };
        let level = scene.dataset.pyramid().level(0);
        let drifts = scene.drifts().clone();
        self.tracing = Some(Trace {
            channel,
            time,
            stamp: self.stamp,
            task: Progress::spawn(Role::Compute, move |cancel, progress| {
                let request = TraceRequest::new(channel, time, level, drifts);
                let mut traced = None;
                let outcome = trace(&channels, request, cancel, &mut |event| match event {
                    SurfaceEvent::Progress { done, total } => progress(done, total),
                    SurfaceEvent::Surface { mesh, colors } => {
                        traced = Some(Piece { mesh, colors });
                    }
                });
                match (outcome, traced) {
                    (Ok(()), Some(piece)) => Ok(piece),
                    (Ok(()), None) => Err("the labels hold no surface to trace".to_string()),
                    (Err(detail), _) => Err(detail),
                }
            }),
        });
    }
}

impl Session {
    pub fn surfaces(&self) -> &[Surface] {
        self.traced.shown()
    }

    pub fn surfaces_settled(&self) -> bool {
        match self.scene.as_ref() {
            Some(scene) => self.traced.settled(scene),
            None => true,
        }
    }

    pub fn surface_progress(&self) -> Option<(&str, usize, usize)> {
        self.traced.progress()
    }

    pub(super) fn poll_surfaces(&mut self) -> bool {
        let Self { scene, traced, .. } = self;
        let Some(scene) = scene.as_ref() else {
            return false;
        };
        let stamp = geometry_stamp(scene);
        traced.poll(stamp, scene)
    }

    pub(super) fn ensure_surfaces(&mut self) {
        if self.loading.is_some() {
            return;
        }
        let Self {
            scene,
            traced,
            channels,
            ..
        } = self;
        let Some(scene) = scene.as_ref() else {
            return;
        };
        let stamp = geometry_stamp(scene);
        if traced.stamp != stamp {
            traced.held.clear();
            traced.failed.clear();
            traced.tracing = None;
            traced.shown_key = (0, 0);
            traced.stamp = stamp;
        }
        traced.ensure(scene, channels.as_ref().map(Arc::clone));
    }
}

fn wanted(scene: &Scene, channel: usize) -> bool {
    scene.dataset.channels()[channel].color_table().is_some()
}

fn geometry_stamp(scene: &Scene) -> u64 {
    let level = scene.dataset.pyramid().level(0);
    let extent = level.extent();
    let basis = level.basis().matrix();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |value: u64| hash = (hash ^ value).wrapping_mul(0x0000_0100_0000_01b3);
    for column in 0..3 {
        for value in basis.col(column).to_array() {
            mix(u64::from(value.to_bits()));
        }
    }
    for value in level.origin() {
        mix(u64::from(value.to_bits()));
    }
    for value in [extent.x(), extent.y(), extent.z()] {
        mix(u64::from(value));
    }
    mix(scene.drifts().digest());
    hash
}
