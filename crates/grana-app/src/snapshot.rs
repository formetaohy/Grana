use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use grana_display::{PANE_GAP, Panes};
use grana_engine::Panels;
use grana_offline::{Destination, Offline, Outcome, Plan, Pose, Task};
use grana_pipeline::{Channels, SnapshotRequest, Snapshotter};
use grana_scene::Scene;
use grana_ui::{Rect, Vec2};

use crate::bricks::ImageBricks;

const STAGE_WIDTH: f32 = 1000.0;

pub struct Snapshot {
    state: Mutex<Option<Offline>>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self::new()
    }
}

impl Snapshot {
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

impl Snapshotter for Snapshot {
    fn open(&self) -> Result<(), String> {
        self.state().map(|_| ())
    }

    fn snapshot(
        &self,
        channels: &Channels,
        scene: &Scene,
        request: &SnapshotRequest,
        path: &Path,
    ) -> Result<(), String> {
        let mut slot = self.state()?;
        let offline = slot.as_mut().expect("the offline context is ready");
        let mut panes = Panes::new();
        if let Some(view) = &request.view {
            panes.restore(view.panes);
            panes.set_layout(view.layout);
            panes.set_split(view.split);
            panes.set_row(view.row);
        }
        let panels = panels(&panes, request.size);
        let regions = panels.export(request.size[0], false);
        if request.view.is_none() {
            panes.fit(
                scene.dataset.world_size(),
                scene.dataset.voxel_size(),
                std::array::from_fn(|index| {
                    regions.panes[index].map_or(1.0, |pane| aspect(pane.region))
                }),
            );
        }
        let task = Task {
            plan: Plan {
                destination: Destination::Image {
                    path: path.to_path_buf(),
                },
                panels,
                width: request.size[0],
                transparent: request.transparent,
                poses: vec![Pose {
                    time: request.time,
                    views: panes.views(),
                }],
                fps: 0,
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
            Outcome::Cancelled => Err(format!(
                "the quality snapshot at {} was cancelled",
                path.display()
            )),
        }
    }
}

fn panels(panes: &Panes, size: [u32; 2]) -> Panels {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a quality snapshot covers at least one pixel"
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

fn aspect(region: [u32; 4]) -> f32 {
    region[2].max(1) as f32 / region[3].max(1) as f32
}
