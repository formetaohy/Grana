use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_display::{PaneKind, Panes};
use grana_engine::Panels;
use grana_offline::{Bricks, Destination, Offline, Outcome, Plan, Pose, Task};
use grana_scene::{Dataset, Scene};
use grana_ui::{Rect, Vec2};
use grana_volume::{
    BrickData, BrickGrid, BrickRequest, Extent, Pyramid, Region, Spacing, Volume, VoxelData,
};
use wgpu::TextureFormat;

use crate::support::{Gpu, Temp, gpu, spheres};

const STAGE: [f32; 2] = [1000.0, 700.0];
const WIDTH: u32 = 800;

struct VolumeBricks {
    volume: Volume,
    pyramid: Pyramid,
}

impl Bricks for VolumeBricks {
    fn read(&self, request: BrickRequest, _shift: [f32; 3]) -> Result<BrickData, String> {
        let grid = BrickGrid::new(self.pyramid.level(request.level).extent());
        Ok(self.volume.brick(&grid, request.coord))
    }
}

fn export_volume() -> Volume {
    let extent = Extent::new(128, 128, 64);
    let spacing = Spacing::new(0.5, 0.5, 1.0);
    spheres(
        extent,
        spacing,
        &[([64.0, 64.0, 32.0], 22.0), ([30.0, 40.0, 24.0], 12.0)],
        60_000,
    )
}

fn panels() -> Panels {
    let rect =
        |x: f32, width: f32| Rect::from_min_size(Vec2::new(x, 0.0), Vec2::new(width, STAGE[1]));
    Panels::new([
        Some((rect(0.0, 600.0), PaneKind::Volume)),
        Some((rect(600.0, 400.0), PaneKind::Section)),
        None,
        None,
    ])
}

fn offline(gpu: &Gpu) -> Offline {
    Offline::new(
        gpu.device().clone(),
        gpu.queue().clone(),
        TextureFormat::Rgba8Unorm,
    )
}

fn task(volume: Volume, plan: Plan) -> Task {
    let pyramid = volume.pyramid();
    let scene = Scene::new(Dataset::synthetic("export", pyramid.clone()));
    Task {
        plan,
        scene,
        bricks: Arc::new(VolumeBricks { volume, pyramid }),
    }
}

fn export_image(gpu: &Gpu, destination: Destination, transparent: bool) -> [u32; 2] {
    let volume = export_volume();
    let pyramid = volume.pyramid();
    let scene = Scene::new(Dataset::synthetic("export", pyramid.clone()));
    let mut panes = Panes::new();
    panes.fit(
        scene.dataset.world_size(),
        scene.dataset.voxel_size(),
        [600.0 / STAGE[1], 400.0 / STAGE[1], 1.0, 1.0],
    );
    let panels = panels();
    let regions = panels.export(WIDTH, false);
    let plan = Plan {
        destination,
        panels,
        width: WIDTH,
        transparent,
        poses: vec![Pose {
            time: 0,
            views: panes.views(),
        }],
        fps: 0,
        region: Region::none(),
    };
    let mut context = offline(gpu);
    match context.run(task(volume, plan), &AtomicBool::new(false), &mut |_, _| {}) {
        Outcome::Complete { .. } => regions.size,
        Outcome::Failed { detail } => panic!("the export failed: {detail}"),
        Outcome::Cancelled => panic!("the export was cancelled"),
    }
}

fn read_plane(image: &grana_tiff::Image, channel: usize, size: [u32; 2]) -> Vec<u16> {
    let data = image
        .read_region(0, 0, channel, 0..size[0], 0..size[1], 0..1)
        .expect("the exported plane reads");
    let VoxelData::U16(values) = data else {
        panic!("the export stores sixteen bit voxels");
    };
    values
}

#[test]
fn exports_a_two_view_image_at_the_requested_size() {
    let Some(gpu) = gpu() else {
        return;
    };
    let temp = Temp::new("grana-export");
    let path = temp.path().join("image.png");
    let size = export_image(&gpu, Destination::Image { path: path.clone() }, false);
    let image = grana_raster::read(&path).expect("the export writes a readable image");
    assert_eq!(image.size(), size);
    assert_eq!(image.width(), WIDTH);
    assert!(
        image.pixels().iter().any(|value| *value > 0),
        "the export writes the rendered scene"
    );
}

#[test]
fn exports_a_sixteen_bit_ome_tiff() {
    let Some(gpu) = gpu() else {
        return;
    };
    let temp = Temp::new("grana-export-wide");
    let path = temp.path().join("image.ome.tif");
    let size = export_image(&gpu, Destination::Image { path: path.clone() }, false);
    let image = grana_tiff::Image::open(&path).expect("the wide export reads");
    assert_eq!(image.name(), "export");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 3);
    assert_eq!(pyramid.level(0).extent(), Extent::new(size[0], size[1], 1));
    assert_eq!(pyramid.channels()[0].name(), "Red");
    let values = read_plane(&image, 0, size);
    assert!(
        values.iter().filter(|value| **value > 512).count() > 1000,
        "the wide export writes the rendered scene"
    );
    let beyond = values.iter().filter(|value| **value % 257 != 0).count();
    assert!(
        beyond > values.len() / 16,
        "the wide export keeps more than eight bits of precision"
    );
}

#[test]
fn exports_a_transparent_sixteen_bit_ome_tiff() {
    let Some(gpu) = gpu() else {
        return;
    };
    let temp = Temp::new("grana-export-wide-alpha");
    let path = temp.path().join("image.ome.tif");
    let size = export_image(&gpu, Destination::Image { path: path.clone() }, true);
    let image = grana_tiff::Image::open(&path).expect("the transparent wide export reads");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 4);
    assert_eq!(pyramid.channels()[3].name(), "Alpha");
    let alpha = read_plane(&image, 3, size);
    assert!(
        alpha.iter().any(|value| *value > 32_768),
        "the transparent export covers some pixels"
    );
    assert!(
        alpha.contains(&0),
        "the transparent export leaves some pixels empty"
    );
}

fn still_plan(volume: &Volume, destination: Destination, width: u32) -> Plan {
    let mut panes = Panes::new();
    let spacing = volume.spacing();
    panes.fit(
        Vec3::from(volume.world_extent()),
        Vec3::new(spacing.x(), spacing.y(), spacing.z()),
        [600.0 / STAGE[1], 400.0 / STAGE[1], 1.0, 1.0],
    );
    Plan {
        destination,
        panels: panels(),
        width,
        transparent: false,
        poses: vec![Pose {
            time: 0,
            views: panes.views(),
        }],
        fps: 0,
        region: Region::none(),
    }
}

fn rejects(gpu: &Gpu, plan: Plan, volume: Volume) -> String {
    let mut context = offline(gpu);
    match context.run(task(volume, plan), &AtomicBool::new(false), &mut |_, _| {}) {
        Outcome::Failed { detail } => detail,
        Outcome::Complete { .. } => panic!("the export was accepted"),
        Outcome::Cancelled => panic!("the export was cancelled"),
    }
}

#[test]
fn rejects_a_sixteen_bit_still_beyond_the_texture_ceiling() {
    let Some(gpu) = gpu() else {
        return;
    };
    let temp = Temp::new("grana-export-huge");
    let volume = export_volume();
    let plan = still_plan(
        &volume,
        Destination::Image {
            path: temp.path().join("huge.ome.tif"),
        },
        16384,
    );
    let error = rejects(&gpu, plan, volume);
    assert!(
        error.contains("texture memory"),
        "the rejection names the wide ceiling: {error}"
    );
}

#[test]
fn rejects_a_still_without_a_known_extension() {
    let Some(gpu) = gpu() else {
        return;
    };
    let temp = Temp::new("grana-export-unknown");
    let volume = export_volume();
    let plan = still_plan(
        &volume,
        Destination::Image {
            path: temp.path().join("image.bmp"),
        },
        WIDTH,
    );
    let error = rejects(&gpu, plan, volume);
    assert!(
        error.contains("OME-TIFF"),
        "the rejection names the supported stills: {error}"
    );
}

#[test]
fn cancels_an_export_without_leaving_a_partial_file() {
    let Some(gpu) = gpu() else {
        return;
    };
    let temp = Temp::new("grana-export-cancel");
    let path = temp.path().join("movie.mp4");
    let volume = export_volume();
    let pyramid = volume.pyramid();
    let scene = Scene::new(Dataset::synthetic("export", pyramid.clone()));
    let mut panes = Panes::new();
    panes.fit(
        scene.dataset.world_size(),
        scene.dataset.voxel_size(),
        [1.0; 4],
    );
    let plan = Plan {
        destination: Destination::Movie { path: path.clone() },
        panels: panels(),
        width: WIDTH,
        transparent: false,
        poses: vec![
            Pose {
                time: 0,
                views: panes.views(),
            },
            Pose {
                time: 0,
                views: panes.views(),
            },
        ],
        fps: 30,
        region: Region::none(),
    };
    let cancel = AtomicBool::new(true);
    let mut context = offline(&gpu);
    match context.run(task(volume, plan), &cancel, &mut |_, _| {}) {
        Outcome::Cancelled => {}
        Outcome::Complete { .. } => panic!("the export was not cancelled"),
        Outcome::Failed { detail } => panic!("the export failed: {detail}"),
    }
    assert!(
        !path.exists(),
        "a cancelled movie leaves no partial file behind"
    );
}

#[test]
fn runs_an_export_on_the_worker_thread() {
    let temp = Temp::new("grana-export-worker");
    let path = temp.path().join("image.png");
    let volume = export_volume();
    let pyramid = volume.pyramid();
    let scene = Scene::new(Dataset::synthetic("export", pyramid.clone()));
    let mut panes = Panes::new();
    panes.fit(
        scene.dataset.world_size(),
        scene.dataset.voxel_size(),
        [1.0; 4],
    );
    let plan = Plan {
        destination: Destination::Image { path: path.clone() },
        panels: panels(),
        width: 320,
        transparent: false,
        poses: vec![Pose {
            time: 0,
            views: panes.views(),
        }],
        fps: 0,
        region: Region::none(),
    };
    let mut worker = grana_offline::Worker::spawn();
    let handle = worker.submit(task(volume, plan));
    let started = std::time::Instant::now();
    let mut progress = Vec::new();
    let finished = loop {
        let (events, alive) = worker.drain();
        let mut finished = None;
        for event in events {
            match event {
                grana_offline::Event::Progress { frame, total, .. } => {
                    progress.push((frame, total));
                }
                grana_offline::Event::Finished { outcome, .. } => finished = Some(outcome),
            }
        }
        if let Some(outcome) = finished {
            break outcome;
        }
        assert!(alive, "the export worker stopped");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(120),
            "the export worker never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(handle.id(), 1);
    match finished {
        Outcome::Complete { path: saved } => assert_eq!(saved, path),
        Outcome::Failed { detail } => panic!("the export failed: {detail}"),
        Outcome::Cancelled => panic!("the export was cancelled"),
    }
    assert!(path.exists(), "the worker writes the exported image");
    assert!(
        progress.contains(&(1, 1)),
        "the worker reports the finished frame"
    );
}

#[test]
fn snapshots_a_scene_for_offline_rendering() {
    let volume = export_volume();
    let mut scene = Scene::new(Dataset::synthetic("export", volume.pyramid()));
    scene.exposure = 1.5;
    scene.shading.ambient = 0.6;
    scene.set_window(0, 10.0, 4_000.0);
    let restored = scene.snapshot().into_scene();
    assert_eq!(restored.dataset.name(), "export");
    assert_eq!(restored.exposure, 1.5);
    assert_eq!(restored.state(), scene.state());
}
