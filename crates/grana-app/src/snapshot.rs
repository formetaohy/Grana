use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use grana_display::{Camera, CameraState, SectionCamera, SectionState, Viewport};
use grana_engine::{Destination, Engine, Outcome, Panels, Panes, Plan, Pose};
use grana_gpu::Headless;
use grana_image::Image;
use grana_pipeline::{SnapshotRequest, Snapshotter};
use grana_scene::Scene;
use grana_ui::{Rect, Ui, Vec2};
use grana_volume::Axis;

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
        reader: &Image,
        scene: &Scene,
        request: &SnapshotRequest,
        path: &Path,
    ) -> Result<(), String> {
        let mut slot = self.state()?;
        let State { gpu, engine } = slot.as_mut().expect("the snapshot state is ready");
        let split = request
            .view
            .as_ref()
            .map_or_else(|| Viewport::new().split(), |view| view.split);
        let panels = panels(split, request.size);
        let panes = panels.export(request.size[0], false);
        let (camera, section) = match &request.view {
            Some(view) => (view.camera, view.section),
            None => fitted(scene, &panes),
        };
        let mut ui: Ui<()> = Ui::new(&gpu.device, &gpu.queue, gpu.format());
        ui.resize(panes.size);
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
                    camera,
                    section,
                }],
                fps: 0,
            },
        )?;
        let outcome = render(engine, gpu, &mut ui, reader, scene, path);
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
    reader: &Image,
    scene: &Scene,
    path: &Path,
) -> Result<(), String> {
    for _ in 0..ATTEMPTS {
        pump(engine, gpu, reader, scene.drift())?;
        match engine.render_export(&gpu.device, &gpu.queue, ui, scene) {
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

fn panels(split: f32, size: [u32; 2]) -> Panels {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a quality snapshot covers at least one pixel"
    );
    let split = split.clamp(0.25, 0.8);
    let height = STAGE_WIDTH * size[1] as f32 / size[0] as f32;
    let volume_width = (STAGE_WIDTH - STAGE_GAP) * split;
    let volume = Rect::from_min_max(Vec2::ZERO, Vec2::new(volume_width, height));
    let section = Rect::from_min_max(
        Vec2::new(volume_width + STAGE_GAP, 0.0),
        Vec2::new(STAGE_WIDTH, height),
    );
    Panels::new(volume, section)
}

fn fitted(scene: &Scene, panes: &Panes) -> (CameraState, SectionState) {
    let world = scene.dataset.world_size();
    let mut camera = Camera::new(35.0);
    camera.fit(world, aspect(panes.volume));
    let mut section = SectionCamera::new(Axis::Z);
    section.fit(world, scene.dataset.voxel_size(), aspect(panes.section));
    (camera.state(), section.state())
}

fn aspect(region: [u32; 4]) -> f32 {
    region[2].max(1) as f32 / region[3].max(1) as f32
}

fn pump(
    engine: &mut Engine,
    gpu: &Headless,
    reader: &Image,
    drift: &grana_scene::Drift,
) -> Result<(), String> {
    let Some(requests) = engine.poll_requests(&gpu.device) else {
        return Ok(());
    };
    for request in requests
        .volume
        .iter()
        .chain(&requests.section)
        .chain(&requests.prefetch)
    {
        let data = reader
            .read_brick(
                request.time,
                request.level,
                request.coord,
                drift.offset(request.time),
            )
            .map_err(|error| error.to_string())?;
        engine.upload(&gpu.device, &gpu.queue, *request, &data);
    }
    Ok(())
}
