use grana_display::{PaneKind, Panes};
use grana_engine::{Destination, Engine, Outcome, Panels, Plan, Pose};
use grana_scene::{Dataset, Scene};
use grana_ui::{Rect, Ui, Vec2};
use grana_volume::{BrickData, BrickGrid, Extent, Pyramid, Region, Spacing, Volume};
use wgpu::TextureFormat;

use crate::support::{Gpu, Temp, gpu, spheres};

const STAGE: [f32; 2] = [1000.0, 700.0];
const WIDTH: u32 = 800;

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

fn upload(gpu: &Gpu, engine: &mut Engine, volume: &Volume, pyramid: &Pyramid) {
    let Some(requests) = engine.poll_requests(gpu.device()) else {
        return;
    };
    let mut bricks = Vec::new();
    for request in requests
        .volume
        .iter()
        .chain(&requests.section)
        .chain(&requests.prefetch)
    {
        let grid = BrickGrid::new(pyramid.level(request.level).extent());
        let data: BrickData = volume.brick(&grid, request.coord);
        bricks.push((*request, data));
    }
    engine.upload(gpu.queue(), &bricks);
}

#[test]
fn exports_a_two_view_image_at_the_requested_size() {
    let Some(gpu) = gpu() else {
        return;
    };
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
    let mut engine = Engine::new(gpu.device(), gpu.queue(), TextureFormat::Rgba8Unorm);
    engine.set_dataset(gpu.device(), &pyramid);
    let temp = Temp::new("grana-export");
    let path = temp.path().join("image.png");
    engine
        .start_export(
            gpu.device(),
            Plan {
                destination: Destination::Image { path: path.clone() },
                panels,
                width: WIDTH,
                transparent: false,
                poses: vec![Pose {
                    time: 0,
                    views: panes.views(),
                }],
                fps: 0,
                region: Region::none(),
            },
        )
        .expect("the export fits the device");
    let mut ui: Ui<()> = Ui::new(gpu.device(), gpu.queue(), TextureFormat::Rgba8Unorm);
    ui.resize(regions.size);
    let mut done = None;
    for _ in 0..4096 {
        upload(&gpu, &mut engine, &volume, &pyramid);
        match engine.render_export(gpu.device(), gpu.queue(), &mut ui, &scene, 0) {
            Outcome::Running => {}
            Outcome::Complete { path } => {
                done = Some(path);
                break;
            }
            Outcome::Failed { detail } => panic!("the export failed: {detail}"),
        }
    }
    let written = done.expect("the export never completed");
    assert_eq!(written, path);
    let image = grana_raster::read(&written).expect("the export writes a readable image");
    assert_eq!(image.size(), regions.size);
    assert_eq!(image.width(), WIDTH);
    assert!(
        image.pixels().iter().any(|value| *value > 0),
        "the export writes the rendered scene"
    );
}
