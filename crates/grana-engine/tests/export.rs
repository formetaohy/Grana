use std::path::PathBuf;

use glam::Vec2;
use grana_display::{Camera, SectionCamera};
use grana_engine::{Destination, Engine, Outcome, Panels, Plan, Pose};
use grana_scene::{Dataset, Scene};
use grana_ui::Rect;
use grana_volume::{Axis, BrickGrid, Channel, Extent, Spacing, Volume, VoxelData};
use wgpu::TextureFormat;

const FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;
const WIDTH: u32 = 640;

fn section_void(size: [u32; 2]) -> (u32, u32) {
    let origin = size[0] * 130 / 240;
    let x = origin + (size[0] - origin) * 18 / 100;
    let y = size[1] * 11 / 100;
    (x, y)
}

fn capsule_fill(size: [u32; 2]) -> (u32, u32) {
    (size[0] * 5 / 16, size[1] * 13 / 16)
}

fn ink(image: &Decoded, size: [u32; 2]) -> (u32, u32, u32, u32, usize) {
    let mut left = u32::MAX;
    let mut top = u32::MAX;
    let mut right = 0;
    let mut bottom = 0;
    let mut count = 0;
    for (index, pixel) in image.pixels.as_chunks::<4>().0.iter().enumerate() {
        let x = index as u32 % size[0];
        let y = index as u32 / size[0];
        if x >= size[0] / 2 || y < size[1] * 2 / 3 || pixel[3] == 0 {
            continue;
        }
        if pixel[..3]
            .iter()
            .map(|value| u32::from(*value))
            .sum::<u32>()
            <= 400
        {
            continue;
        }
        left = left.min(x);
        top = top.min(y);
        right = right.max(x);
        bottom = bottom.max(y);
        count += 1;
    }
    (left, top, right, bottom, count)
}

#[test]
fn writes_a_transparent_viewport() {
    let Some((device, queue)) = device() else {
        return;
    };
    let volume = sphere();
    let mut scene = Scene::new(Dataset::synthetic("cells", volume.pyramid()));
    let Some((camera, section)) = view(&volume) else {
        return;
    };
    let root = scratch("transparent");
    let opaque_path = root.join("opaque.png");
    let transparent_path = root.join("transparent.png");
    let mut engine = Engine::new(&device, &queue, FORMAT);
    engine.set_dataset(&device, scene.dataset.pyramid());
    let mut ui = grana_ui::Ui::new(&device, &queue, FORMAT);
    ui.resize([WIDTH, 240]);
    let poses = vec![Pose {
        time: 0,
        camera,
        section,
    }];
    let opaque = run(
        &mut engine,
        &mut ui,
        &volume,
        &mut scene,
        &device,
        &queue,
        Plan {
            destination: Destination::Image {
                path: opaque_path.clone(),
            },
            panels: panels(),
            width: WIDTH,
            transparent: false,
            poses: poses.clone(),
            fps: 0,
        },
    );
    assert!(matches!(opaque, Outcome::Complete { .. }));
    run(
        &mut engine,
        &mut ui,
        &volume,
        &mut scene,
        &device,
        &queue,
        Plan {
            destination: Destination::Image {
                path: transparent_path.clone(),
            },
            panels: panels(),
            width: WIDTH,
            transparent: true,
            poses,
            fps: 0,
        },
    );
    let opaque = decode(&opaque_path);
    let transparent = decode(&transparent_path);
    let size = transparent.size;
    let corner = pixel(&transparent, 3, 3);
    assert_eq!(corner[3], 0, "the transparent background stays empty");
    assert_eq!(
        [corner[0], corner[1], corner[2]],
        [0, 0, 0],
        "an empty background carries no colour"
    );
    let opaque_corner = pixel(&opaque, 3, 3);
    assert_eq!(opaque_corner[3], 255, "an opaque image stays opaque");
    let (void_x, void_y) = section_void(size);
    let section = pixel(&transparent, void_x, void_y);
    assert_eq!(
        section[3], 0,
        "the empty part of the section pane stays transparent"
    );
    let (capsule_x, capsule_y) = capsule_fill(size);
    assert_eq!(
        pixel(&transparent, capsule_x, capsule_y)[3],
        0,
        "a transparent figure carries no backdrop fill"
    );
    let opaque_ink = ink(&opaque, size);
    let transparent_ink = ink(&transparent, size);
    assert_eq!(
        (opaque_ink.0, opaque_ink.1, opaque_ink.2, opaque_ink.3),
        (
            transparent_ink.0,
            transparent_ink.1,
            transparent_ink.2,
            transparent_ink.3
        ),
        "the transparent figure keeps the opaque figure ink in place"
    );
    let dense = opaque_ink.4.max(transparent_ink.4);
    let sparse = opaque_ink.4.min(transparent_ink.4);
    assert!(
        dense - sparse <= dense / 8,
        "the transparent figure draws the ink of the opaque figure: {opaque_ink:?} against {transparent_ink:?}"
    );
    let centre = pixel(&transparent, size[0] / 4, size[1] / 2);
    assert!(centre[3] > 0, "the volume is transparent nowhere");
    let opaque_centre = pixel(&opaque, size[0] / 4, size[1] / 2);
    for channel in 0..3 {
        let difference = i32::from(centre[channel]) - i32::from(opaque_centre[channel]);
        assert!(
            difference.abs() <= 2,
            "the straight volume edge disagrees with the opaque render: {difference}"
        );
    }
    let mut edge = 0;
    for (index, pixel) in transparent.pixels.as_chunks::<4>().0.iter().enumerate() {
        let x = index as u32 % size[0];
        let y = index as u32 / size[0];
        if pixel[3] > 0 && pixel[3] < 64 {
            edge += 1;
            let white = pixel[..3].iter().all(|value| *value == 255);
            assert!(
                white || pixel[..3].iter().any(|value| *value < 250),
                "a semi transparent edge pixel is blown out at {x},{y}: {pixel:?}"
            );
        }
    }
    assert!(edge > 0, "the volume carries no semi transparent edge");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writes_a_frame_sequence() {
    let Some((device, queue)) = device() else {
        return;
    };
    let volume = sphere();
    let mut scene = Scene::new(Dataset::synthetic("cells", volume.pyramid()));
    let Some((camera, section)) = view(&volume) else {
        return;
    };
    let root = scratch("sequence");
    let directory = root.join("cells-sequence");
    let mut engine = Engine::new(&device, &queue, FORMAT);
    engine.set_dataset(&device, scene.dataset.pyramid());
    let mut ui = grana_ui::Ui::new(&device, &queue, FORMAT);
    let poses = (0..3)
        .map(|_| Pose {
            time: 0,
            camera,
            section,
        })
        .collect::<Vec<_>>();
    assert_eq!(poses.len(), 3);
    let outcome = run(
        &mut engine,
        &mut ui,
        &volume,
        &mut scene,
        &device,
        &queue,
        Plan {
            destination: Destination::Sequence {
                directory: directory.clone(),
                stem: "cells".to_string(),
            },
            panels: panels(),
            width: WIDTH,
            transparent: false,
            poses,
            fps: 0,
        },
    );
    assert!(matches!(outcome, Outcome::Complete { .. }));
    let mut names = std::fs::read_dir(&directory)
        .expect("the sequence folder reads")
        .map(|entry| {
            entry
                .expect("the entry reads")
                .file_name()
                .into_string()
                .expect("the name is text")
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        [
            "cells-0000-t0000.png",
            "cells-0001-t0000.png",
            "cells-0002-t0000.png"
        ]
    );
    std::fs::remove_dir_all(&root).ok();
}

fn run(
    engine: &mut Engine,
    ui: &mut grana_ui::Ui,
    volume: &Volume,
    scene: &mut Scene,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    plan: Plan,
) -> Outcome {
    engine
        .start_export(device, plan)
        .expect("the export starts");
    let grid = BrickGrid::new(volume.extent());
    for _ in 0..4096 {
        let pose = engine.pose().expect("the export holds a pose");
        scene.set_time(pose.time);
        let outcome = engine.render_export(device, queue, ui, scene);
        if !matches!(outcome, Outcome::Running) {
            return outcome;
        }
        for _ in 0..8 {
            let Some(requests) = engine.poll_requests(device) else {
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("the graphics device responds");
                continue;
            };
            for request in requests.volume.iter().chain(&requests.section) {
                let brick = volume.brick(&grid, request.coord);
                engine.upload(device, queue, *request, &brick);
            }
        }
    }
    panic!("the export never settled");
}

fn view(volume: &Volume) -> Option<(grana_display::CameraState, grana_display::SectionState)> {
    let world = glam::Vec3::from(volume.world_extent());
    let spacing = volume.spacing();
    let voxel = glam::Vec3::new(spacing.x(), spacing.y(), spacing.z());
    let mut camera = Camera::new(35.0);
    camera.fit(world, 120.0 / 90.0);
    let mut section = SectionCamera::new(Axis::Z);
    section.fit(world, voxel, 110.0 / 90.0);
    Some((camera.state(), section.state()))
}

fn panels() -> Panels {
    Panels::new(
        Rect::from_min_max(Vec2::new(0.0, 0.0), Vec2::new(120.0, 90.0)),
        Rect::from_min_max(Vec2::new(130.0, 0.0), Vec2::new(240.0, 90.0)),
    )
}

fn sphere() -> Volume {
    let extent = Extent::new(64, 64, 64);
    let centre = 32.0;
    let mut voxels = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let radius = ((x as f32 - centre).powi(2)
                    + (y as f32 - centre).powi(2)
                    + (z as f32 - centre).powi(2))
                .sqrt();
                let density = (1.0 - radius / 18.0).max(0.0);
                voxels[extent.index(x, y, z) as usize] = (density * density * 60000.0) as u16;
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.1),
        vec![Channel::new(
            "signal",
            [0.3, 0.6, 1.0],
            VoxelData::U16(voxels),
        )],
    )
}

struct Decoded {
    size: [u32; 2],
    pixels: Vec<u8>,
}

fn decode(path: &std::path::Path) -> Decoded {
    let raster = grana_raster::read(path).expect("the image reads");
    Decoded {
        size: raster.size(),
        pixels: raster.pixels().to_vec(),
    }
}

fn pixel(image: &Decoded, x: u32, y: u32) -> [u8; 4] {
    let index = (y * image.size[0] + x) as usize * 4;
    image.pixels[index..index + 4]
        .try_into()
        .expect("a pixel holds four channels")
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("grana-export-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder is created");
    path
}

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    match grana_gpu::Headless::new() {
        Ok(headless) => Some((headless.device, headless.queue)),
        Err(detail) => {
            println!("{detail}, skipping");
            None
        }
    }
}
