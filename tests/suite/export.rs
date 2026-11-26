use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_display::{Camera, OutputSize, Slice};
use grana_offline::{Bricks, Destination, Offline, Outcome, Plan, Pose, Task};
use grana_scene::{Dataset, ObjectKind, Scene};
use grana_volume::{
    BrickData, BrickGrid, BrickRequest, Calibration, ChannelInfo, Extent, Intensity, Level, NoData,
    PADDED_SIDE, Pyramid, Region, Spacing, Statistics, Volume, VoxelData, VoxelFormat,
    VoxelSemantics,
};
use wgpu::TextureFormat;

use crate::support::{Gpu, Temp, gpu, spheres};

const ASPECT: f32 = 0.7;
const WIDTH: u32 = 800;

struct VolumeBricks {
    volume: Volume,
    pyramid: Pyramid,
}

impl Bricks for VolumeBricks {
    fn read(&self, request: BrickRequest, _warps: &grana_warp::Warps) -> Result<BrickData, String> {
        let grid = BrickGrid::new(self.pyramid.level(request.level).extent());
        Ok(self.volume.brick(&grid, request.coord))
    }

    fn empty(
        &self,
        _request: BrickRequest,
        _warps: &grana_warp::Warps,
    ) -> Result<Option<NoData>, String> {
        Ok(None)
    }
}

struct EmptyBricks {
    channels: usize,
}

impl Bricks for EmptyBricks {
    fn read(
        &self,
        _request: BrickRequest,
        _warps: &grana_warp::Warps,
    ) -> Result<BrickData, String> {
        let voxels = (PADDED_SIDE as usize).pow(3);
        Ok(BrickData::new(
            (0..self.channels)
                .map(|_| VoxelData::U16(vec![0u16; voxels]))
                .collect(),
        ))
    }

    fn empty(
        &self,
        _request: BrickRequest,
        _warps: &grana_warp::Warps,
    ) -> Result<Option<NoData>, String> {
        Ok(Some(NoData::U16(0)))
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

fn output(width: u32, even: bool) -> OutputSize {
    let height = ((width as f32 * ASPECT).round() as u32).max(1);
    let size = OutputSize::of(width, height);
    if even { size.even() } else { size }
}

fn aspect(size: OutputSize) -> f32 {
    size.width() as f32 / size.height().max(1) as f32
}

fn offline(gpu: &Gpu) -> Offline {
    Offline::new(
        gpu.device().clone(),
        gpu.queue().clone(),
        TextureFormat::Rgba8Unorm,
        &crate::support::ledger(),
        512 << 20,
        gpu.pipelines(),
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
    let size = output(WIDTH, false);
    let mut camera = Camera::new(35.0);
    camera.fit(scene.dataset.world_size(), aspect(size));
    let plan = Plan {
        destination,
        size,
        transparent,
        poses: vec![Pose {
            time: 0,
            camera: camera.state(),
        }],
        fps: 0,
        region: Region::none(),
    };
    let mut context = offline(gpu);
    match context.run(task(volume, plan), &AtomicBool::new(false), &mut |_, _| {}) {
        Outcome::Complete { .. } => size.to_array(),
        Outcome::Failed { detail } => panic!("the export failed: {detail}"),
        Outcome::Cancelled => panic!("the export was cancelled"),
    }
}

#[test]
fn exports_vector_figures_as_svg_and_pdf() {
    let Some(gpu) = gpu() else {
        return;
    };
    let temp = Temp::new("grana-export-figure");
    let svg = temp.path().join("figure.svg");
    export_image(&gpu, Destination::Figure { path: svg.clone() }, false);
    let markup = std::fs::read_to_string(&svg).expect("the vector figure reads as text");
    assert!(markup.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
    assert!(markup.contains("<svg "));
    assert!(markup.contains("<image "));
    assert!(markup.contains("<path d=\"M "));
    assert!(
        !markup.contains("<text"),
        "the figure letters are stored as outlines"
    );
    assert!(markup.contains("fill-opacity=\"0.6\""));
    let pdf = temp.path().join("figure.pdf");
    export_image(&gpu, Destination::Figure { path: pdf.clone() }, false);
    let bytes = std::fs::read(&pdf).expect("the vector figure reads as bytes");
    assert!(bytes.starts_with(b"%PDF-1.7\n"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    assert!(find(&bytes, b"/Subtype /Image").is_some());
    assert!(find(&bytes, b"/MediaBox [0 0 800 560]").is_some());
    let start = find(&bytes, b"startxref\n").expect("the cross reference table is announced") + 10;
    let text = std::str::from_utf8(&bytes[start..])
        .expect("the cross reference offset is written in ascii")
        .lines()
        .next()
        .expect("the cross reference offset stands on its own line")
        .trim()
        .parse::<usize>()
        .expect("the cross reference offset is a number");
    assert_eq!(&bytes[text..text + 4], b"xref");
    let transparent = temp.path().join("figure-transparent.pdf");
    export_image(
        &gpu,
        Destination::Figure {
            path: transparent.clone(),
        },
        true,
    );
    let bytes = std::fs::read(&transparent).expect("the transparent figure reads as bytes");
    assert!(
        find(&bytes, b"/SMask").is_some(),
        "a transparent figure masks the backdrop"
    );
}

fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .rposition(|window| window == needle)
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
    let size = output(WIDTH, true);
    let mut camera = Camera::new(35.0);
    camera.fit(scene.dataset.world_size(), aspect(size));
    let view = camera.state();
    let plan = Plan {
        destination: Destination::Movie { path: path.clone() },
        size,
        transparent: false,
        poses: vec![
            Pose {
                time: 0,
                camera: view,
            },
            Pose {
                time: 0,
                camera: view,
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
    let staging = std::fs::read_dir(temp.path())
        .expect("the temporary directory lists")
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".part"))
        .count();
    assert_eq!(
        staging, 0,
        "a cancelled movie leaves no staging file behind"
    );
}

#[test]
fn paints_the_scene_overlay_in_an_export() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = export_volume();
    let pyramid = volume.pyramid();
    let mut scene = Scene::new(Dataset::synthetic("export", pyramid.clone()));
    let set = scene.add_object(ObjectKind::Spots, 0);
    let size = output(WIDTH, false);
    let mut camera = Camera::new(35.0);
    camera.fit(scene.dataset.world_size(), aspect(size));
    let half = 0.5 * scene.dataset.world_size();
    let corners = [
        [-half.x, -half.y, 0.0],
        [half.x, -half.y, 0.0],
        [half.x, half.y, 0.0],
        [-half.x, half.y, 0.0],
    ];
    let pixels = Arc::from(vec![255u8; 64 * 64 * 4]);
    let bitmap = grana_raster::Bitmap::new([64, 64], pixels);
    let overlay = grana_raster::Overlay::new(bitmap, corners, [1.0, 0.0, 0.0], 1.0);
    assert!(
        scene.overlays.set(set, overlay),
        "the scene holds the overlay it was given"
    );
    let temp = Temp::new("grana-export-overlay");
    let path = temp.path().join("overlay.png");
    let plan = Plan {
        destination: Destination::Image { path: path.clone() },
        size,
        transparent: false,
        poses: vec![Pose {
            time: 0,
            camera: camera.state(),
        }],
        fps: 0,
        region: Region::none(),
    };
    let task = Task {
        plan,
        scene: scene.snapshot().into_scene(),
        bricks: Arc::new(VolumeBricks { volume, pyramid }),
    };
    match offline(&gpu).run(task, &AtomicBool::new(false), &mut |_, _| {}) {
        Outcome::Complete { .. } => {}
        Outcome::Failed { detail } => panic!("the overlay export failed: {detail}"),
        Outcome::Cancelled => panic!("the overlay export was cancelled"),
    }
    let image = grana_raster::read(&path).expect("the overlay export writes an image");
    let total = (image.width() * image.height()) as usize;
    let red = image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| {
            i32::from(pixel[0]) > 120
                && i32::from(pixel[0]) > i32::from(pixel[1]) + 40
                && i32::from(pixel[0]) > i32::from(pixel[2]) + 40
        })
        .count();
    assert!(
        red > total / 50,
        "the export paints {red} of {total} pixels with the overlay"
    );
}

#[test]
fn exports_a_slice_view() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = export_volume();
    let pyramid = volume.pyramid();
    let scene = Scene::new(Dataset::synthetic("export", pyramid.clone()));
    let size = output(WIDTH, false);
    let level = pyramid.level(0);
    let frame = level.slice_frame(2);
    let mut camera = Camera::new(35.0);
    camera.set_slice(Slice::new(2, frame.right(), frame.up(), frame.step()));
    camera.fit_slice(
        Vec3::from(level.slice_center(2, 32)),
        frame.half_width(),
        frame.half_height(),
        aspect(size),
    );
    let temp = Temp::new("grana-export-slice");
    let path = temp.path().join("slice.png");
    let plan = Plan {
        destination: Destination::Image { path: path.clone() },
        size,
        transparent: false,
        poses: vec![Pose {
            time: 0,
            camera: camera.state(),
        }],
        fps: 0,
        region: Region::none(),
    };
    let task = Task {
        plan,
        scene,
        bricks: Arc::new(VolumeBricks { volume, pyramid }),
    };
    match offline(&gpu).run(task, &AtomicBool::new(false), &mut |_, _| {}) {
        Outcome::Complete { .. } => {}
        Outcome::Failed { detail } => panic!("the slice export failed: {detail}"),
        Outcome::Cancelled => panic!("the slice export was cancelled"),
    }
    let image = grana_raster::read(&path).expect("the slice export writes an image");
    assert_eq!(image.size(), size.to_array());
    let [width, height] = size.to_array();
    let total = (width * height) as usize;
    let bright = |pixel: &[u8]| pixel[..3].iter().any(|value| *value > 120);
    let painted = image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| bright(&pixel[..]))
        .count();
    assert!(
        painted > total / 200,
        "the slice export paints {painted} of {total} pixels"
    );
    assert!(
        painted < total / 2,
        "the slice export paints {painted} of {total} pixels"
    );
    let centre = 4 * ((height / 2) * width + width / 2) as usize;
    assert!(
        bright(&image.pixels()[centre..]),
        "the slice export misses the middle of the volume"
    );
    assert!(
        !bright(&image.pixels()[..4]),
        "the slice export fills the corner of the frame"
    );
}

#[test]
fn reserves_only_the_memory_an_export_uses() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = export_volume();
    let pyramid = volume.pyramid();
    let scene = Scene::new(Dataset::synthetic("export", pyramid.clone()));
    let size = output(WIDTH, false);
    let mut camera = Camera::new(35.0);
    camera.fit(scene.dataset.world_size(), aspect(size));
    let directory = Temp::new("export-claim");
    let path = directory.path().join("claim.tif");
    let plan = Plan {
        destination: Destination::Image { path },
        size,
        transparent: false,
        poses: vec![Pose {
            time: 0,
            camera: camera.state(),
        }],
        fps: 0,
        region: Region::none(),
    };
    let ledger = Arc::new(grana_gpu::Ledger::new(1 << 30));
    let mut context = Offline::new(
        gpu.device().clone(),
        gpu.queue().clone(),
        TextureFormat::Rgba8Unorm,
        &ledger,
        512 << 20,
        gpu.pipelines(),
    );
    match context.run(task(volume, plan), &AtomicBool::new(false), &mut |_, _| {}) {
        Outcome::Complete { .. } => {}
        Outcome::Failed { detail } => {
            panic!("the export claims more memory than the residency it fills: {detail}")
        }
        Outcome::Cancelled => panic!("the export was cancelled"),
    }
    assert_eq!(
        ledger.claimed(grana_gpu::Consumer::Export),
        0,
        "an export releases the textures it claimed"
    );
}

#[test]
fn refuses_an_export_whose_textures_exceed_the_device_allowance() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = export_volume();
    let pyramid = volume.pyramid();
    let scene = Scene::new(Dataset::synthetic("export", pyramid.clone()));
    let size = OutputSize::of(2048, 2048);
    let mut camera = Camera::new(35.0);
    camera.fit(scene.dataset.world_size(), aspect(size));
    let directory = Temp::new("export-refused");
    let path = directory.path().join("refused.png");
    let plan = Plan {
        destination: Destination::Image { path },
        size,
        transparent: false,
        poses: vec![Pose {
            time: 0,
            camera: camera.state(),
        }],
        fps: 0,
        region: Region::none(),
    };
    let ledger = Arc::new(grana_gpu::Ledger::new(128 << 20));
    let mut context = Offline::new(
        gpu.device().clone(),
        gpu.queue().clone(),
        TextureFormat::Rgba8Unorm,
        &ledger,
        64 << 20,
        gpu.pipelines(),
    );
    match context.run(task(volume, plan), &AtomicBool::new(false), &mut |_, _| {}) {
        Outcome::Failed { detail } => {
            assert!(
                detail.contains("2048 × 2048")
                    && detail.contains("export textures")
                    && detail.contains("MiB"),
                "the refusal names the export size and the bytes it needs: {detail}"
            );
        }
        Outcome::Complete { .. } => panic!("an export larger than the device allowance completed"),
        Outcome::Cancelled => panic!("the export was cancelled"),
    }
    assert_eq!(
        ledger.claimed(grana_gpu::Consumer::Export),
        0,
        "a refused export releases the textures it claimed"
    );
}

#[test]
fn claims_the_bricks_an_empty_volume_holds_rather_than_the_bricks_the_view_covers() {
    let Some(gpu) = gpu() else {
        return;
    };
    let level = Level::new(Extent::new(1024, 1024, 64), Spacing::new(1.0, 1.0, 1.0));
    let pyramid = Pyramid::new(
        vec![level],
        1,
        vec![ChannelInfo::new(
            "signal",
            [1.0, 1.0, 1.0],
            Statistics::declared(Intensity::new(0.0, 1.0), VoxelSemantics::Scalar),
            VoxelFormat::U16,
        )],
        1,
        1.0,
        Calibration::Recorded,
    );
    let scene = Scene::new(Dataset::synthetic("empty", pyramid.clone()));
    let size = OutputSize::of(320, 224);
    let mut camera = Camera::new(35.0);
    camera.fit(scene.dataset.world_size(), aspect(size));
    let held = grana_render::residency(&pyramid, &camera, size.to_array(), Region::none());
    assert!(
        held.detail.bricks > 1000,
        "the fitted view covers {} of the 2048 bricks of the level",
        held.detail.bricks
    );
    let probed = grana_render::visible(&pyramid, &camera, size.to_array(), Region::none());
    assert_eq!(
        probed.bricks.len(),
        held.detail.bricks,
        "the bricks an export probes are the bricks its claim counts"
    );
    let temp = Temp::new("grana-export-empty");
    let path = temp.path().join("empty.png");
    let plan = Plan {
        destination: Destination::Image { path: path.clone() },
        size,
        transparent: false,
        poses: vec![Pose {
            time: 0,
            camera: camera.state(),
        }],
        fps: 0,
        region: Region::none(),
    };
    let ledger = Arc::new(grana_gpu::Ledger::new(200 << 20));
    let mut context = Offline::new(
        gpu.device().clone(),
        gpu.queue().clone(),
        TextureFormat::Rgba8Unorm,
        &ledger,
        100 << 20,
        gpu.pipelines(),
    );
    let task = Task {
        plan,
        scene,
        bricks: Arc::new(EmptyBricks { channels: 1 }),
    };
    match context.run(task, &AtomicBool::new(false), &mut |_, _| {}) {
        Outcome::Complete { .. } => {}
        Outcome::Failed { detail } => {
            panic!("an empty volume needs no brick residency to export: {detail}")
        }
        Outcome::Cancelled => panic!("the export was cancelled"),
    }
    assert!(path.exists(), "the empty export writes its frame");
}
