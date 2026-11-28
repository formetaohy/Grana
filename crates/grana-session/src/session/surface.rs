use std::collections::HashMap;
use std::sync::Arc;

use grana_exec::Role;
use grana_pipeline::Surface;
use grana_pipeline::surface::{self, Cache, Outcome};
use grana_scene::Scene;

use super::Session;
use crate::task::Progress;

const SURFACE: &str = "Tracing surfaces";

struct Trace {
    channel: usize,
    time: u32,
    task: Progress<Result<Outcome, String>>,
}

#[derive(Default)]
pub(super) struct Surfaces {
    shown: Vec<Surface>,
    tracing: Option<Trace>,
    failed: HashMap<(usize, u32), String>,
    shown_key: (u32, u64),
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
        self.shown.clear();
        self.tracing = None;
        self.failed.clear();
        self.shown_key = (0, 0);
    }

    fn settled(&self, scene: &Scene, cache: &Cache) -> bool {
        if self.tracing.is_some() {
            return false;
        }
        let time = scene.time();
        (0..scene.channels.len()).all(|channel| {
            !surface::wanted(scene, channel)
                || cache.peek(channel, time).is_some()
                || self.failed.contains_key(&(channel, time))
        })
    }

    fn refresh(&mut self, scene: &Scene, cache: &Cache) {
        let key = (scene.time(), cache.generation());
        if self.shown_key == key {
            return;
        }
        self.shown_key = key;
        let time = scene.time();
        self.shown.clear();
        for channel in 0..scene.channels.len() {
            if !surface::wanted(scene, channel) {
                continue;
            }
            let chosen = cache.peek(channel, time).or_else(|| {
                match self.failed.contains_key(&(channel, time)) {
                    true => None,
                    false => cache.newest(channel),
                }
            });
            if let Some(held) = chosen {
                self.shown.push(held);
            }
        }
    }

    fn poll(&mut self, cache: &Cache, scene: &Scene) -> bool {
        let Some(tracing) = self.tracing.as_mut() else {
            return false;
        };
        let channel = tracing.channel;
        let time = tracing.time;
        match tracing.task.poll() {
            crate::task::Poll::Event(Ok(Outcome::Traced(held))) => {
                self.tracing = None;
                grana_log::info!(
                    target: "grana::session",
                    channel,
                    time,
                    triangles = held.mesh().triangle_count(),
                    "the channel surface is ready"
                );
                self.refresh(scene, cache);
                true
            }
            crate::task::Poll::Event(Ok(Outcome::Empty)) => {
                self.tracing = None;
                self.failed.insert(
                    (channel, time),
                    "the labels hold no surface to trace".to_string(),
                );
                true
            }
            crate::task::Poll::Event(Ok(Outcome::Cancelled)) => {
                self.tracing = None;
                true
            }
            crate::task::Poll::Event(Err(detail)) => {
                self.tracing = None;
                grana_log::warn!(
                    target: "grana::session",
                    channel,
                    time,
                    %detail,
                    "the channel surface failed"
                );
                self.failed.insert((channel, time), detail);
                self.refresh(scene, cache);
                true
            }
            crate::task::Poll::Empty => false,
            crate::task::Poll::Closed => {
                self.tracing = None;
                true
            }
        }
    }

    fn ensure(&mut self, scene: &Scene, cache: Arc<Cache>) {
        self.refresh(scene, &cache);
        if self.tracing.is_some() {
            return;
        }
        let time = scene.time();
        if self.failed.len() > 32 {
            self.failed.retain(|(_, failed), _| *failed == time);
        }
        let Some(channel) = (0..scene.channels.len()).find(|channel| {
            surface::wanted(scene, *channel)
                && cache.peek(*channel, time).is_none()
                && !self.failed.contains_key(&(*channel, time))
        }) else {
            return;
        };
        self.tracing = Some(Trace {
            channel,
            time,
            task: Progress::spawn(Role::Compute, move |cancel, progress| {
                cache.get(channel, time, cancel, &mut |done, total| {
                    progress(done, total)
                })
            }),
        });
    }
}

impl Session {
    pub fn surfaces(&self) -> &[Surface] {
        self.traced.shown()
    }

    pub fn surface_cache(&self) -> Option<Arc<Cache>> {
        self.traced_cache.as_ref().map(Arc::clone)
    }

    pub fn surfaces_settled(&self) -> bool {
        match (self.scene.as_ref(), self.traced_cache.as_ref()) {
            (Some(scene), Some(cache)) => self.traced.settled(scene, cache),
            _ => true,
        }
    }

    pub fn surface_progress(&self) -> Option<(&str, usize, usize)> {
        self.traced.progress()
    }

    pub(super) fn poll_surfaces(&mut self) -> bool {
        let Self {
            scene,
            traced,
            traced_cache,
            ..
        } = self;
        let (Some(scene), Some(cache)) = (scene.as_ref(), traced_cache.as_ref()) else {
            return false;
        };
        traced.poll(cache, scene)
    }

    pub(super) fn ensure_surfaces(&mut self) {
        if self.loading.is_some() {
            return;
        }
        let Self {
            scene,
            traced,
            traced_cache,
            channels,
            ..
        } = self;
        let (Some(scene), Some(channels)) = (scene.as_ref(), channels.as_ref()) else {
            return;
        };
        if traced_cache
            .as_ref()
            .is_none_or(|cache| cache.stamp() != surface::stamp(scene) || !cache.holds(channels))
        {
            *traced_cache = Some(Arc::new(Cache::new(Arc::clone(channels), scene)));
            traced.clear();
        }
        let cache = Arc::clone(traced_cache.as_ref().expect("the surface cache is ready"));
        traced.ensure(scene, cache);
    }
}
