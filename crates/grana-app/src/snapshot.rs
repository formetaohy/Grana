use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use grana_display::{PANE_COUNT, PaneKind, Panes};
use grana_engine::{Destination, Engine, Outcome, Panels, Plan, Pose};
use grana_gpu::Headless;
use grana_pipeline::{Channels, SnapshotRequest, Snapshotter};
use grana_scene::Scene;
use grana_ui::{Rect, Ui, Vec2};

const STAGE_WIDTH: f32 = 1000.0;
const STAGE_GAP: f32 = 6.0;
const ATTEMPTS: usize = 4096;

pub struct Snapshot {
    state: Mutex<Option<State>>,
}

struct State {
    gpu: Headless,
    engine: Engine,
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

    fn state(&self) -> Result<MutexGuard<'_, Option<State>>, String> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.is_none() {
            let gpu = Headless::new()?;
            let engine = Engine::new(&gpu.device, &gpu.queue, gpu.format());
            *state = Some(State { gpu, engine });
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
        let State { gpu, engine } = slot.as_mut().expect("the snapshot state is ready");
        let mut panes = Panes::new();
        if let Some(view) = &request.view {
            panes.restore(view.panes);
            panes.set_split(view.split);
        }
        let kinds: [PaneKind; PANE_COUNT] = std::array::from_fn(|index| panes.pane(index).kind());
        let panels = panels(panes.split(), kinds, request.size);
        let regions = panels.export(request.size[0], false);
        if request.view.is_none() {
            panes.fit(
                scene.dataset.world_size(),
                scene.dataset.voxel_size(),
                std::array::from_fn(|index| aspect(regions.panes[index].region)),
            );
        }
        let mut ui: Ui<()> = Ui::new(&gpu.device, &gpu.queue, gpu.format());
        ui.resize(regions.size);
        engine.set_dataset(&gpu.device, scene.dataset.pyramid());
        engine.start_export(
            &gpu.device,
            Plan {
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
            },
        )?;
        let outcome = render(engine, gpu, &mut ui, channels, scene, path);
        if outcome.is_err() {
            engine.cancel_export();
        }
        outcome
    }
}

fn render(
    engine: &mut Engine,
    gpu: &Headless,
    ui: &mut Ui,
    channels: &Channels,
    scene: &Scene,
    path: &Path,
) -> Result<(), String> {
    for _ in 0..ATTEMPTS {
        pump(engine, gpu, channels, scene.drift())?;
        match engine.render_export(&gpu.device, &gpu.queue, ui, scene, 0) {
            Outcome::Running => {}
            Outcome::Complete { .. } => return Ok(()),
            Outcome::Failed { detail } => return Err(detail),
        }
    }
    Err(format!(
        "the quality snapshot at {} did not settle",
        path.display()
    ))
}

fn panels(split: f32, kinds: [PaneKind; PANE_COUNT], size: [u32; 2]) -> Panels {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a quality snapshot covers at least one pixel"
    );
    let split = split.clamp(grana_display::SPLIT_MIN, grana_display::SPLIT_MAX);
    let height = STAGE_WIDTH * size[1] as f32 / size[0] as f32;
    let left = (STAGE_WIDTH - STAGE_GAP) * split;
    let rects = [
        Rect::from_min_max(Vec2::ZERO, Vec2::new(left, height)),
        Rect::from_min_max(
            Vec2::new(left + STAGE_GAP, 0.0),
            Vec2::new(STAGE_WIDTH, height),
        ),
    ];
    Panels::new(std::array::from_fn(|index| (rects[index], kinds[index])))
}

fn aspect(region: [u32; 4]) -> f32 {
    region[2].max(1) as f32 / region[3].max(1) as f32
}

fn pump(
    engine: &mut Engine,
    gpu: &Headless,
    channels: &Channels,
    drift: &grana_scene::Drift,
) -> Result<(), String> {
    let Some(requests) = engine.poll_requests(&gpu.device) else {
        return Ok(());
    };
    let mut bricks = Vec::with_capacity(
        requests.volume.len() + requests.section.len() + requests.prefetch.len(),
    );
    for request in requests
        .volume
        .iter()
        .chain(&requests.section)
        .chain(&requests.prefetch)
    {
        let data = channels
            .read_brick(
                request.time,
                request.level,
                request.coord,
                drift.offset(request.time),
            )
            .map_err(|error| error.to_string())?;
        bricks.push((*request, data));
    }
    engine.upload(&gpu.queue, &bricks);
    Ok(())
}
