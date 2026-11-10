use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use grana_display::{OutputSize, Panes};
use grana_engine::Panels;
use grana_offline::{Destination, Offline, Outcome, Plan, Pose, Task};
use grana_pipeline::{Capture, CaptureKind, CaptureRequest, Channels};
use grana_scene::Scene;

use crate::bricks::ImageBricks;

pub struct OfflineCapture {
    state: Mutex<Option<Offline>>,
}

impl Default for OfflineCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl OfflineCapture {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(None),
        }
    }

    fn state(&self) -> Result<MutexGuard<'_, Option<Offline>>, String> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.is_none() {
            *state = Some(Offline::headless()?);
        }
        Ok(state)
    }
}

impl Capture for OfflineCapture {
    fn open(&self) -> Result<(), String> {
        self.state().map(|_| ())
    }

    fn capture(
        &self,
        channels: &Channels,
        scene: &Scene,
        request: &CaptureRequest,
        path: &Path,
    ) -> Result<(), String> {
        let mut slot = self.state()?;
        let offline = slot.as_mut().expect("the offline context is ready");
        let mut panes = Panes::new();
        panes.restore(request.view.panes);
        panes.set_layout(request.view.layout);
        panes.set_split(request.view.split);
        panes.set_row(request.view.row);
        let size = OutputSize::of(request.size[0], request.size[1]);
        let size = match request.kind {
            CaptureKind::Movie => size.even(),
            CaptureKind::Still { .. } | CaptureKind::Sequence => size,
        };
        let panels = Panels::of(&panes, size.to_array());
        let views = panes.views();
        let (destination, poses, fps) = match request.kind {
            CaptureKind::Still { time } => (
                Destination::Image {
                    path: path.to_path_buf(),
                },
                vec![Pose { time, views }],
                0,
            ),
            CaptureKind::Movie => {
                let poses = grana_offline::poses(&request.view.timeline);
                if poses.is_empty() {
                    return Err(format!(
                        "the timeline of {} holds no keyframe to film",
                        path.display()
                    ));
                }
                (
                    Destination::Movie {
                        path: path.to_path_buf(),
                    },
                    poses,
                    request.view.timeline.fps(),
                )
            }
            CaptureKind::Sequence => {
                let frames = scene.frame_count();
                if frames == 0 {
                    return Err(format!("{} holds no frame to render", path.display()));
                }
                let poses = (0..frames).map(|time| Pose { time, views }).collect();
                (
                    Destination::Sequence {
                        directory: path.to_path_buf(),
                        stem: grana_session::slug(scene.dataset.name()),
                    },
                    poses,
                    0,
                )
            }
        };
        let task = Task {
            plan: Plan {
                destination,
                panels,
                size,
                transparent: request.transparent,
                poses,
                fps,
                region: scene.region,
            },
            scene: scene.snapshot().into_scene(),
            bricks: Arc::new(ImageBricks::new(Channels::render(
                channels.image_shared(),
                scene,
            ))),
        };
        match offline.render(task) {
            Outcome::Complete { .. } => Ok(()),
            Outcome::Failed { detail } => Err(detail),
            Outcome::Cancelled => Err(format!("the capture at {} was cancelled", path.display())),
        }
    }
}
