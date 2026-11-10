use std::sync::Arc;

use grana_image::Image;
use grana_offline::{Event, Handle, Outcome, Plan, Task, Worker};
use grana_pipeline::Channels;
use grana_scene::Scene;

use crate::bricks::ImageBricks;

pub enum Report {
    Finished(Outcome),
    Closed,
}

pub struct Export {
    worker: Worker,
    active: Option<Handle>,
    progress: Option<(u32, u32)>,
}

impl Default for Export {
    fn default() -> Self {
        Self::new()
    }
}

impl Export {
    pub fn new() -> Self {
        Self {
            worker: Worker::spawn(),
            active: None,
            progress: None,
        }
    }

    pub fn running(&self) -> bool {
        self.active.is_some()
    }

    pub fn progress(&self) -> Option<(u32, u32)> {
        self.progress
    }

    pub fn start(&mut self, plan: Plan, image: Arc<Image>, scene: &Scene) -> Result<(), String> {
        if self.active.is_some() {
            return Err("an export is already running".to_string());
        }
        let scene = scene.snapshot().into_scene();
        let channels = Channels::render(image, &scene);
        let task = Task {
            plan,
            scene,
            bricks: Arc::new(ImageBricks::new(channels)),
        };
        self.active = Some(self.worker.submit(task));
        self.progress = None;
        Ok(())
    }

    pub fn cancel(&mut self) {
        if let Some(handle) = self.active.as_ref() {
            handle.cancel();
        }
    }

    pub fn poll(&mut self) -> Vec<Report> {
        let (events, alive) = self.worker.drain();
        let mut reports = Vec::new();
        for event in events {
            match event {
                Event::Progress { id, frame, total } => {
                    if self.active.as_ref().is_some_and(|handle| handle.id() == id) {
                        self.progress = Some((frame, total));
                    }
                }
                Event::Finished { id, outcome } => {
                    if self.active.as_ref().is_some_and(|handle| handle.id() == id) {
                        self.active = None;
                        self.progress = None;
                        reports.push(Report::Finished(outcome));
                    }
                }
            }
        }
        if !alive && self.active.take().is_some() {
            self.progress = None;
            reports.push(Report::Closed);
        }
        reports
    }
}
