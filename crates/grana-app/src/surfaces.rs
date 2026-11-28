use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use grana_engine::Surface;
use grana_exec::{Executor, Job, Priority, Role};
use grana_pipeline::surface::{self, Cache};
use grana_scene::Scene;

pub struct Traced {
    cache: Arc<Cache>,
    prefetch: Mutex<Option<(u32, Job)>>,
}

impl Traced {
    pub fn new(cache: Arc<Cache>) -> Self {
        Self {
            cache,
            prefetch: Mutex::new(None),
        }
    }
}

impl Drop for Traced {
    fn drop(&mut self) {
        let held = self
            .prefetch
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some((_, job)) = held {
            job.cancel();
        }
    }
}

impl grana_offline::Surfaces for Traced {
    fn frame(&self, scene: &Scene, time: u32, cancel: &AtomicBool) -> Result<Vec<Surface>, String> {
        let mut traced = Vec::new();
        for channel in 0..scene.channels.len() {
            if !surface::wanted(scene, channel) {
                continue;
            }
            match self.cache.get(channel, time, cancel, &mut |_, _| {})? {
                surface::Outcome::Traced(held) => traced.push(converted(&held)),
                surface::Outcome::Empty => {}
                surface::Outcome::Cancelled => break,
            }
        }
        Ok(traced)
    }

    fn prefetch(&self, scene: &Scene, time: u32) {
        let channels = (0..scene.channels.len())
            .filter(|channel| surface::wanted(scene, *channel))
            .collect::<Vec<_>>();
        if channels.is_empty() {
            return;
        }
        let mut held = self
            .prefetch
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if held.as_ref().is_some_and(|(pending, _)| *pending == time) {
            return;
        }
        if let Some((_, job)) = held.take() {
            job.cancel();
        }
        let cache = Arc::clone(&self.cache);
        let job = Executor::lane(Role::Compute).submit_with(Priority::Background, move |cancel| {
            for channel in channels {
                if cancel.cancelled() {
                    break;
                }
                let _ = cache.get(channel, time, cancel, &mut |_, _| {});
            }
        });
        *held = Some((time, job));
    }
}

pub(crate) fn converted(surface: &grana_pipeline::Surface) -> Surface {
    Surface::new(
        surface.channel(),
        Arc::clone(surface.mesh()),
        Arc::clone(surface.shared_colors()),
        surface.revision(),
    )
}
