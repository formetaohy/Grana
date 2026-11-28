use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use grana_engine::Surface;
use grana_pipeline::surface::{self, Cache};
use grana_scene::Scene;

pub struct Traced {
    cache: Arc<Cache>,
}

impl Traced {
    pub fn new(cache: Arc<Cache>) -> Self {
        Self { cache }
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
}

pub(crate) fn converted(surface: &grana_pipeline::Surface) -> Surface {
    Surface::new(
        surface.channel(),
        Arc::clone(surface.mesh()),
        Arc::clone(surface.shared_colors()),
        surface.revision(),
    )
}
