use glam::Vec3;
use grana_display::{PaneKind, Panes};
use grana_engine::{Destination, Engine, Outcome, Panels, Plan, Pose};
use grana_scene::{Dataset, Scene};
use grana_ui::{Rect, Ui, Vec2};
use grana_volume::{BrickData, BrickGrid, Extent, Pyramid, Region, Spacing, Volume, VoxelData};
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
    let mut engine = Engine::new(gpu.device(), gpu.queue(), TextureFormat::Rgba8Unorm);
    engine.set_dataset(gpu.device(), &pyramid);
    engine
        .start_export(
            gpu.device(),
            Plan {
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
            },
        )
        .expect("the export fits the device");
    let mut ui: Ui<()> = Ui::new(gpu.device(), gpu.queue(), TextureFormat::Rgba8Unorm);
    ui.resize(regions.size);
    for _ in 0..4096 {
        upload(gpu, &mut engine, &volume, &pyramid);
        match engine.render_export(gpu.device(), gpu.queue(), &mut ui, &scene, 0) {
            Outcome::Running => {}
            Outcome::Complete { .. } => return regions.size,
            Outcome::Failed { detail } => panic!("the export failed: {detail}"),
        }
    }
    panic!("the export never completed");
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

#[test]
fn rejects_a_sixteen_bit_still_beyond_the_texture_ceiling() {
    let Some(gpu) = gpu() else {
        return;
    };
    let temp = Temp::new("grana-export-huge");
    let volume = export_volume();
    let pyramid = volume.pyramid();
    let mut engine = Engine::new(gpu.device(), gpu.queue(), TextureFormat::Rgba8Unorm);
    engine.set_dataset(gpu.device(), &pyramid);
    let mut panes = Panes::new();
    let spacing = volume.spacing();
    panes.fit(
        Vec3::from(volume.world_extent()),
        Vec3::new(spacing.x(), spacing.y(), spacing.z()),
        [600.0 / STAGE[1], 400.0 / STAGE[1], 1.0, 1.0],
    );
    let error = engine
        .start_export(
            gpu.device(),
            Plan {
                destination: Destination::Image {
                    path: temp.path().join("huge.ome.tif"),
                },
                panels: panels(),
                width: 16384,
                transparent: false,
                poses: vec![Pose {
                    time: 0,
                    views: panes.views(),
                }],
                fps: 0,
                region: Region::none(),
            },
        )
        .expect_err("a still beyond the wide texture ceiling is rejected");
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
    let pyramid = volume.pyramid();
    let scene = Scene::new(Dataset::synthetic("export", pyramid.clone()));
    let mut engine = Engine::new(gpu.device(), gpu.queue(), TextureFormat::Rgba8Unorm);
    engine.set_dataset(gpu.device(), &pyramid);
    let mut panes = Panes::new();
    panes.fit(
        scene.dataset.world_size(),
        scene.dataset.voxel_size(),
        [1.0; 4],
    );
    let error = engine
        .start_export(
            gpu.device(),
            Plan {
                destination: Destination::Image {
                    path: temp.path().join("image.bmp"),
                },
                panels: panels(),
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
        .expect_err("an unknown still extension is rejected");
    assert!(
        error.contains("OME-TIFF"),
        "the rejection names the supported stills: {error}"
    );
}
