use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_display::{Camera, OutputSize, Slice};
use grana_object::{IntensityTable, SampleStatistics, Spot, SpotSet, Track, TrackNode, TrackSet};
use grana_offline::{Bricks, Destination, Offline, Outcome, Plan, Pose, Task};
use grana_scene::{Dataset, ObjectFrame, ObjectKind, Scene};
use grana_volume::{
    BrickData, BrickGrid, BrickRequest, Extent, Pyramid, Region, Spacing, Volume, VoxelData,
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

#[test]
fn exports_a_track_lineage_as_a_vector_page() {
    let Some(gpu) = gpu() else {
        return;
    };
    let temp = Temp::new("grana-export-lineage");
    let volume = export_volume();
    let base = volume.pyramid();
    let pyramid = Pyramid::new(
        base.levels().to_vec(),
        base.stored(),
        base.channels().to_vec(),
        4,
        1.0,
    );
    let mut scene = Scene::new(Dataset::synthetic("lineage", pyramid.clone()));
    let set = scene.add_object(ObjectKind::Spots, 0);
    for time in 0..4 {
        let spot = Spot::new([16.0 + time as f32, 64.0, 32.0], 4.0, 1.0);
        let intensities =
            IntensityTable::new(1, vec![SampleStatistics::new(60.0, 40.0, 80.0, 2.0, 60.0)]);
        scene.install(
            set,
            time,
            ObjectFrame::Spots(SpotSet::new(0, vec![spot], intensities)),
        );
    }
    let mother = Track::new(
        None,
        vec![
            TrackNode::new(0, 0, [16.0, 64.0, 32.0]),
            TrackNode::new(1, 0, [17.0, 64.0, 32.0]),
        ],
    );
    let daughter = Track::new(
        Some(0),
        vec![
            TrackNode::new(2, 0, [18.0, 64.0, 32.0]),
            TrackNode::new(3, 0, [19.0, 64.0, 32.0]),
        ],
    );
    let tracks = TrackSet::new(1.0, vec![mother, daughter]);
    assert_eq!(grana_object::TrackLayout::of(&tracks).order(), &[0, 1]);
    scene.install_tracks(set, tracks);
    let size = grana_figure::lineage::page_size(2, 960).expect("two tracks fit a page");
    assert_eq!(size, [960, 162]);
    assert!(
        grana_figure::lineage::page_size(2_000, 960).is_ok(),
        "a large lineage compresses its rows"
    );
    assert!(
        grana_figure::lineage::page_size(5_000, 960).is_err(),
        "a lineage past the page limit asks for paging"
    );
    let plan = Plan {
        destination: Destination::Lineage {
            path: temp.path().join("lineage.svg"),
            set,
        },
        size: OutputSize::of(size[0], size[1]),
        transparent: false,
        poses: vec![Pose {
            time: 1,
            camera: Camera::new(35.0).state(),
        }],
        fps: 0,
        region: Region::none(),
    };
    let task = Task {
        plan,
        scene: scene.snapshot().into_scene(),
        bricks: Arc::new(VolumeBricks {
            volume: volume.clone(),
            pyramid: pyramid.clone(),
        }),
    };
    let mut offline = offline(&gpu);
    let Outcome::Complete { path } = offline.run(task, &AtomicBool::new(false), &mut |_, _| {})
    else {
        panic!("the lineage was not written");
    };
    let markup = std::fs::read_to_string(&path).expect("the lineage reads as text");
    assert!(markup.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
    assert!(markup.contains("width=\"960pt\" height=\"162pt\""));
    assert!(markup.contains("<path d=\"M "));
    assert!(
        !markup.contains("<image ") && !markup.contains("<text"),
        "a lineage page holds paths and outlines, not rasters or text"
    );
    let pdf_plan = Plan {
        destination: Destination::Lineage {
            path: temp.path().join("lineage.pdf"),
            set,
        },
        size: OutputSize::of(size[0], size[1]),
        transparent: false,
        poses: vec![Pose {
            time: 1,
            camera: Camera::new(35.0).state(),
        }],
        fps: 0,
        region: Region::none(),
    };
    let task = Task {
        plan: pdf_plan,
        scene: scene.snapshot().into_scene(),
        bricks: Arc::new(VolumeBricks { volume, pyramid }),
    };
    let Outcome::Complete { path } = offline.run(task, &AtomicBool::new(false), &mut |_, _| {})
    else {
        panic!("the lineage pdf was not written");
    };
    let bytes = std::fs::read(&path).expect("the lineage pdf reads as bytes");
    assert!(bytes.starts_with(b"%PDF-1.7\n"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    assert!(find(&bytes, b"/MediaBox [0 0 960 162]").is_some());
    assert!(
        find(&bytes, b"/Subtype /Image").is_none(),
        "a lineage page stays vector only"
    );
}

fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .rposition(|window| window == needle)
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
    let size = output(width, false);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), aspect(size));
    Plan {
        destination,
        size,
        transparent: false,
        poses: vec![Pose {
            time: 0,
            camera: camera.state(),
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
fn runs_an_export_on_the_worker_thread() {
    let temp = Temp::new("grana-export-worker");
    let path = temp.path().join("image.png");
    let volume = export_volume();
    let pyramid = volume.pyramid();
    let scene = Scene::new(Dataset::synthetic("export", pyramid.clone()));
    let size = output(320, false);
    let mut camera = Camera::new(35.0);
    camera.fit(scene.dataset.world_size(), aspect(size));
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
