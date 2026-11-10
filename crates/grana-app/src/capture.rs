use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use grana_display::{PANE_GAP, Panes};
use grana_engine::Panels;
use grana_offline::{Destination, Offline, Outcome, Plan, Pose, Task};
use grana_pipeline::{Capture, CaptureKind, CaptureRequest, Channels};
use grana_scene::Scene;
use grana_ui::{Rect, Vec2};

use crate::bricks::ImageBricks;

const STAGE_WIDTH: f32 = 1000.0;

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
        let panels = panels(&panes, request.size);
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
                width: request.size[0],
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

fn panels(panes: &Panes, size: [u32; 2]) -> Panels {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a capture covers at least one pixel"
    );
    let height = STAGE_WIDTH * size[1] as f32 / size[0] as f32;
    let rects = panes.rects([0.0, 0.0, STAGE_WIDTH, height], PANE_GAP);
    Panels::new(std::array::from_fn(|index| {
        panes.visible(index).then(|| {
            (
                Rect::from_min_size(
                    Vec2::new(rects[index][0], rects[index][1]),
                    Vec2::new(rects[index][2], rects[index][3]),
                ),
                panes.pane(index).kind(),
            )
        })
    }))
}
