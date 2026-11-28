use std::collections::HashMap;
use std::sync::Arc;

use grana_exec::Role;
use grana_pipeline::Surface;
use grana_pipeline::run::{SurfaceEvent, TraceRequest, trace};
use grana_scene::Scene;

use super::Session;
use crate::task::Progress;

const SURFACE: &str = "Tracing surfaces";

struct Trace {
    channel: usize,
    stamp: u64,
    task: Progress<Result<Surface, String>>,
}

#[derive(Default)]
pub(super) struct Surfaces {
    held: Vec<Surface>,
    tracing: Option<Trace>,
    failed: HashMap<usize, String>,
    stamp: u64,
    revision: u64,
}

impl Surfaces {
    pub(super) fn held(&self) -> &[Surface] {
        &self.held
    }

    pub(super) fn busy(&self) -> bool {
        self.tracing.is_some()
    }

    pub(super) fn progress(&self) -> Option<(&str, usize, usize)> {
        let tracing = self.tracing.as_ref()?;
        let (done, total) = tracing.task.progress();
        Some((SURFACE, done, total))
    }

    fn settled(&self, scene: &Scene) -> bool {
        if self.tracing.is_some() {
            return false;
        }
        (0..scene.channels.len()).all(|channel| {
            let Some(held) = scene.channels.get(channel) else {
                return true;
            };
            !held.enabled
                || !wanted(scene, channel)
                || self.held.iter().any(|surface| surface.channel() == channel)
                || self.failed.contains_key(&channel)
        })
    }

    pub(super) fn clear(&mut self) {
        self.held.clear();
        self.tracing = None;
        self.failed.clear();
        self.stamp = 0;
    }

    fn poll(&mut self, stamp: u64) -> bool {
        let Some(tracing) = self.tracing.as_mut() else {
            return false;
        };
        match tracing.task.poll() {
            crate::task::Poll::Event(outcome) => {
                let channel = tracing.channel;
                let current = tracing.stamp == stamp;
                self.tracing = None;
                match outcome {
                    Ok(surface) if current => {
                        self.revision += 1;
                        let surface = Surface::new(
                            surface.channel(),
                            surface.time(),
                            Arc::clone(surface.mesh()),
                            Arc::clone(surface.shared_colors()),
                            self.revision,
                        );
                        grana_log::info!(
                            target: "grana::session",
                            channel = surface.channel(),
                            triangles = surface.mesh().triangle_count(),
                            labels = surface.colors().len(),
                            "the channel surface is ready"
                        );
                        self.held.retain(|held| held.channel() != surface.channel());
                        self.held.push(surface);
                    }
                    Ok(_) => {}
                    Err(detail) if current => {
                        grana_log::warn!(
                            target: "grana::session",
                            channel,
                            %detail,
                            "the channel surface failed"
                        );
                        self.failed.insert(channel, detail);
                    }
                    Err(_) => {}
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
}

impl Session {
    pub fn surfaces(&self) -> &[Surface] {
        self.traced.held()
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
        let stamp = self.scene.as_ref().map_or(0, geometry_stamp);
        self.traced.poll(stamp)
    }

    pub(super) fn ensure_surfaces(&mut self) {
        if self.loading.is_some() {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let stamp = geometry_stamp(scene);
        if self.traced.stamp != stamp {
            self.traced.held.clear();
            self.traced.failed.clear();
            self.traced.tracing = None;
            self.traced.stamp = stamp;
        }
        if self.traced.busy() {
            return;
        }
        let Some(channels) = self.channels.as_ref().map(Arc::clone) else {
            return;
        };
        let Some(channel) = (0..scene.channels.len()).find(|channel| {
            let Some(held) = scene.channels.get(*channel) else {
                return false;
            };
            held.enabled
                && wanted(scene, *channel)
                && !self.traced.failed.contains_key(channel)
                && !self
                    .traced
                    .held
                    .iter()
                    .any(|surface| surface.channel() == *channel)
        }) else {
            return;
        };
        let time = scene.time();
        let level = scene.dataset.pyramid().level(0);
        let drifts = scene.drifts().clone();
        self.traced.tracing = Some(Trace {
            channel,
            stamp,
            task: Progress::spawn(Role::Compute, move |cancel, progress| {
                let request = TraceRequest::new(channel, time, level, drifts);
                let mut traced = None;
                let outcome = trace(&channels, request, cancel, &mut |event| match event {
                    SurfaceEvent::Progress { done, total } => progress(done, total),
                    SurfaceEvent::Surface { mesh, colors } => {
                        traced = Some((mesh, colors));
                    }
                });
                match (outcome, traced) {
                    (Ok(()), Some((mesh, colors))) => {
                        Ok(Surface::new(channel, time, mesh, colors, 0))
                    }
                    (Ok(()), None) => Err("the labels hold no surface to trace".to_string()),
                    (Err(detail), _) => Err(detail),
                }
            }),
        });
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
    mix(u64::from(scene.time()));
    hash
}
