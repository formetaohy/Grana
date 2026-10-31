mod common;

use common::{
    SIDE, Session, blue, bright_fraction, channels, dark_fraction, device, green, green_pixels,
    green_sum, lit, pixel, read_pixels, red_count, red_pixels, settle, target,
};
use glam::Vec3;
use grana_object::{Mesh, MeshVertex, Track, TrackNode, TrackSet};
use grana_render::{
    Camera, Crop, Layer, LayerKey, Marker, Renderer, SceneFrame, SectionCamera, Shading, Stop,
    Style, TransferFunction,
};
use grana_volume::{
    Axis, BrickGrid, Channel, Extent, Level, Movie, Pyramid, Spacing, Volume, VoxelData,
};

const BUDGET: u64 = 512 << 20;
const WHOLE: [u32; 4] = [0, 0, SIDE, SIDE];
const CORNER: [u32; 4] = [0, 0, 1, 1];

struct Job<'a> {
    texture: &'a wgpu::Texture,
    volume: &'a Volume,
    volume_region: [u32; 4],
    section_region: [u32; 4],
    focus_z: f32,
    mesh: Option<(&'a Mesh, &'a [Style])>,
}

#[test]
fn streams_two_channels() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = two_channel_volume();
    let pyramid = volume.pyramid();
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
    let requests = settle(
        &mut renderer,
        Session {
            device: &device,
            queue: &queue,
            target: &view,
            camera: &camera,
            section: &section,
            scene: &scene,
            volume_region: WHOLE,
            section_region: CORNER,
            crop: Crop::none(),
            time: 0,
        },
        |request| {
            let grid = BrickGrid::new(volume.extent());
            volume.brick(&grid, request.coord)
        },
    );
    assert!(requests.volume_view.visible > 0, "the view sees no brick");
    assert_eq!(
        requests.volume_view.resident, requests.volume_view.visible,
        "the stream never completed"
    );
    let pixels = read_pixels(&device, &queue, &texture, SIDE);
    assert!(
        lit(&pixels) > (SIDE * SIDE / 20) as usize,
        "the volume barely rendered: {} lit pixels",
        lit(&pixels)
    );
    assert!(blue(&pixels) > 200, "the blue channel barely rendered");
    assert!(green(&pixels) > 200, "the green channel barely rendered");
}

#[test]
fn switches_frames() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let movie = two_frame_movie();
    let pyramid = movie.pyramid().clone();
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(movie.frame(0).world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
    let source = |request: grana_render::BrickRequest| {
        let grid = BrickGrid::new(movie.frame(0).extent());
        movie.brick(request.time, &grid, request.coord)
    };
    settle(
        &mut renderer,
        Session {
            device: &device,
            queue: &queue,
            target: &view,
            camera: &camera,
            section: &section,
            scene: &scene,
            volume_region: WHOLE,
            section_region: CORNER,
            crop: Crop::none(),
            time: 0,
        },
        source,
    );
    let first = read_pixels(&device, &queue, &texture, SIDE);
    assert!(
        blue(&first) > 200,
        "the first frame barely rendered: {} blue pixels",
        blue(&first)
    );
    assert!(
        green(&first) < 60,
        "the first frame leaked the second frame: {} green pixels",
        green(&first)
    );
    settle(
        &mut renderer,
        Session {
            device: &device,
            queue: &queue,
            target: &view,
            camera: &camera,
            section: &section,
            scene: &scene,
            volume_region: WHOLE,
            section_region: CORNER,
            crop: Crop::none(),
            time: 1,
        },
        source,
    );
    let second = read_pixels(&device, &queue, &texture, SIDE);
    assert!(
        green(&second) > 200,
        "the second frame barely rendered: {} green pixels",
        green(&second)
    );
    assert!(
        blue(&second) < 60,
        "the second frame kept the first frame: {} blue pixels",
        blue(&second)
    );
}

#[test]
fn sections_planes_and_layers() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let layer = flat_volume(|_, _, z| if z == 8 { 60000 } else { 0 });
    let plane = section_render(&device, &queue, &texture, &layer, Axis::Z, [0.0, 0.0, 0.0]);
    assert!(bright_fraction(&plane, 0.38, 0.62, 0.38, 0.62) > 0.98);
    let layer = flat_volume(|_, _, z| if z == 8 { 60000 } else { 0 });
    let neighbour = section_render(&device, &queue, &texture, &layer, Axis::Z, [0.0, 0.0, 0.5]);
    assert!(dark_fraction(&neighbour, 0.0, 1.0, 0.0, 1.0) > 0.98);
    let half = flat_volume(|_, y, _| if y >= 32 { 60000 } else { 0 });
    let plane = section_render(&device, &queue, &texture, &half, Axis::X, [0.0, 0.0, 0.0]);
    assert!(dark_fraction(&plane, 0.0, 0.33, 0.0, 1.0) > 0.98);
    assert!(bright_fraction(&plane, 0.67, 1.0, 0.2, 0.8) > 0.30);
}

#[test]
fn clips_to_a_crop_box() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = sphere(Extent::new(128, 128, 64), Spacing::new(0.1, 0.1, 0.2));
    let pyramid = volume.pyramid();
    let world = Vec3::from(volume.world_extent());
    let half = 0.5 * world;
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    let section = SectionCamera::new(Axis::Z);
    let transfer = TransferFunction::new(vec![
        Stop::new(0.0, [0.0, 0.0, 0.0], 0.0),
        Stop::new(0.49, [0.0, 0.0, 0.0], 0.0),
        Stop::new(0.51, [1.0, 1.0, 1.0], 1.0),
        Stop::new(1.0, [1.0, 1.0, 1.0], 1.0),
    ]);
    let channels = vec![grana_render::ChannelRow {
        visible: true,
        opacity: 1.0,
        window: pyramid.channels()[0].window(),
        transfer: &transfer,
    }];
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
    let source = |request: grana_render::BrickRequest| {
        let grid = BrickGrid::new(volume.extent());
        volume.brick(&grid, request.coord)
    };
    let session = |crop: Crop| Session {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
        crop,
        time: 0,
    };

    settle(&mut renderer, session(Crop::none()), source);
    let full = bright_pixels(&read_pixels(&device, &queue, &texture, SIDE));

    let left = Crop::new(
        Vec3::new(-half.x, -half.y, -half.z),
        Vec3::new(0.0, half.y, half.z),
    );
    settle(&mut renderer, session(left), source);
    let cropped = bright_pixels(&read_pixels(&device, &queue, &texture, SIDE));

    let corner = Crop::new(Vec3::splat(0.75 * half.x), Vec3::splat(half.x));
    settle(&mut renderer, session(corner), source);
    let erased = bright_pixels(&read_pixels(&device, &queue, &texture, SIDE));

    assert!(full > 0, "the uncropped volume is empty");
    assert!(
        cropped > 0 && cropped < full,
        "the crop does not cut the volume: {cropped} of {full}"
    );
    assert_eq!(
        erased, 0,
        "a crop away from the volume still renders bricks"
    );

    let plane_out = Crop::new(
        Vec3::new(-half.x, -half.y, 0.5 * half.z),
        Vec3::new(half.x, half.y, half.z),
    );
    settle(
        &mut renderer,
        Session {
            volume_region: CORNER,
            section_region: WHOLE,
            crop: plane_out,
            ..session(Crop::none())
        },
        source,
    );
    let section = read_pixels(&device, &queue, &texture, SIDE);
    assert!(
        dark_fraction(&section, 0.0, 1.0, 0.0, 1.0) > 0.9,
        "the section ignores the crop"
    );
}

fn bright_pixels(pixels: &[[u8; 4]]) -> usize {
    pixels
        .iter()
        .filter(|pixel| {
            pixel
                .iter()
                .take(3)
                .map(|value| u32::from(*value))
                .sum::<u32>()
                > 400
        })
        .count()
}

#[test]
fn composites_objects_into_the_volume() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let empty = volume(0);
    let dense = volume(60_000);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(empty.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let markers = [Marker::new([0.0, 0.0, 0.0], 0.4, [1.0, 0.1, 0.1], 1.0)];

    let visible = marker_scene(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &empty,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: None,
        },
        &camera,
        &section,
        &markers,
    );
    assert!(red_count(&visible, WHOLE) > 200, "the object is not drawn");

    let hidden = marker_scene(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &dense,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: None,
        },
        &camera,
        &section,
        &markers,
    );
    assert!(
        red_count(&hidden, WHOLE) < 20,
        "the volume does not occlude the object"
    );

    let sliced = marker_scene(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &empty,
            volume_region: CORNER,
            section_region: WHOLE,
            focus_z: 0.0,
            mesh: None,
        },
        &camera,
        &section,
        &markers,
    );
    assert!(
        red_count(&sliced, WHOLE) > 200,
        "the section does not mark the object"
    );

    let mut moved = camera.clone();
    moved.set_focus_along(Axis::Z, 2.0 * section.span());
    let missed = marker_scene(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &empty,
            volume_region: CORNER,
            section_region: WHOLE,
            focus_z: 0.0,
            mesh: None,
        },
        &moved,
        &section,
        &markers,
    );
    assert_eq!(
        red_count(&missed, WHOLE),
        0,
        "the section marks an object it does not intersect"
    );

    let background = pixel(&visible, 4, SIDE - 4);
    assert!(
        background
            .iter()
            .take(3)
            .map(|value| u32::from(*value))
            .sum::<u32>()
            < 120,
        "the background must stay dark: {background:?}"
    );
}

#[test]
fn renders_surface_objects_and_sections() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mesh = cubes();
    let visible = [
        Style::new([1.0, 0.12, 0.12], true, false),
        Style::new([0.12, 1.0, 0.12], true, false),
    ];
    let selected = [
        Style::new([1.0, 0.12, 0.12], true, true),
        Style::new([0.12, 1.0, 0.12], true, false),
    ];
    let hidden = [
        Style::new([1.0, 0.12, 0.12], false, false),
        Style::new([0.12, 1.0, 0.12], false, false),
    ];

    let pixels = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&mesh, &visible)),
        },
    );
    let red = red_pixels(&pixels);
    let green = green_pixels(&pixels);
    assert!(red > 1500, "the red object is not drawn: {red}");
    assert!(green > 1500, "the green object is not drawn: {green}");
    let background = pixel(&pixels, 4, SIDE - 4);
    assert!(
        background
            .iter()
            .take(3)
            .map(|value| u32::from(*value))
            .sum::<u32>()
            < 120,
        "the background must stay dark: {background:?}"
    );

    let changed = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&mesh, &selected)),
        },
    );
    assert!(
        green_sum(&changed) > green_sum(&pixels) + u64::from(SIDE * SIDE),
        "selection does not highlight the object"
    );

    let cleared = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&mesh, &hidden)),
        },
    );
    assert_eq!(
        red_pixels(&cleared) + green_pixels(&cleared),
        0,
        "a hidden object is still drawn"
    );

    let section = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: CORNER,
            section_region: WHOLE,
            focus_z: 0.0,
            mesh: Some((&mesh, &visible)),
        },
    );
    let inside = red_pixels(&section);
    assert!(
        inside > 1500,
        "the section does not fill the object: {inside}"
    );

    let missed = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: CORNER,
            section_region: WHOLE,
            focus_z: 1.2,
            mesh: Some((&mesh, &visible)),
        },
    );
    assert_eq!(
        red_pixels(&missed),
        0,
        "the section fills an object the plane misses"
    );
}

#[test]
fn renders_track_tubes_and_sections() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mesh = tracks().mesh(0.15);
    let visible = [
        Style::new([1.0, 0.12, 0.12], true, false),
        Style::new([0.12, 1.0, 0.12], true, false),
    ];
    let hidden = [
        Style::new([1.0, 0.12, 0.12], false, false),
        Style::new([0.12, 1.0, 0.12], false, false),
    ];

    let pixels = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&mesh, &visible)),
        },
    );
    let red = red_pixels(&pixels);
    let green = green_pixels(&pixels);
    assert!(red > 100, "the first track is not drawn: {red}");
    assert!(green > 100, "the second track is not drawn: {green}");

    let cleared = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&mesh, &hidden)),
        },
    );
    assert_eq!(
        red_pixels(&cleared) + green_pixels(&cleared),
        0,
        "a hidden track is still drawn"
    );

    let section = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: CORNER,
            section_region: WHOLE,
            focus_z: 0.0,
            mesh: Some((&mesh, &visible)),
        },
    );
    assert!(
        red_pixels(&section) + green_pixels(&section) > 20,
        "the tracks do not cross the section"
    );
}

#[test]
fn switches_levels() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let levels = [
        (Extent::new(256, 256, 64), Spacing::new(0.1, 0.1, 0.2)),
        (Extent::new(128, 128, 32), Spacing::new(0.2, 0.2, 0.4)),
        (Extent::new(64, 64, 16), Spacing::new(0.4, 0.4, 0.8)),
    ];
    let volumes = levels
        .iter()
        .map(|(extent, spacing)| sphere(*extent, *spacing))
        .collect::<Vec<_>>();
    let pyramid = Pyramid::new(
        levels
            .iter()
            .map(|(extent, spacing)| Level::new(*extent, *spacing))
            .collect(),
        volumes[0].pyramid().channels().to_vec(),
        1,
        1.0,
    );
    let grids = levels
        .iter()
        .map(|(extent, _)| BrickGrid::new(*extent))
        .collect::<Vec<_>>();
    let transfers = vec![TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volumes[0].world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = Renderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        100 * 78_608,
    );
    renderer.set_dataset(&device, &pyramid);
    let source = |request: grana_render::BrickRequest| {
        let volume = &volumes[request.level];
        volume.brick(&grids[request.level], request.coord)
    };
    let fitted = settle(
        &mut renderer,
        Session {
            device: &device,
            queue: &queue,
            target: &view,
            camera: &camera,
            section: &section,
            scene: &scene,
            volume_region: WHOLE,
            section_region: CORNER,
            crop: Crop::none(),
            time: 0,
        },
        source,
    )
    .volume_view;
    assert!(
        fitted.level > 0,
        "the fitted view must coarsen to fit the cache"
    );
    for _ in 0..10 {
        camera.zoom(1.0);
    }
    let close = settle(
        &mut renderer,
        Session {
            device: &device,
            queue: &queue,
            target: &view,
            camera: &camera,
            section: &section,
            scene: &scene,
            volume_region: WHOLE,
            section_region: CORNER,
            crop: Crop::none(),
            time: 0,
        },
        source,
    )
    .volume_view;
    assert_eq!(close.level, 0, "zooming in must resolve the finest level");
    for _ in 0..10 {
        camera.zoom(-1.0);
    }
    let far = settle(
        &mut renderer,
        Session {
            device: &device,
            queue: &queue,
            target: &view,
            camera: &camera,
            section: &section,
            scene: &scene,
            volume_region: WHOLE,
            section_region: CORNER,
            crop: Crop::none(),
            time: 0,
        },
        source,
    )
    .volume_view;
    assert!(far.level > 0, "zooming out must coarsen again");
}

fn section_render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    volume: &Volume,
    axis: Axis,
    focus: [f32; 3],
) -> Vec<[u8; 4]> {
    let view = texture.create_view(&Default::default());
    let pyramid = volume.pyramid();
    let world = Vec3::from(volume.world_extent());
    let voxel = Vec3::new(
        volume.spacing().x(),
        volume.spacing().y(),
        volume.spacing().z(),
    );
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    camera.set_focus_along(Axis::X, focus[0]);
    camera.set_focus_along(Axis::Y, focus[1]);
    camera.set_focus_along(Axis::Z, focus[2]);
    let mut section = SectionCamera::new(axis);
    section.fit(world, voxel, 1.0);
    let transfer = TransferFunction::new(vec![
        Stop::new(0.0, [0.0, 0.0, 0.0], 0.0),
        Stop::new(1.0, [1.0, 1.0, 1.0], 1.0),
    ]);
    let channels = vec![grana_render::ChannelRow {
        visible: true,
        opacity: 1.0,
        window: pyramid.channels()[0].window(),
        transfer: &transfer,
    }];
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(device, &pyramid);
    settle(
        &mut renderer,
        Session {
            device,
            queue,
            target: &view,
            camera: &camera,
            section: &section,
            scene: &scene,
            volume_region: CORNER,
            section_region: WHOLE,
            crop: Crop::none(),
            time: 0,
        },
        |request| {
            let grid = BrickGrid::new(volume.extent());
            volume.brick(&grid, request.coord)
        },
    );
    read_pixels(device, queue, texture, SIDE)
}

struct Setup<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    job: Job<'a>,
    camera: Option<&'a Camera>,
    section: Option<&'a SectionCamera>,
    markers: &'a [Marker],
}

fn scene_render(device: &wgpu::Device, queue: &wgpu::Queue, job: Job<'_>) -> Vec<[u8; 4]> {
    render(Setup {
        device,
        queue,
        job,
        camera: None,
        section: None,
        markers: &[],
    })
}

fn marker_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    job: Job<'_>,
    camera: &Camera,
    section: &SectionCamera,
    markers: &[Marker],
) -> Vec<[u8; 4]> {
    render(Setup {
        device,
        queue,
        job,
        camera: Some(camera),
        section: Some(section),
        markers,
    })
}

fn render(setup: Setup<'_>) -> Vec<[u8; 4]> {
    let Setup {
        device,
        queue,
        job,
        camera,
        section,
        markers,
    } = setup;
    let volume = job.volume;
    let view = job.texture.create_view(&Default::default());
    let pyramid = volume.pyramid();
    let world = Vec3::from(volume.world_extent());
    let voxel = Vec3::new(
        volume.spacing().x(),
        volume.spacing().y(),
        volume.spacing().z(),
    );
    let camera = match camera {
        Some(camera) => camera.clone(),
        None => {
            let mut camera = Camera::new(35.0);
            camera.fit(world, 1.0);
            camera.set_focus_along(Axis::Z, job.focus_z);
            camera
        }
    };
    let section = match section {
        Some(section) => section.clone(),
        None => {
            let mut section = SectionCamera::new(Axis::Z);
            section.fit(world, voxel, 1.0);
            section
        }
    };
    let transfer = TransferFunction::new(vec![
        Stop::new(0.0, [0.0, 0.0, 0.0], 0.0),
        Stop::new(1.0, [1.0, 1.0, 1.0], 1.0),
    ]);
    let channels = vec![grana_render::ChannelRow {
        visible: true,
        opacity: 1.0,
        window: pyramid.channels()[0].window(),
        transfer: &transfer,
    }];
    let layers = job
        .mesh
        .map(|(mesh, styles)| Layer {
            key: LayerKey::at_frame(1, 0, 0, 1),
            mesh: Some(mesh),
            style_start: 0,
            style_count: styles.len(),
        })
        .into_iter()
        .collect::<Vec<_>>();
    let styles = job
        .mesh
        .map_or_else(Vec::new, |(_, styles)| styles.to_vec());
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        markers,
    };
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(device, &pyramid);
    settle(
        &mut renderer,
        Session {
            device,
            queue,
            target: &view,
            camera: &camera,
            section: &section,
            scene: &scene,
            volume_region: job.volume_region,
            section_region: job.section_region,
            crop: Crop::none(),
            time: 0,
        },
        |request| {
            let grid = BrickGrid::new(volume.extent());
            volume.brick(&grid, request.coord)
        },
    );
    read_pixels(device, queue, job.texture, SIDE)
}

fn two_channel_volume() -> Volume {
    let extent = Extent::new(128, 128, 96);
    let count = extent.voxel_count() as usize;
    let mut blue = vec![0u16; count];
    let mut green = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let index = extent.index(x, y, z) as usize;
                blue[index] = density(x, y, z, 40.0);
                green[index] = density(x, y, z, 88.0);
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.13),
        vec![
            Channel::new("nuclei", [0.2, 0.4, 1.0], VoxelData::U16(blue)),
            Channel::new("membrane", [0.2, 1.0, 0.3], VoxelData::U16(green)),
        ],
    )
}

fn density(x: u32, y: u32, z: u32, centre_x: f32) -> u16 {
    let radius =
        ((x as f32 - centre_x).powi(2) + (y as f32 - 64.0).powi(2) + (z as f32 - 48.0).powi(2))
            .sqrt();
    let density = (1.0 - radius / 30.0).max(0.0);
    (density * density * 60000.0) as u16
}

fn two_frame_movie() -> Movie {
    let extent = Extent::new(96, 96, 64);
    let spacing = Spacing::new(0.1, 0.1, 0.13);
    let count = extent.voxel_count() as usize;
    let zeros = vec![0u16; count];
    let mut blob = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let radius = ((x as f32 - 48.0).powi(2)
                    + (y as f32 - 48.0).powi(2)
                    + (z as f32 - 32.0).powi(2))
                .sqrt();
                let density = (1.0 - radius / 22.0).max(0.0);
                blob[extent.index(x, y, z) as usize] = (density * density * 60000.0) as u16;
            }
        }
    }
    let frame = |blue: Vec<u16>, green: Vec<u16>| {
        Volume::new(
            extent,
            spacing,
            vec![
                Channel::new("nuclei", [0.2, 0.4, 1.0], VoxelData::U16(blue)),
                Channel::new("membrane", [0.2, 1.0, 0.3], VoxelData::U16(green)),
            ],
        )
    };
    Movie::new(
        vec![frame(blob.clone(), zeros.clone()), frame(zeros, blob)],
        1.0,
    )
}

fn flat_volume(value: impl Fn(u32, u32, u32) -> u16) -> Volume {
    let extent = Extent::new(64, 64, 16);
    let mut voxels = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                voxels[extent.index(x, y, z) as usize] = value(x, y, z);
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.5),
        vec![Channel::new(
            "section",
            [0.4, 0.7, 1.0],
            VoxelData::U16(voxels),
        )],
    )
}

fn volume(value: u16) -> Volume {
    let extent = Extent::new(64, 64, 16);
    let mut voxels = vec![value; extent.voxel_count() as usize];
    voxels[0] = 0;
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.5),
        vec![Channel::new(
            "density",
            [0.3, 0.6, 1.0],
            VoxelData::U16(voxels),
        )],
    )
}

fn empty_volume() -> Volume {
    let extent = Extent::new(32, 32, 32);
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.1),
        vec![Channel::new(
            "empty",
            [0.4, 0.6, 1.0],
            VoxelData::U16(vec![0; extent.voxel_count() as usize]),
        )],
    )
}

fn sphere(extent: Extent, spacing: Spacing) -> Volume {
    let mut data = vec![0u16; extent.voxel_count() as usize];
    let centre = [
        extent.x() as f32 * 0.5,
        extent.y() as f32 * 0.5,
        extent.z() as f32 * 0.5,
    ];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let dx = (x as f32 - centre[0]) / (extent.x() as f32 * 0.3);
                let dy = (y as f32 - centre[1]) / (extent.y() as f32 * 0.3);
                let dz = (z as f32 - centre[2]) / (extent.z() as f32 * 0.3);
                let radius = (dx * dx + dy * dy + dz * dz).sqrt();
                let value = (1.0 - radius).max(0.0);
                data[extent.index(x, y, z) as usize] = (value * 60000.0) as u16;
            }
        }
    }
    Volume::new(
        extent,
        spacing,
        vec![Channel::new(
            "sphere",
            [0.3, 0.6, 1.0],
            VoxelData::U16(data),
        )],
    )
}

fn cubes() -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, centre) in [-0.7f32, 0.7].into_iter().enumerate() {
        let base = vertices.len() as u32;
        let corners = [
            [-1.0f32, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ];
        for corner in corners {
            let position =
                Vec3::new(corner[0], corner[1], corner[2]) * 0.6 + Vec3::new(centre, 0.0, 0.0);
            let normal = (position - Vec3::new(centre, 0.0, 0.0)).normalize();
            vertices.push(MeshVertex::new(
                position.to_array(),
                normal.to_array(),
                object as u32,
            ));
        }
        for face in [
            [0u32, 1, 2],
            [0, 2, 3],
            [4, 6, 5],
            [4, 7, 6],
            [0, 4, 5],
            [0, 5, 1],
            [1, 5, 6],
            [1, 6, 2],
            [2, 6, 7],
            [2, 7, 3],
            [3, 7, 4],
            [3, 4, 0],
        ] {
            indices.extend(face.map(|corner| base + corner));
        }
    }
    Mesh::new(vertices, indices)
}

fn tracks() -> TrackSet {
    let first = Track::new(
        None,
        vec![
            TrackNode::new(0, 0, [-0.7, -0.1, 0.0]),
            TrackNode::new(1, 0, [-0.5, -0.1, 0.0]),
            TrackNode::new(2, 0, [-0.3, -0.1, 0.0]),
        ],
    );
    let second = Track::new(
        None,
        vec![
            TrackNode::new(0, 1, [0.3, 0.1, 0.0]),
            TrackNode::new(1, 1, [0.5, 0.1, 0.0]),
            TrackNode::new(2, 1, [0.7, 0.1, 0.0]),
        ],
    );
    TrackSet::new(1.0, vec![first, second])
}
