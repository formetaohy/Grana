mod common;

use common::{
    SAMPLES, SIDE, Session, blue, bright_fraction, channels, dark_fraction, device, green,
    green_pixels, green_sum, lit, pixel, read_pixels, red_count, red_pixels, settle,
    settle_clipped, target,
};
use std::sync::Arc;

use glam::Vec3;
use grana_display::{
    Camera, CameraState, SectionCamera, Shading, Stop, TransferFunction, linear_to_srgb,
    srgb_to_linear,
};
use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_object::{Track, TrackNode, TrackSet};
use grana_render::{
    CutInfo, Frame, Layer, LayerKey, Marker, PickId, PickPane, Renderer, SceneFrame, Style,
};
use grana_volume::{
    Axis, BrickGrid, Channel, Clip, Extent, Level, PADDED_SIDE, Pyramid, Region, Spacing, Volume,
    VolumeSeries, VoxelData,
};

const BUDGET: u64 = 512 << 20;
const SLOT_BYTES: u64 = (PADDED_SIDE as u64).pow(3) * size_of::<u16>() as u64;
const WHOLE: [u32; 4] = [0, 0, SIDE, SIDE];
const CORNER: [u32; 4] = [0, 0, 1, 1];
const SECTION_AREA: [u32; 4] = [1, 1, SIDE - 1, SIDE - 1];

#[derive(Clone, Copy)]
struct Job<'a> {
    texture: &'a wgpu::Texture,
    volume: &'a Volume,
    volume_region: [u32; 4],
    section_region: [u32; 4],
    focus_z: f32,
    mesh: Option<(&'a PackedMesh, &'a [Style])>,
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
            region: Region::none(),
            time: 0,
        },
        |request| {
            let grid = BrickGrid::new(volume.extent());
            volume.brick(&grid, request.coord)
        },
    );
    assert!(requests.volume_view.visible > 0, "the view sees no brick");
    assert_eq!(
        requests.volume_view.resident, requests.volume_view.demand,
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
fn samples_a_channel_beyond_the_first_pack() {
    let Some(pixels) = packed_render(packed_volume(5, 4)) else {
        return;
    };
    assert!(
        red_pixels(&pixels) > 200,
        "the fifth channel lives in the second pack"
    );
}

#[test]
fn samples_a_pack_with_three_channels() {
    let Some(pixels) = packed_render(packed_volume(3, 2)) else {
        return;
    };
    assert!(
        red_pixels(&pixels) > 200,
        "the third channel pads its pack to four texels"
    );
}

#[test]
fn bounds_the_demand_to_the_cache() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = sphere(Extent::new(256, 256, 128), Spacing::new(0.1, 0.1, 0.2));
    let pyramid = volume.pyramid();
    let transfers = vec![TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
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
        100 * SLOT_BYTES,
    );
    renderer.set_dataset(&device, &pyramid);
    let session = Session {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
        region: Region::none(),
        time: 0,
    };
    renderer.draw(&device, &queue, common::frame(&session));
    let first =
        common::next_demand(&mut renderer, &session).expect("the stream produced no demand");
    assert!(
        first.volume_view.visible > first.volume_view.demand,
        "the cache must bound the demand: {:?}",
        first.volume_view
    );
    assert!(
        !first.volume.is_empty(),
        "the bounded cache requested nothing"
    );
    let grid = BrickGrid::new(volume.extent());
    let half = 0.5 * Vec3::from(volume.world_extent());
    let dims = Vec3::new(
        volume.extent().x() as f32,
        volume.extent().y() as f32,
        volume.extent().z() as f32,
    );
    let eye = camera.eye();
    let mut far = 0.0f32;
    for index in 0..8u32 {
        let corner = Vec3::new(
            if index & 1 == 0 { -half.x } else { half.x },
            if index & 2 == 0 { -half.y } else { half.y },
            if index & 4 == 0 { -half.z } else { half.z },
        );
        far = far.max((corner - eye).length());
    }
    let bucket = |request: &grana_volume::BrickRequest| {
        let origin = grid.origin(request.coord);
        let centre = Vec3::new(
            origin[0] as f32 + 16.0,
            origin[1] as f32 + 16.0,
            origin[2] as f32 + 16.0,
        );
        let position = -half + centre / dims * (2.0 * half);
        let depth = (position - eye).length() / far * grana_render::DEPTH_BUCKETS as f32;
        (depth as u32).min(grana_render::DEPTH_BUCKETS as u32 - 1)
    };
    let mut previous = 0;
    for request in &first.volume {
        let bucket = bucket(request);
        assert!(
            bucket >= previous,
            "the demand is not ordered by view depth"
        );
        previous = bucket;
    }
    let settled = settle(&mut renderer, session, |request| {
        volume.brick(&grid, request.coord)
    });
    assert_eq!(
        settled.volume_view.resident, settled.volume_view.demand,
        "the bounded demand never settled"
    );
    assert!(
        settled.volume_view.demand > 0,
        "the cache admitted no brick"
    );
}

#[test]
fn prefetches_the_next_frame() {
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
    for _ in 0..16 {
        camera.zoom(1.0);
    }
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
    let grid = BrickGrid::new(movie.frame(0).extent());
    let source =
        |request: grana_volume::BrickRequest| movie.brick(request.time, &grid, request.coord);
    let session = Session {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
        region: Region::none(),
        time: 0,
    };
    let mut prefetched = 0;
    for _ in 0..64 {
        renderer.draw(&device, &queue, common::prefetch_frame(&session));
        if !renderer.requests_pending() {
            break;
        }
        let Some(requests) = common::next_demand(&mut renderer, &session) else {
            continue;
        };
        prefetched += requests.prefetch.len();
        for request in requests
            .volume
            .iter()
            .chain(&requests.section)
            .chain(&requests.prefetch)
        {
            let brick = source(*request);
            let _ = renderer.upload(&device, &queue, *request, &brick);
        }
    }
    assert!(prefetched > 0, "playback issued no prefetch");
    let next = Session { time: 1, ..session };
    renderer.draw(&device, &queue, common::frame(&next));
    let first =
        common::next_demand(&mut renderer, &next).expect("the next frame produced no demand");
    assert!(
        first.volume.is_empty() && first.section.is_empty(),
        "the next frame was not prefetched"
    );
    assert!(
        first.volume_view.demand > 0,
        "the next frame demands no brick"
    );
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
    let source = |request: grana_volume::BrickRequest| {
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
            region: Region::none(),
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
            region: Region::none(),
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
fn samples_bricks_shorter_than_their_side() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = slab_volume(22..26);
    let world = volume.world_extent()[2];
    let focus = (24.5 / 48.0 - 0.5) * world;
    let plane = section_render(
        &device,
        &queue,
        &texture,
        &volume,
        Axis::Z,
        [0.0, 0.0, focus],
    );
    assert!(
        bright_fraction(&plane, 0.25, 0.75, 0.25, 0.75) > 0.9,
        "a plane inside a short brick reads the brick it lies in"
    );
}

#[test]
fn marches_bricks_shorter_than_their_side() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = slab_volume(24..26);
    let world = Vec3::from(volume.world_extent());
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    camera.restore(CameraState {
        pivot: [0.0; 3],
        yaw: 0.0,
        pitch: 0.0,
        distance: camera.distance(),
        fov_y: camera.fov_y(),
    });
    let pixels = render(Setup {
        device: &device,
        queue: &queue,
        job: Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: None,
        },
        camera: Some(&camera),
        section: None,
        markers: &[],
        shading: None,
    });
    let darkest = (SIDE * 2 / 5..SIDE * 3 / 5)
        .flat_map(|y| (SIDE * 2 / 5..SIDE * 3 / 5).map(move |x| (x, y)))
        .map(|(x, y)| pixel(&pixels, x, y).map(u32::from))
        .map(|[red, green, blue, _]| red + green + blue)
        .min()
        .expect("the crop holds pixels");
    assert!(
        darkest > 400,
        "a ray inside a short brick reads the brick it lies in: {darkest}"
    );
}

#[test]
fn marches_a_bead_inside_a_sparse_brick() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = bead_volume(Extent::new(128, 128, 64), Spacing::new(0.1, 0.1, 0.2));
    let pixels = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: None,
        },
    );
    assert!(
        lit(&pixels) > 40,
        "a bead inside one brick still renders: {} lit pixels",
        lit(&pixels)
    );
    let brightest = pixels
        .iter()
        .map(|pixel| pixel.map(u32::from))
        .map(|[red, green, blue, _]| red + green + blue)
        .max()
        .expect("the pane holds pixels");
    assert!(
        brightest > 400,
        "the bead survives empty cell skipping: {brightest}"
    );
}

fn bead_volume(extent: Extent, spacing: Spacing) -> Volume {
    let mut data = vec![0u16; extent.voxel_count() as usize];
    let centre = [80.5, 48.5, 48.5];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let radius = (0..3)
                    .map(|axis| ([x as f32, y as f32, z as f32][axis] - centre[axis]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                if radius <= 6.0 {
                    data[extent.index(x, y, z) as usize] = 60000;
                }
            }
        }
    }
    Volume::new(
        extent,
        spacing,
        vec![Channel::new("bead", [1.0, 1.0, 1.0], VoxelData::U16(data))],
    )
}

fn slab_volume(ribbon: std::ops::Range<u32>) -> Volume {
    let extent = Extent::new(64, 64, 48);
    let mut voxels = vec![0u16; extent.voxel_count() as usize];
    for z in ribbon {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                voxels[extent.index(x, y, z) as usize] = 60000;
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.2),
        vec![Channel::new(
            "slab",
            [1.0, 1.0, 1.0],
            VoxelData::U16(voxels),
        )],
    )
}

#[test]
fn clips_to_a_region_box() {
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
    let source = |request: grana_volume::BrickRequest| {
        let grid = BrickGrid::new(volume.extent());
        volume.brick(&grid, request.coord)
    };
    let session = |region: Region| Session {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
        region,
        time: 0,
    };

    settle(&mut renderer, session(Region::none()), source);
    let full = bright_pixels(&read_pixels(&device, &queue, &texture, SIDE));

    let left = Region::new(
        Vec3::new(-half.x, -half.y, -half.z),
        Vec3::new(0.0, half.y, half.z),
    );
    settle(&mut renderer, session(left), source);
    let cropped = bright_pixels(&read_pixels(&device, &queue, &texture, SIDE));

    let corner = Region::new(Vec3::splat(0.75 * half.x), Vec3::splat(half.x));
    settle(&mut renderer, session(corner), source);
    let erased = bright_pixels(&read_pixels(&device, &queue, &texture, SIDE));

    assert!(full > 0, "the unrestricted volume is empty");
    assert!(
        cropped > 0 && cropped < full,
        "the region does not cut the volume: {cropped} of {full}"
    );
    assert_eq!(
        erased, 0,
        "a region away from the volume still renders bricks"
    );

    let plane_out = Region::new(
        Vec3::new(-half.x, -half.y, 0.5 * half.z),
        Vec3::new(half.x, half.y, half.z),
    );
    settle(
        &mut renderer,
        Session {
            volume_region: CORNER,
            section_region: WHOLE,
            region: plane_out,
            ..session(Region::none())
        },
        source,
    );
    let section = read_pixels(&device, &queue, &texture, SIDE);
    assert!(
        dark_fraction(&section, 0.0, 1.0, 0.0, 1.0) > 0.9,
        "the section ignores the region"
    );
}

#[test]
fn clips_to_a_plane() {
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
    let source = |request: grana_volume::BrickRequest| {
        let grid = BrickGrid::new(volume.extent());
        volume.brick(&grid, request.coord)
    };
    let session = || Session {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
        region: Region::none(),
        time: 0,
    };

    settle(&mut renderer, session(), source);
    let full = bright_pixels(&read_pixels(&device, &queue, &texture, SIDE));

    settle_clipped(&mut renderer, session(), Clip::new(Vec3::X, 0.0), source);
    let cut = bright_pixels(&read_pixels(&device, &queue, &texture, SIDE));

    settle_clipped(
        &mut renderer,
        session(),
        Clip::new(Vec3::X, -0.65 * half.x),
        source,
    );
    let erased = bright_pixels(&read_pixels(&device, &queue, &texture, SIDE));

    assert!(full > 0, "the unclipped volume is empty");
    assert!(
        cut > 0 && cut < full,
        "the plane does not cut the volume: {cut} of {full}"
    );
    assert_eq!(erased, 0, "a plane away from the volume still renders it");

    settle_clipped(
        &mut renderer,
        Session {
            volume_region: CORNER,
            section_region: WHOLE,
            ..session()
        },
        Clip::new(-Vec3::Z, -0.65 * half.z),
        source,
    );
    let section = read_pixels(&device, &queue, &texture, SIDE);
    assert!(
        dark_fraction(&section, 0.0, 1.0, 0.0, 1.0) > 0.9,
        "the section ignores the plane"
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
        red_count(&missed, SECTION_AREA),
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
fn reveals_objects_behind_a_transparent_object() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let empty = empty_volume();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(empty.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let reach = 0.3 * Vec3::from(empty.world_extent()).length();
    let red = [1.0, 0.1, 0.1];
    let green = [0.1, 1.0, 0.1];
    let pair = |alpha: f32| {
        [
            Marker::new([0.0, 0.0, reach], 0.6 * reach, red, alpha),
            Marker::new([0.0, 0.0, -reach], 0.6 * reach, green, alpha),
        ]
    };
    let job = || Job {
        texture: &texture,
        volume: &empty,
        volume_region: WHOLE,
        section_region: CORNER,
        focus_z: 0.0,
        mesh: None,
    };
    let opaque = marker_scene(&device, &queue, job(), &camera, &section, &pair(1.0));
    let blended = marker_scene(&device, &queue, job(), &camera, &section, &pair(0.5));
    let opaque = pixel(&opaque, SIDE / 2, SIDE / 2).map(u32::from);
    let blended = pixel(&blended, SIDE / 2, SIDE / 2).map(u32::from);
    let hidden = opaque[0].min(opaque[1]);
    let shown = blended[0].min(blended[1]);
    assert!(
        opaque[0].max(opaque[1]) > hidden + 100,
        "the opaque objects do not hide one another: {opaque:?}"
    );
    assert!(
        shown > hidden + 30,
        "the transparent near object hides the far one: {opaque:?} {blended:?}"
    );
}

#[test]
fn orders_transparent_objects_by_depth() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let empty = empty_volume();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(empty.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let red = [1.0, 0.1, 0.1];
    let green = [0.1, 1.0, 0.1];
    let reach = 0.3 * Vec3::from(empty.world_extent()).length();
    let pair = |near: [f32; 3], far: [f32; 3]| {
        [
            Marker::new([0.0, 0.0, reach], 0.6 * reach, near, 0.5),
            Marker::new([0.0, 0.0, -reach], 0.6 * reach, far, 0.5),
        ]
    };

    let job = || Job {
        texture: &texture,
        volume: &empty,
        volume_region: WHOLE,
        section_region: CORNER,
        focus_z: 0.0,
        mesh: None,
    };
    let first = marker_scene(&device, &queue, job(), &camera, &section, &pair(red, green));
    let second = marker_scene(&device, &queue, job(), &camera, &section, &pair(green, red));
    let centre = (SIDE / 2, SIDE / 2);
    let first = pixel(&first, centre.0, centre.1);
    let second = pixel(&second, centre.0, centre.1);
    let first_warm = first[0] > first[1] + 8;
    let second_warm = second[0] > second[1] + 8;
    assert!(
        first_warm != second_warm,
        "the layer order ignores depth: {first:?} {second:?}"
    );
}

#[test]
fn blends_transparent_surfaces() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mesh = PackedMesh::new(cubes());
    let solid = [
        Style::new([1.0, 0.12, 0.12], true, false, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let faded = [
        Style::new([1.0, 0.12, 0.12], true, false, 0.4),
        Style::new([0.12, 1.0, 0.12], true, false, 0.4),
    ];
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let job = Job {
        texture: &texture,
        volume: &volume,
        volume_region: WHOLE,
        section_region: CORNER,
        focus_z: 0.0,
        mesh: Some((&mesh, &solid)),
    };
    let faded_job = Job {
        mesh: Some((&mesh, &faded)),
        ..job
    };
    let opaque = framing_scene(&device, &queue, job, &camera, None);
    let blended = framing_scene(&device, &queue, faded_job, &camera, None);
    assert!(red_count(&opaque, WHOLE) > 100, "the surfaces are missing");
    assert!(
        red_count(&blended, WHOLE) > 100,
        "the transparent surfaces are missing"
    );
    let red_objects = |pixels: &[[u8; 4]]| {
        let (sum, count) = pixels
            .iter()
            .filter(|pixel| {
                u32::from(pixel[0]) > 120 && u32::from(pixel[1]) + 40 < u32::from(pixel[0])
            })
            .fold((0u64, 0u64), |(sum, count), pixel| {
                (sum + u64::from(pixel[0]), count + 1)
            });
        (sum, count)
    };
    let (opaque_sum, opaque_count) = red_objects(&opaque);
    let (blended_sum, blended_count) = red_objects(&blended);
    assert!(
        opaque_count > 100 && blended_count > 100,
        "the red surfaces are missing"
    );
    assert!(
        blended_sum * opaque_count < opaque_sum * blended_count,
        "the transparent surfaces are drawn opaque"
    );
}

#[test]
fn composites_transparent_markers_beyond_the_layers() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let empty = empty_volume();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(empty.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let eye = camera.eye();
    let forward = camera.forward();
    let red = [1.0, 0.12, 0.12];
    let blue = [0.12, 0.2, 1.0];
    let along = |index: usize| eye + forward * (5.6 + 0.4 * index as f32);
    let mut markers = (0..4)
        .map(|index| Marker::new(along(index).to_array(), 0.25, red, 0.15))
        .collect::<Vec<_>>();
    let near = markers.clone();
    for index in 4..6 {
        markers.push(Marker::new(along(index).to_array(), 0.25, blue, 0.9));
    }
    let job = || Job {
        texture: &texture,
        volume: &empty,
        volume_region: WHOLE,
        section_region: CORNER,
        focus_z: 0.0,
        mesh: None,
    };
    let near = marker_scene(&device, &queue, job(), &camera, &section, &near);
    let all = marker_scene(&device, &queue, job(), &camera, &section, &markers);
    let centre = (SIDE / 2, SIDE / 2);
    let near = pixel(&near, centre.0, centre.1).map(u32::from);
    let all = pixel(&all, centre.0, centre.1).map(u32::from);
    assert!(
        all[2] > near[2] + 30,
        "the markers beyond the fourth layer are dropped: {near:?} {all:?}"
    );
}

#[test]
fn composites_transparent_surfaces_beyond_the_layers() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mesh = PackedMesh::new(view_stack(&camera, 6));
    let red = [1.0, 0.12, 0.12];
    let blue = [0.12, 0.2, 1.0];
    let shown = (0..6)
        .map(|index| style_of(index, red, blue, true))
        .collect::<Vec<_>>();
    let job = || Job {
        texture: &texture,
        volume: &volume,
        volume_region: WHOLE,
        section_region: CORNER,
        focus_z: 0.0,
        mesh: Some((&mesh, &shown)),
    };
    let blocked = (0..6)
        .map(|index| style_of(index, red, blue, index < 4))
        .collect::<Vec<_>>();
    let blocked_job = Job {
        mesh: Some((&mesh, &blocked)),
        ..job()
    };
    let near = framing_scene(&device, &queue, job(), &camera, None);
    let blocked = framing_scene(&device, &queue, blocked_job, &camera, None);
    let centre = (SIDE / 2, SIDE / 2);
    let near = pixel(&near, centre.0, centre.1).map(u32::from);
    let blocked = pixel(&blocked, centre.0, centre.1).map(u32::from);
    assert!(
        near[2] > blocked[2] + 30,
        "the surfaces beyond the fourth layer are dropped: {near:?} {blocked:?}"
    );
}

#[test]
fn composites_transparent_surfaces_beyond_the_stored_layers() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mesh = PackedMesh::new(view_row(&camera, 20, 0.25));
    let red = [1.0, 0.12, 0.12];
    let blue = [0.12, 0.2, 1.0];
    let styles = |far: bool| {
        (0..20)
            .map(|index| {
                if index < 16 {
                    Style::new(red, true, false, 0.05)
                } else {
                    Style::new(blue, far, false, 0.9)
                }
            })
            .collect::<Vec<_>>()
    };
    let shown_styles = styles(true);
    let hidden_styles = styles(false);
    let shown = framing_scene(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&mesh, &shown_styles)),
        },
        &camera,
        None,
    );
    let hidden = framing_scene(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&mesh, &hidden_styles)),
        },
        &camera,
        None,
    );
    let centre = (SIDE / 2, SIDE / 2);
    let shown = pixel(&shown, centre.0, centre.1).map(u32::from);
    let hidden = pixel(&hidden, centre.0, centre.1).map(u32::from);
    assert!(
        shown[2] > hidden[2] + 30,
        "the surfaces beyond the stored layers are dropped: {shown:?} {hidden:?}"
    );
}

#[test]
fn composites_the_tail_inside_a_split_pane() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mesh = PackedMesh::new(view_row(&camera, 20, 0.25));
    let red = [1.0, 0.12, 0.12];
    let blue = [0.12, 0.2, 1.0];
    let styles = |far: bool| {
        (0..20)
            .map(|index| {
                if index < 16 {
                    Style::new(red, true, false, 0.05)
                } else {
                    Style::new(blue, far, false, 0.9)
                }
            })
            .collect::<Vec<_>>()
    };
    let shown_styles = styles(true);
    let hidden_styles = styles(false);
    let pane = [0, 0, SIDE / 2, SIDE / 2];
    let shown = framing_scene(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: pane,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&mesh, &shown_styles)),
        },
        &camera,
        None,
    );
    let hidden = framing_scene(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: pane,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&mesh, &hidden_styles)),
        },
        &camera,
        None,
    );
    let blue_fraction = |pixels: &[[u8; 4]]| {
        let mut total = 0u64;
        for y in pane[1]..pane[1] + pane[3] {
            for x in pane[0]..pane[0] + pane[2] {
                total += u64::from(pixel(pixels, x, y)[2]);
            }
        }
        total as f64 / (f64::from(pane[2]) * f64::from(pane[3]))
    };
    let shown_blue = blue_fraction(&shown);
    let hidden_blue = blue_fraction(&hidden);
    assert!(
        shown_blue > hidden_blue + 8.0,
        "the pane drops the surfaces beyond the stored layers: {shown_blue} {hidden_blue}"
    );
    let outside = pixel(&shown, SIDE * 3 / 4, SIDE / 4).map(u32::from);
    let outside_hidden = pixel(&hidden, SIDE * 3 / 4, SIDE / 4).map(u32::from);
    assert!(
        outside[2] <= outside_hidden[2] + 10,
        "the tail of a pane spills outside its region: {outside:?} {outside_hidden:?}"
    );
}

#[test]
fn downsamples_the_transparent_tail_of_a_large_pane() {
    let Some((device, queue)) = device() else {
        return;
    };
    let size = [1024u32, 1026];
    let texture = common::target_rect(&device, size);
    let volume = empty_volume();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mesh = PackedMesh::new(view_row(&camera, 20, 0.25));
    let red = [1.0, 0.12, 0.12];
    let blue = [0.12, 0.2, 1.0];
    let styles = |far: bool| {
        (0..20)
            .map(|index| {
                if index < 16 {
                    Style::new(red, true, false, 0.05)
                } else {
                    Style::new(blue, far, false, 0.9)
                }
            })
            .collect::<Vec<_>>()
    };
    let shown_styles = styles(true);
    let hidden_styles = styles(false);
    let pane = [0, 0, size[0], size[1]];
    let render = |styles: &[Style]| {
        render_rect(
            Setup {
                device: &device,
                queue: &queue,
                job: Job {
                    texture: &texture,
                    volume: &volume,
                    volume_region: pane,
                    section_region: CORNER,
                    focus_z: 0.0,
                    mesh: Some((&mesh, styles)),
                },
                camera: Some(&camera),
                section: None,
                markers: &[],
                shading: None,
            },
            size,
        )
    };
    let shown = render(&shown_styles);
    let hidden = render(&hidden_styles);
    let blue_fraction = |pixels: &[[u8; 4]]| {
        let mut total = 0u64;
        for y in pane[1]..pane[1] + pane[3] {
            for x in pane[0]..pane[0] + pane[2] {
                total += u64::from(pixels[(y * size[0] + x) as usize][2]);
            }
        }
        total as f64 / (f64::from(pane[2]) * f64::from(pane[3]))
    };
    let shown_blue = blue_fraction(&shown);
    let hidden_blue = blue_fraction(&hidden);
    assert!(
        shown_blue > hidden_blue + 8.0,
        "the downsampled tail drops the surfaces beyond the stored layers: {shown_blue} {hidden_blue}"
    );
}

#[test]
fn keeps_near_transparent_surfaces_drawn_after_far_ones() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let far = PackedMesh::new(receding_planes(&camera, 16, 0.5, 0.17));
    let near = PackedMesh::new(view_plane(&camera, -1.2, 0.9));
    let blue = [0.12, 0.2, 1.0];
    let red = [1.0, 0.12, 0.12];
    let mut styles = (0..16)
        .map(|_| Style::new(blue, true, false, 0.05))
        .collect::<Vec<_>>();
    styles.push(Style::new(red, true, false, 0.9));
    let layers = [
        Layer {
            key: LayerKey::at_frame(1, 0, 0, 1),
            mesh: Arc::new(far.clone()),
            style_start: 0,
            style_count: 16,
        },
        Layer {
            key: LayerKey::at_frame(2, 0, 0, 1),
            mesh: Arc::new(near.clone()),
            style_start: 16,
            style_count: 1,
        },
    ];
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        markers: &[],
    };
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
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
            region: Region::none(),
            time: 0,
        },
        |request| {
            let grid = BrickGrid::new(volume.extent());
            volume.brick(&grid, request.coord)
        },
    );
    let pixels = read_pixels(&device, &queue, &texture, SIDE);
    let centre = pixel(&pixels, SIDE / 2, SIDE / 2).map(u32::from);
    assert!(
        centre[0] > centre[2] + 40,
        "the nearest transparent surface vanishes when farther ones fill the layers first: {centre:?}"
    );
}

fn receding_planes(camera: &Camera, count: usize, first: f32, step: f32) -> Mesh {
    let pivot = camera.focus();
    let forward = camera.forward();
    let right = camera.right();
    let up = camera.up();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for object in 0..count {
        let centre = pivot + forward * (first + step * object as f32);
        let base = vertices.len() as u32;
        for corner in [[-1.0f32, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
            let position = centre + (right * corner[0] + up * corner[1]) * 0.9;
            vertices.push(MeshVertex::new(
                position.to_array(),
                (-forward).to_array(),
                object as u32,
            ));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(vertices, indices)
}

fn rotated_plane(camera: &Camera, angle: f32, half: f32) -> Mesh {
    let centre = camera.focus();
    let right = camera.right();
    let up = camera.up();
    let (sine, cosine) = angle.sin_cos();
    let mut vertices = Vec::new();
    for corner in [[-1.0f32, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
        let local = [
            corner[0] * cosine - corner[1] * sine,
            corner[0] * sine + corner[1] * cosine,
        ];
        let position = centre + (right * local[0] + up * local[1]) * half;
        vertices.push(MeshVertex::new(
            position.to_array(),
            (-camera.forward()).to_array(),
            0,
        ));
    }
    Mesh::new(vertices, vec![0, 1, 2, 0, 2, 3])
}

fn view_plane(camera: &Camera, offset: f32, half: f32) -> Mesh {
    let centre = camera.focus() + camera.forward() * offset;
    let right = camera.right();
    let up = camera.up();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for corner in [[-1.0f32, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
        let position = centre + (right * corner[0] + up * corner[1]) * half;
        vertices.push(MeshVertex::new(
            position.to_array(),
            (-camera.forward()).to_array(),
            0,
        ));
    }
    indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    Mesh::new(vertices, indices)
}

#[test]
fn orders_transparent_surfaces_independently_of_draw_order() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let forward = (0..8).collect::<Vec<_>>();
    let backward = (0..8).rev().collect::<Vec<_>>();
    let red = [1.0, 0.12, 0.12];
    let blue = [0.12, 0.2, 1.0];
    let styles = |order: &[usize]| {
        order
            .iter()
            .map(|depth| style_of(*depth, red, blue, true))
            .collect::<Vec<_>>()
    };
    let near_styles = styles(&forward);
    let far_styles = styles(&backward);
    let near = PackedMesh::new(stacked_view(&camera, &forward));
    let far = PackedMesh::new(stacked_view(&camera, &backward));
    let near = framing_scene(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&near, &near_styles)),
        },
        &camera,
        None,
    );
    let far = framing_scene(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&far, &far_styles)),
        },
        &camera,
        None,
    );
    assert!(
        near == far,
        "the transparent surfaces composite in draw order instead of depth order"
    );
}

fn stacked_view(camera: &Camera, order: &[usize]) -> Mesh {
    let eye = camera.eye();
    let forward = camera.forward();
    let right = camera.right();
    let up = camera.up();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, depth) in order.iter().enumerate() {
        let centre = eye + forward * (5.6 + 0.4 * *depth as f32);
        let base = vertices.len() as u32;
        for corner in [[-1.0f32, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
            let position = centre + (right * corner[0] + up * corner[1]) * 0.9;
            vertices.push(MeshVertex::new(
                position.to_array(),
                (-forward).to_array(),
                object as u32,
            ));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(vertices, indices)
}

fn style_of(index: usize, red: [f32; 3], blue: [f32; 3], visible: bool) -> Style {
    if index < 4 {
        Style::new(red, visible, false, 0.15)
    } else {
        Style::new(blue, visible, false, 0.9)
    }
}

fn view_stack(camera: &Camera, count: usize) -> Mesh {
    view_row(camera, count, 0.4)
}

fn view_row(camera: &Camera, count: usize, spacing: f32) -> Mesh {
    let eye = camera.eye();
    let forward = camera.forward();
    let right = camera.right();
    let up = camera.up();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for object in 0..count {
        let centre = eye + forward * (5.6 + spacing * object as f32);
        let base = vertices.len() as u32;
        for corner in [[-1.0f32, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
            let position = centre + (right * corner[0] + up * corner[1]) * 0.9;
            vertices.push(MeshVertex::new(
                position.to_array(),
                (-forward).to_array(),
                object as u32,
            ));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(vertices, indices)
}

#[test]
fn renders_surface_objects_and_sections() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mesh = PackedMesh::new(cubes());
    let visible = [
        Style::new([1.0, 0.12, 0.12], true, false, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let selected = [
        Style::new([1.0, 0.12, 0.12], true, true, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let hidden = [
        Style::new([1.0, 0.12, 0.12], false, false, 1.0),
        Style::new([0.12, 1.0, 0.12], false, false, 1.0),
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
        red_count(&missed, SECTION_AREA),
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
    let mesh = PackedMesh::new(tracks().mesh(0.15));
    let visible = [
        Style::new([1.0, 0.12, 0.12], true, false, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let hidden = [
        Style::new([1.0, 0.12, 0.12], false, false, 1.0),
        Style::new([0.12, 1.0, 0.12], false, false, 1.0),
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
fn keeps_hollow_bricks_out_of_the_cache() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = corner_brick();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
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
        8 * SLOT_BYTES,
    );
    renderer.set_dataset(&device, &pyramid);
    let session = Session {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
        region: Region::none(),
        time: 0,
    };
    let grid = BrickGrid::new(volume.extent());
    for _ in 0..96 {
        renderer.draw(&device, &queue, common::frame(&session));
        let Some(requests) = common::next_demand(&mut renderer, &session) else {
            continue;
        };
        for request in requests.volume.iter().chain(&requests.section) {
            let brick = volume.brick(&grid, request.coord);
            let _ = renderer.upload(&device, &queue, *request, &brick);
        }
    }
    let pixels = read_pixels(&device, &queue, &texture, SIDE);
    assert!(
        lit(&pixels) > 100,
        "a cache of empty bricks hides the only solid brick: {} lit pixels",
        lit(&pixels)
    );
}

fn corner_brick() -> Volume {
    let extent = Extent::new(128, 128, 128);
    let mut data = vec![0u16; extent.voxel_count() as usize];
    for z in 0..32 {
        for y in 0..32 {
            for x in 0..32 {
                data[extent.index(x, y, z) as usize] = 60_000;
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.1),
        vec![Channel::new(
            "corner",
            [0.3, 0.6, 1.0],
            VoxelData::U16(data),
        )],
    )
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
        3,
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
        100 * SLOT_BYTES,
    );
    renderer.set_dataset(&device, &pyramid);
    let source = |request: grana_volume::BrickRequest| {
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
            region: Region::none(),
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
            region: Region::none(),
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
            region: Region::none(),
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
            region: Region::none(),
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
    shading: Option<Shading>,
}

fn scene_render(device: &wgpu::Device, queue: &wgpu::Queue, job: Job<'_>) -> Vec<[u8; 4]> {
    render(Setup {
        device,
        queue,
        job,
        camera: None,
        section: None,
        markers: &[],
        shading: None,
    })
}

fn framing_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    job: Job<'_>,
    camera: &Camera,
    shading: Option<Shading>,
) -> Vec<[u8; 4]> {
    render(Setup {
        device,
        queue,
        job,
        camera: Some(camera),
        section: None,
        markers: &[],
        shading,
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
        shading: None,
    })
}

fn render(setup: Setup<'_>) -> Vec<[u8; 4]> {
    render_rect(setup, [SIDE, SIDE])
}

fn render_rect(setup: Setup<'_>, size: [u32; 2]) -> Vec<[u8; 4]> {
    let Setup {
        device,
        queue,
        job,
        camera,
        section,
        markers,
        shading,
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
            mesh: Arc::new(mesh.clone()),
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
        shading: shading.unwrap_or_default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        markers,
    };
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(device, &pyramid);
    common::settle_sized(
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
            region: Region::none(),
            time: 0,
        },
        size,
        |request| {
            let grid = BrickGrid::new(volume.extent());
            volume.brick(&grid, request.coord)
        },
    );
    common::read_pixels_rect(device, queue, job.texture, size)
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

fn packed_render(volume: Volume) -> Option<Vec<[u8; 4]>> {
    let (device, queue) = device()?;
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
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
            region: Region::none(),
            time: 0,
        },
        |request| {
            let grid = BrickGrid::new(volume.extent());
            volume.brick(&grid, request.coord)
        },
    );
    Some(read_pixels(&device, &queue, &texture, SIDE))
}

fn packed_volume(channels: usize, active: usize) -> Volume {
    let extent = Extent::new(128, 128, 96);
    let count = extent.voxel_count() as usize;
    let mut red = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let index = extent.index(x, y, z) as usize;
                red[index] = density(x, y, z, 40.0);
            }
        }
    }
    let tint = |channel: usize| {
        if channel == active {
            [1.0, 0.2, 0.2]
        } else {
            [0.2, 0.4, 1.0]
        }
    };
    let channels = (0..channels)
        .map(|channel| {
            let data = if channel == active {
                VoxelData::U16(red.clone())
            } else {
                VoxelData::U16(vec![0; count])
            };
            Channel::new(format!("channel {}", channel + 1), tint(channel), data)
        })
        .collect();
    Volume::new(extent, Spacing::new(0.1, 0.1, 0.13), channels)
}

fn density(x: u32, y: u32, z: u32, centre_x: f32) -> u16 {
    let radius =
        ((x as f32 - centre_x).powi(2) + (y as f32 - 64.0).powi(2) + (z as f32 - 48.0).powi(2))
            .sqrt();
    let density = (1.0 - radius / 30.0).max(0.0);
    (density * density * 60000.0) as u16
}

fn two_frame_movie() -> VolumeSeries {
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
    VolumeSeries::new(
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

#[test]
fn draws_wide_packed_objects() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mesh = PackedMesh::new(grid());
    assert!(
        !mesh.wide().is_empty(),
        "the grid is packed with wide indices"
    );
    let styles = [Style::new([1.0, 0.12, 0.12], true, false, 1.0)];
    let pixels = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: WHOLE,
            section_region: CORNER,
            focus_z: 0.0,
            mesh: Some((&mesh, &styles)),
        },
    );
    assert!(
        red_pixels(&pixels) > 4000,
        "the wide object barely rendered: {} pixels",
        red_pixels(&pixels)
    );
}

#[test]
fn keeps_object_geometry_resident() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let mesh = PackedMesh::new(cubes());
    let styles = [Style::new([1.0, 1.0, 1.0], true, false, 1.0); 2];
    let transfer = TransferFunction::tinted([0.3, 0.6, 1.0]);
    let transfers = [transfer];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
    let first = [Layer {
        key: LayerKey::stable(1, 0, 1),
        mesh: Arc::new(mesh.clone()),
        style_start: 0,
        style_count: 2,
    }];
    let second = [Layer {
        key: LayerKey::stable(1, 0, 2),
        mesh: Arc::new(mesh.clone()),
        style_start: 0,
        style_count: 2,
    }];
    let resident = Resident {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        channels: &channels,
        styles: &styles,
    };
    let a = resident.geometry(&mut renderer, &first);
    let b = resident.geometry(&mut renderer, &second);
    let c = resident.geometry(&mut renderer, &first);
    assert!(a > 0, "the frame carries no geometry");
    assert!(b > a, "the second frame adds its geometry");
    assert_eq!(c, b, "the first frame stays resident");
}

struct Resident<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    target: &'a wgpu::TextureView,
    camera: &'a Camera,
    section: &'a SectionCamera,
    channels: &'a [grana_render::ChannelRow<'a>],
    styles: &'a [Style],
}

impl Resident<'_> {
    fn geometry(&self, renderer: &mut Renderer, layers: &[Layer]) -> u64 {
        let scene = SceneFrame {
            channels: self.channels,
            shading: Shading::default(),
            step_scale: 1.0,
            styles: self.styles,
            layers,
            markers: &[],
        };
        renderer
            .draw(
                self.device,
                self.queue,
                Frame {
                    target: self.target,
                    size: [SIDE, SIDE],
                    volume_region: WHOLE,
                    section_region: CORNER,
                    time: 0,
                    camera: self.camera,
                    section: self.section,
                    region: Region::none(),
                    clip: Clip::none(),
                    exposure: 1.0,
                    samples: SAMPLES,
                    scene: &scene,
                    prefetch: false,
                },
            )
            .geometry
    }
}

fn grid() -> Mesh {
    let side = 266u32;
    let mut vertices = Vec::with_capacity((side * side) as usize);
    for y in 0..side {
        for x in 0..side {
            let u = x as f32 / (side - 1) as f32 * 2.0 - 1.0;
            let v = y as f32 / (side - 1) as f32 * 2.0 - 1.0;
            vertices.push(MeshVertex::new([u, v, 0.0], [0.0, 0.0, 1.0], 0));
        }
    }
    let mut indices = Vec::new();
    for y in 0..side - 1 {
        for x in 0..side - 1 {
            let corner = y * side + x;
            indices.extend([
                corner,
                corner + 1,
                corner + side,
                corner + 1,
                corner + side + 1,
                corner + side,
            ]);
        }
    }
    Mesh::new(vertices, indices)
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

#[test]
fn accumulates_samples_until_the_view_changes() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = volume(0);
    let pyramid = volume.pyramid();
    let transfer = TransferFunction::tinted([0.3, 0.6, 1.0]);
    let transfers = [transfer];
    let channels = channels(&pyramid, &transfers);
    let camera = Camera::new(35.0);
    let section = SectionCamera::new(Axis::Z);
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
    let session = Session {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
        region: Region::none(),
        time: 0,
    };
    let first = renderer.draw(&device, &queue, common::frame(&session));
    assert_eq!(first.samples, 1, "a new view resolves one sample");
    let second = renderer.draw(&device, &queue, common::frame(&session));
    assert_eq!(second.samples, 2, "a still view accumulates another sample");
    for _ in 2..SAMPLES {
        renderer.draw(&device, &queue, common::frame(&session));
    }
    let settled = renderer.draw(&device, &queue, common::frame(&session));
    assert_eq!(settled.samples, SAMPLES, "accumulation stops at the limit");
    let mut moved = Camera::new(35.0);
    moved.zoom(0.25);
    let departed = renderer.draw(
        &device,
        &queue,
        common::frame(&Session {
            camera: &moved,
            ..session
        }),
    );
    assert_eq!(departed.samples, 1, "a moved camera restarts accumulation");
}

#[test]
fn shadows_darken_the_volume() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = sphere(Extent::new(128, 128, 128), Spacing::new(0.1, 0.1, 0.2));
    let pyramid = volume.pyramid();
    let transfer = TransferFunction::tinted([0.3, 0.6, 1.0]);
    let transfers = [transfer];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let mut lit = [0u64; 2];
    for (slot, shadows) in [0.0f32, 1.0].into_iter().enumerate() {
        let scene = SceneFrame {
            channels: &channels,
            shading: Shading {
                shadows,
                ..Shading::default()
            },
            step_scale: 1.0,
            styles: &[],
            layers: &[],
            markers: &[],
        };
        let view = texture.create_view(&Default::default());
        let pixels = render_samples(
            &texture,
            &volume,
            Session {
                device: &device,
                queue: &queue,
                target: &view,
                camera: &camera,
                section: &section,
                scene: &scene,
                volume_region: WHOLE,
                section_region: CORNER,
                region: Region::none(),
                time: 0,
            },
        );
        lit[slot] = green_sum(&pixels);
    }
    assert!(
        lit[1] * 1000 < lit[0] * 997,
        "cast shadows do not darken the volume: {lit:?}"
    );
}

#[test]
fn objects_cast_shadows_onto_surfaces() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let mesh = PackedMesh::new(casting_planes([-0.6, 0.6]));
    let styles = [Style::new([0.98, 0.72, 0.5], true, false, 1.0); 2];
    let layers = [Layer {
        key: LayerKey::at_frame(1, 0, 0, 1),
        mesh: Arc::new(mesh.clone()),
        style_start: 0,
        style_count: 2,
    }];
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let mut lit = [0u64; 2];
    for (slot, shadows) in [0.0f32, 0.95].into_iter().enumerate() {
        let scene = SceneFrame {
            channels: &channels,
            shading: Shading {
                shadows,
                ..Shading::default()
            },
            step_scale: 1.0,
            styles: &styles,
            layers: &layers,
            markers: &[],
        };
        let view = texture.create_view(&Default::default());
        let pixels = render_samples(
            &texture,
            &volume,
            Session {
                device: &device,
                queue: &queue,
                target: &view,
                camera: &camera,
                section: &section,
                scene: &scene,
                volume_region: WHOLE,
                section_region: CORNER,
                region: Region::none(),
                time: 0,
            },
        );
        lit[slot] = pixels.iter().map(|pixel| u64::from(pixel[0])).sum();
    }
    assert!(
        lit[1] * 1000 < lit[0] * 995,
        "objects cast no shadow onto a surface: {lit:?}"
    );
}

fn casting_planes(offset: [f32; 2]) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, (height, extent, shift)) in [(-1.5f32, 1.5f32, [0.0f32, 0.0]), (-0.4, 0.6, offset)]
        .into_iter()
        .enumerate()
    {
        let base = vertices.len() as u32;
        for (x, y) in [
            (-extent, -extent),
            (extent, -extent),
            (extent, extent),
            (-extent, extent),
        ] {
            vertices.push(MeshVertex::new(
                [x + shift[0], y + shift[1], height],
                [0.0, 0.0, 1.0],
                object as u32,
            ));
        }
        indices.extend([0u32, 1, 2, 0, 2, 3].map(|index| base + index));
    }
    Mesh::new(vertices, indices)
}

#[test]
fn refreshes_object_shadows_when_the_geometry_changes() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
    let styles = [
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
        Style::new([1.0, 0.12, 0.12], true, false, 1.0),
    ];
    let first = PackedMesh::new(casting_planes([-0.6, 0.6]));
    let second = PackedMesh::new(casting_planes([0.6, 0.6]));
    let pixels = ShadowPixels {
        device: &device,
        queue: &queue,
        texture: &texture,
        view: &view,
        camera: &camera,
        section: &section,
        channels: &channels,
        volume: &volume,
        styles: &styles,
    };
    let first_lit = pixels.draw(&mut renderer, &first, 1, 0.0);
    let first_shadowed = pixels.draw(&mut renderer, &first, 1, 0.95);
    let second_lit = pixels.draw(&mut renderer, &second, 2, 0.0);
    let second_shadowed = pixels.draw(&mut renderer, &second, 2, 0.95);
    let brightness = |pixels: &[[u8; 4]], index: usize| -> i32 {
        let [red, green, blue, _] = pixels[index].map(i32::from);
        red + green + blue
    };
    let receiver = |pixels: &[[u8; 4]], index: usize| -> bool {
        let [red, green, blue, _] = pixels[index].map(u32::from);
        green > red + 30 && green > blue + 30
    };
    let mut moved = 0usize;
    for index in 0..first_lit.len() {
        if !receiver(&first_lit, index) || !receiver(&second_lit, index) {
            continue;
        }
        let unshadowed = brightness(&first_lit, index);
        if (unshadowed - brightness(&second_lit, index)).abs() > 8 {
            continue;
        }
        if unshadowed - brightness(&first_shadowed, index) < 30 {
            continue;
        }
        if brightness(&second_lit, index) - brightness(&second_shadowed, index) < 30 {
            moved += 1;
        }
    }
    assert!(
        moved > 60,
        "the object shadow does not follow the moved geometry: {moved}"
    );
}

#[test]
fn projects_object_shadows_outside_the_data_box() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::splat(12.0), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
    let styles = [
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
        Style::new([1.0, 0.12, 0.12], true, false, 1.0),
    ];
    let mesh = PackedMesh::new(shifted(
        casting_planes([-0.6, 0.6]),
        Vec3::new(2.6, 0.0, 0.0),
    ));
    let pixels = ShadowPixels {
        device: &device,
        queue: &queue,
        texture: &texture,
        view: &view,
        camera: &camera,
        section: &section,
        channels: &channels,
        volume: &volume,
        styles: &styles,
    };
    let lit = pixels.draw(&mut renderer, &mesh, 1, 0.0);
    let shadowed = pixels.draw(&mut renderer, &mesh, 1, 0.95);
    let mut darkened = 0usize;
    for index in 0..lit.len() {
        let [red, green, blue, _] = lit[index].map(u32::from);
        if !(green > red + 30 && green > blue + 30) {
            continue;
        }
        let total = |pixel: [u8; 4]| -> i32 {
            let [red, green, blue, _] = pixel.map(i32::from);
            red + green + blue
        };
        if total(lit[index]) - total(shadowed[index]) > 30 {
            darkened += 1;
        }
    }
    assert!(
        darkened > 60,
        "an object outside the data box casts no shadow: {darkened}"
    );
}

fn shifted(mesh: Mesh, shift: Vec3) -> Mesh {
    let vertices = mesh
        .vertices()
        .iter()
        .map(|vertex| {
            MeshVertex::new(
                (Vec3::from(vertex.position()) + shift).to_array(),
                vertex.normal(),
                vertex.object(),
            )
        })
        .collect::<Vec<_>>();
    Mesh::new(vertices, mesh.indices().to_vec())
}

struct ShadowPixels<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    texture: &'a wgpu::Texture,
    view: &'a wgpu::TextureView,
    camera: &'a Camera,
    section: &'a SectionCamera,
    channels: &'a [grana_render::ChannelRow<'a>],
    volume: &'a Volume,
    styles: &'a [Style],
}

impl ShadowPixels<'_> {
    fn draw(
        &self,
        renderer: &mut Renderer,
        mesh: &PackedMesh,
        revision: u64,
        shadows: f32,
    ) -> Vec<[u8; 4]> {
        let layers = [Layer {
            key: LayerKey::at_frame(1, 0, 0, revision),
            mesh: Arc::new(mesh.clone()),
            style_start: 0,
            style_count: self.styles.len(),
        }];
        let scene = SceneFrame {
            channels: self.channels,
            shading: Shading {
                shadows,
                ..Shading::default()
            },
            step_scale: 1.0,
            styles: self.styles,
            layers: &layers,
            markers: &[],
        };
        let session = Session {
            device: self.device,
            queue: self.queue,
            target: self.view,
            camera: self.camera,
            section: self.section,
            scene: &scene,
            volume_region: WHOLE,
            section_region: CORNER,
            region: Region::none(),
            time: 0,
        };
        settle(renderer, session, |request| {
            let grid = BrickGrid::new(self.volume.extent());
            self.volume.brick(&grid, request.coord)
        });
        for _ in 0..SAMPLES {
            renderer.draw(self.device, self.queue, common::frame(&session));
        }
        read_pixels(self.device, self.queue, self.texture, SIDE)
    }
}

fn render_samples(texture: &wgpu::Texture, volume: &Volume, session: Session<'_>) -> Vec<[u8; 4]> {
    let mut renderer = Renderer::new(
        session.device,
        session.queue,
        wgpu::TextureFormat::Rgba8Unorm,
        BUDGET,
    );
    renderer.set_dataset(session.device, &volume.pyramid());
    settle(&mut renderer, session, |request| {
        let grid = BrickGrid::new(volume.extent());
        volume.brick(&grid, request.coord)
    });
    read_pixels(session.device, session.queue, texture, SIDE)
}

#[test]
fn cuts_objects_at_the_plane() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mesh = PackedMesh::new(pyramids());
    let styles = [
        Style::new([1.0, 0.12, 0.12], true, false, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let mut measured = [0usize; 2];
    for (slot, focus) in [-0.7f32, 0.7].into_iter().enumerate() {
        let pixels = scene_render(
            &device,
            &queue,
            Job {
                texture: &texture,
                volume: &volume,
                volume_region: CORNER,
                section_region: WHOLE,
                focus_z: focus,
                mesh: Some((&mesh, &styles)),
            },
        );
        measured[slot] = red_pixels(&pixels) + green_pixels(&pixels);
    }
    assert!(
        measured[0] > 1500,
        "the section does not fill the wide cut: {}",
        measured[0]
    );
    assert!(
        measured[0] > 8 * measured[1],
        "the section projects the object instead of cutting it: {measured:?}"
    );
}

fn pyramids() -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, centre) in [-0.9f32, 0.9].into_iter().enumerate() {
        let base = vertices.len() as u32;
        for (x, y) in [(-1.0f32, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            vertices.push(MeshVertex::new(
                [centre + 0.4 * x, 0.55 + 0.4 * y, -1.0],
                [0.0, 0.0, -1.0],
                object as u32,
            ));
        }
        vertices.push(MeshVertex::new(
            [centre, 0.55, 1.0],
            [0.0, 0.0, 1.0],
            object as u32,
        ));
        let apex = base + 4;
        for corner in 0..4u32 {
            indices.extend([base + corner, base + (corner + 1) % 4, apex]);
        }
        indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    Mesh::new(vertices, indices)
}

fn slab() -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let corners = [
        [-0.8f32, -0.8, -0.5],
        [0.8, -0.8, -0.5],
        [0.8, 0.8, -0.5],
        [-0.8, 0.8, -0.5],
        [-0.8, -0.8, 0.5],
        [0.8, -0.8, 0.5],
        [0.8, 0.8, 0.5],
        [-0.8, 0.8, 0.5],
    ];
    for corner in corners {
        let normal = Vec3::new(
            if corner[0].abs() > 0.79 {
                corner[0].signum()
            } else {
                0.0
            },
            if corner[1].abs() > 0.79 {
                corner[1].signum()
            } else {
                0.0
            },
            if corner[2].abs() > 0.49 {
                corner[2].signum()
            } else {
                0.0
            },
        )
        .normalize();
        vertices.push(MeshVertex::new(corner, normal.to_array(), 0));
    }
    for face in [
        [0u32, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [3, 7, 6],
        [3, 6, 2],
        [0, 4, 7],
        [0, 7, 3],
        [1, 2, 6],
        [1, 6, 5],
    ] {
        indices.extend(face);
    }
    Mesh::new(vertices, indices)
}

#[test]
fn orders_section_markers_against_cut_faces() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mesh = PackedMesh::new(slab());
    let styles = [Style::new([1.0, 0.12, 0.12], true, false, 1.0)];
    let world = Vec3::from(volume.world_extent());
    let voxel = Vec3::new(0.1, 0.1, 0.1);
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    let mut section = SectionCamera::new(Axis::Z);
    section.fit(world, voxel, 1.0);
    let mut measured = [[0u8; 2]; 2];
    for (slot, z) in [-0.15f32, 0.15].into_iter().enumerate() {
        let markers = [Marker::new([0.0, 0.0, z], 0.3, [0.12, 1.0, 0.12], 1.0)];
        let pixels = marker_scene(
            &device,
            &queue,
            Job {
                texture: &texture,
                volume: &volume,
                volume_region: CORNER,
                section_region: WHOLE,
                focus_z: 0.0,
                mesh: Some((&mesh, &styles)),
            },
            &camera,
            &section,
            &markers,
        );
        let [red, green, _, _] = pixel(&pixels, SIDE / 2, SIDE / 2);
        measured[slot] = [red, green];
    }
    assert!(
        measured[0][0] > measured[0][1] && measured[0][1] > 120,
        "a marker behind the plane sits under the cut face: {:?}",
        measured[0]
    );
    assert!(
        measured[1][1] > measured[1][0],
        "a marker in front of the plane sits over the cut face: {:?}",
        measured[1]
    );
}

fn section_job<'a>(
    texture: &'a wgpu::Texture,
    volume: &'a Volume,
    mesh: Option<(&'a PackedMesh, &'a [Style])>,
) -> Job<'a> {
    Job {
        texture,
        volume,
        volume_region: CORNER,
        section_region: WHOLE,
        focus_z: 0.0,
        mesh,
    }
}

fn aces(radiance: f32) -> f32 {
    let toe = 0.03;
    let shoulder = 2.43;
    let offset = 0.59;
    let bias = 0.14;
    let slope = 2.51;
    ((radiance * (slope * radiance + toe)) / (radiance * (shoulder * radiance + offset) + bias))
        .clamp(0.0, 1.0)
}

fn backdrop(ndc: [f32; 2]) -> [f32; 3] {
    let height = (ndc[1] * 0.5 + 0.5).clamp(0.0, 1.0);
    let low = [0.012f32, 0.014, 0.020];
    let high = [0.040f32, 0.048, 0.066];
    [0, 1, 2].map(|channel| low[channel] + (high[channel] - low[channel]) * height)
}

fn expected_cut_fill(color: [f32; 3], energy: f32, ndc: [f32; 2]) -> [i32; 3] {
    let linear = srgb_to_linear(color);
    let backdrop = backdrop(ndc);
    [0, 1, 2].map(|channel| {
        let covered = aces(0.6 * linear[channel] * energy);
        let behind = aces(backdrop[channel]);
        (linear_to_srgb(covered + 0.4 * behind) * 255.0).round() as i32
    })
}

fn brightness(pixel: [u8; 4]) -> u32 {
    pixel.iter().take(3).map(|value| u32::from(*value)).sum()
}

#[test]
fn lights_cut_faces_with_the_surface_shading_model() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mesh = PackedMesh::new(cubes());
    let styles = [
        Style::new([1.0, 0.12, 0.12], true, false, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let configs = [
        Shading {
            ambient: 0.35,
            diffuse: 0.75,
            specular: 0.0,
            shininess: 28.0,
            shadows: 0.0,
            ..Shading::default()
        },
        Shading {
            ambient: 0.8,
            diffuse: 0.2,
            specular: 0.0,
            shininess: 28.0,
            shadows: 0.0,
            ..Shading::default()
        },
        Shading {
            ambient: 0.35,
            diffuse: 0.75,
            specular: 0.0,
            shininess: 28.0,
            shadows: 0.0,
            ..Shading::default()
        },
    ];
    let job = Job {
        texture: &texture,
        volume: &volume,
        volume_region: CORNER,
        section_region: WHOLE,
        focus_z: 0.0,
        mesh: Some((&mesh, &styles)),
    };
    for (slot, shading) in configs.into_iter().enumerate() {
        let mut camera = Camera::new(35.0);
        camera.fit(Vec3::from(volume.world_extent()), 1.0);
        if slot == 2 {
            let state = camera.state();
            camera.restore(CameraState {
                yaw: state.yaw + std::f32::consts::PI,
                ..state
            });
        }
        let pixels = framing_scene(&device, &queue, job, &camera, Some(shading));
        let light = shading.light();
        let energy = shading.ambient + shading.diffuse * light.z.max(0.0);
        let ndc = [
            (2.0 * 75.0 + 1.0) / SIDE as f32 - 1.0,
            1.0 - (2.0 * 128.0 + 1.0) / SIDE as f32,
        ];
        let expected = expected_cut_fill([1.0, 0.12, 0.12], energy, ndc);
        assert!(
            expected[0] > 40,
            "the shading sample is too dark to resolve: {expected:?}"
        );
        let measured = pixel(&pixels, 75, 128);
        for channel in 0..3 {
            let difference = i32::from(measured[channel]) - expected[channel];
            assert!(
                difference.abs() <= 3,
                "the cut fill does not follow the surface shading model: {measured:?} against {expected:?}"
            );
        }
    }
}

fn outline_runs(pixels: &[[u8; 4]], y: u32) -> Vec<(u32, u32)> {
    let mut runs = Vec::new();
    let mut start = None;
    for x in 0..SIDE {
        let [red, green, blue, _] = pixel(pixels, x, y).map(u32::from);
        let outlined = red > green + 20 && red > blue + 20;
        match (outlined, start) {
            (true, None) => start = Some(x),
            (false, Some(from)) => {
                runs.push((from, x - from));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        runs.push((from, SIDE - from));
    }
    runs
}

#[test]
fn outlines_cut_faces_in_screen_space() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mesh = PackedMesh::new(cubes());
    let plain = [
        Style::new([1.0, 0.12, 0.12], true, false, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let selected = [
        Style::new([1.0, 0.12, 0.12], true, true, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let shaded = Shading {
        ambient: 0.0,
        diffuse: 0.0,
        specular: 0.0,
        shininess: 28.0,
        shadows: 0.0,
        ..Shading::default()
    };
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let plain_job = section_job(&texture, &volume, Some((&mesh, &plain)));
    let plain_pixels = framing_scene(&device, &queue, plain_job, &camera, Some(shaded));
    let runs = outline_runs(&plain_pixels, 128);
    assert_eq!(
        runs.len(),
        2,
        "a cut face carries one outline per silhouette edge: {runs:?}"
    );
    let (left, left_width) = runs[0];
    let (right, right_width) = runs[1];
    assert!(
        (20..40).contains(&left) && (1..=6).contains(&left_width),
        "the leading outline is not a thin screen space band: {runs:?}"
    );
    assert!(
        (105..130).contains(&right) && (1..=6).contains(&right_width),
        "the trailing outline is not a thin screen space band: {runs:?}"
    );
    assert!(
        brightness(pixel(&plain_pixels, 75, 128)) < 140,
        "the cut fill shades the face under a zero ambient shading"
    );
    let selected_job = section_job(&texture, &volume, Some((&mesh, &selected)));
    let selected_pixels = framing_scene(&device, &queue, selected_job, &camera, Some(shaded));
    let highlighted = |pixels: &[[u8; 4]]| {
        pixels
            .iter()
            .filter(|pixel| u32::from(pixel[1]) > 180)
            .count()
    };
    assert!(
        highlighted(&selected_pixels) > highlighted(&plain_pixels) + 200,
        "selection does not highlight the outline"
    );
}

fn box_faces(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    object: u32,
    low: [f32; 3],
    high: [f32; 3],
    cap: bool,
) {
    let [x0, y0, z0] = low;
    let [x1, y1, z1] = high;
    let mut faces = vec![
        (
            [[x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]],
            [0.0, 0.0, -1.0],
        ),
        (
            [[x0, y0, z0], [x0, y0, z1], [x1, y0, z1], [x1, y0, z0]],
            [0.0, -1.0, 0.0],
        ),
        (
            [[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]],
            [0.0, 1.0, 0.0],
        ),
        (
            [[x0, y0, z0], [x0, y1, z0], [x0, y1, z1], [x0, y0, z1]],
            [-1.0, 0.0, 0.0],
        ),
        (
            [[x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]],
            [1.0, 0.0, 0.0],
        ),
    ];
    if cap {
        faces.push((
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            [0.0, 0.0, 1.0],
        ));
    }
    for (corners, normal) in faces {
        let base = vertices.len() as u32;
        for corner in corners {
            vertices.push(MeshVertex::new(corner, normal, object));
        }
        indices.extend([0u32, 1, 2, 0, 2, 3].map(|index| base + index));
    }
}

fn box_mesh(cap: bool) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    box_faces(
        &mut vertices,
        &mut indices,
        0,
        [-0.8, -0.8, -0.5],
        [0.8, 0.8, 0.5],
        cap,
    );
    Mesh::new(vertices, indices)
}

fn overlapping_boxes() -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    box_faces(
        &mut vertices,
        &mut indices,
        0,
        [-0.45, -0.4, -0.5],
        [0.05, 0.4, 0.5],
        true,
    );
    box_faces(
        &mut vertices,
        &mut indices,
        1,
        [-0.05, -0.4, -0.5],
        [0.45, 0.4, 0.5],
        true,
    );
    Mesh::new(vertices, indices)
}

#[test]
fn orders_overlapping_cut_faces_by_camera_depth() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let mesh = PackedMesh::new(overlapping_boxes());
    let styles = [
        Style::new([1.0, 0.12, 0.12], true, false, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let selected = [
        Style::new([1.0, 0.12, 0.12], true, true, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let mut positive = Camera::new(35.0);
    positive.fit(Vec3::from(volume.world_extent()), 1.0);
    let mut negative = positive.clone();
    let state = negative.state();
    negative.restore(CameraState {
        yaw: state.yaw + std::f32::consts::PI,
        ..state
    });
    let job = Job {
        texture: &texture,
        volume: &volume,
        volume_region: CORNER,
        section_region: WHOLE,
        focus_z: 0.0,
        mesh: Some((&mesh, &styles)),
    };
    let forward = framing_scene(&device, &queue, job, &positive, None);
    let [red, green, ..] = pixel(&forward, 128, 128).map(u32::from);
    assert!(
        green > red + 20,
        "the nearer object does not own the overlap: red {red} green {green}"
    );
    let backward = framing_scene(&device, &queue, job, &negative, None);
    let [red, green, ..] = pixel(&backward, 128, 128).map(u32::from);
    assert!(
        red > green + 20,
        "orbiting by half a turn does not swap the overlap: red {red} green {green}"
    );
    let highlighted = Job {
        mesh: Some((&mesh, &selected)),
        ..job
    };
    let pixels = framing_scene(&device, &queue, highlighted, &positive, None);
    let [red, green, ..] = pixel(&pixels, 128, 128).map(u32::from);
    assert!(
        green > red + 20,
        "selection breaks the depth order: red {red} green {green}"
    );
}

#[test]
fn shrinks_section_marker_disks_with_the_plane_offset() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let world = Vec3::from(volume.world_extent());
    let voxel = Vec3::new(
        volume.spacing().x(),
        volume.spacing().y(),
        volume.spacing().z(),
    );
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    let mut section = SectionCamera::new(Axis::Z);
    section.fit(world, voxel, 1.0);
    let markers = [Marker::new([0.0, 0.0, 0.0], 0.4, [1.0, 0.1, 0.1], 1.0)];
    let mut counts = [0usize; 3];
    for (slot, focus) in [0.0f32, -0.3, -0.9].into_iter().enumerate() {
        let mut view = camera.clone();
        view.set_focus_along(Axis::Z, focus);
        let pixels = marker_scene(
            &device,
            &queue,
            Job {
                texture: &texture,
                volume: &volume,
                volume_region: CORNER,
                section_region: WHOLE,
                focus_z: 0.0,
                mesh: None,
            },
            &view,
            &section,
            &markers,
        );
        counts[slot] = red_count(&pixels, SECTION_AREA);
    }
    assert!(counts[0] > 1500, "the centred marker disk is missing");
    assert!(
        counts[1] > 500 && counts[1] * 4 < counts[0] * 3,
        "the marker disk does not shrink with the plane offset: {counts:?}"
    );
    assert_eq!(
        counts[2], 0,
        "a marker beyond its radius still marks the plane: {counts:?}"
    );
}

#[test]
fn reuses_the_cut_target_and_batches_every_cut_face() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let mesh = PackedMesh::new(cubes());
    let styles = [
        Style::new([1.0, 0.12, 0.12], true, false, 1.0),
        Style::new([0.12, 1.0, 0.12], true, false, 1.0),
    ];
    let layers = [Layer {
        key: LayerKey::at_frame(1, 0, 0, 1),
        mesh: Arc::new(mesh.clone()),
        style_start: 0,
        style_count: 2,
    }];
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        markers: &[],
    };
    let camera = Camera::new(35.0);
    let section = SectionCamera::new(Axis::Z);
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
    let session = Session {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        scene: &scene,
        volume_region: CORNER,
        section_region: WHOLE,
        region: Region::none(),
        time: 0,
    };
    for sample in 1..=SAMPLES {
        let info = renderer.draw(&device, &queue, common::frame(&session));
        assert_eq!(info.samples, sample, "the cut layer accumulates a sample");
        assert_eq!(info.cut.targets, 1, "the cut target is built once");
        assert_eq!(info.cut.objects, 2, "both objects cross the cut plane");
        assert_eq!(
            info.cut.draws, 3,
            "every cut face costs one draw whatever the object count"
        );
    }
    let settled = renderer.draw(&device, &queue, common::frame(&session));
    assert_eq!(settled.samples, SAMPLES, "accumulation stops at the limit");
    assert_eq!(
        settled.cut.objects, 0,
        "a settled frame draws no cut object"
    );
    assert_eq!(settled.cut.draws, 0, "a settled frame draws no cut face");
    assert_eq!(settled.cut.targets, 1, "the settled frame keeps the target");
}

#[test]
fn cuts_every_object_through_the_parallel_path() {
    let Some((device, queue)) = device() else {
        return;
    };
    let count = 9usize;
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let mesh = PackedMesh::new(many_boxes(count));
    let styles = (0..count)
        .map(|_| Style::new([1.0, 0.12, 0.12], true, false, 1.0))
        .collect::<Vec<_>>();
    let layers = [Layer {
        key: LayerKey::at_frame(1, 0, 0, 1),
        mesh: Arc::new(mesh.clone()),
        style_start: 0,
        style_count: count,
    }];
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        markers: &[],
    };
    let camera = Camera::new(35.0);
    let section = SectionCamera::new(Axis::Z);
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
    let session = Session {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        scene: &scene,
        volume_region: CORNER,
        section_region: WHOLE,
        region: Region::none(),
        time: 0,
    };
    let info = renderer.draw(&device, &queue, common::frame(&session));
    assert_eq!(
        info.cut.objects, count as u32,
        "every object crossing the plane holds a cut face"
    );
}

#[test]
fn reuses_cut_indexes_when_the_axis_returns() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let mesh = PackedMesh::new(many_boxes(9));
    let styles = (0..9)
        .map(|_| Style::new([1.0, 0.12, 0.12], true, false, 1.0))
        .collect::<Vec<_>>();
    let layers = [Layer {
        key: LayerKey::at_frame(1, 0, 0, 1),
        mesh: Arc::new(mesh.clone()),
        style_start: 0,
        style_count: 9,
    }];
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        markers: &[],
    };
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    camera.set_focus_along(Axis::X, -0.12);
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
    let mut cut = |section: &SectionCamera| -> CutInfo {
        let session = Session {
            device: &device,
            queue: &queue,
            target: &view,
            camera: &camera,
            section,
            scene: &scene,
            volume_region: CORNER,
            section_region: WHOLE,
            region: Region::none(),
            time: 0,
        };
        renderer.draw(&device, &queue, common::frame(&session)).cut
    };
    let z_first = cut(&SectionCamera::new(Axis::Z));
    assert!(z_first.objects > 0, "the z cut names every box");
    assert!(z_first.indices > 0, "the z cut indexes its boxes");
    let x_first = cut(&SectionCamera::new(Axis::X));
    assert!(x_first.objects > 0, "the x cut names a box");
    assert!(
        x_first.indices > z_first.indices,
        "a new axis adds its own indexes"
    );
    let z_again = cut(&SectionCamera::new(Axis::Z));
    assert!(
        z_again.indices == x_first.indices,
        "returning to an axis rebuilds its indexes"
    );
}

fn many_boxes(count: usize) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let span = count as f32 * 0.24;
    for object in 0..count {
        let centre = -0.5 * span + 0.24 * object as f32;
        box_faces(
            &mut vertices,
            &mut indices,
            object as u32,
            [centre - 0.08, -0.5, -0.5],
            [centre + 0.08, 0.5, 0.5],
            true,
        );
    }
    Mesh::new(vertices, indices)
}

#[test]
fn exports_section_cut_faces_with_a_transparent_background() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = sphere(Extent::new(128, 128, 64), Spacing::new(0.1, 0.1, 0.2));
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mut section = SectionCamera::new(Axis::Z);
    section.fit(
        Vec3::from(volume.world_extent()),
        Vec3::new(0.1, 0.1, 0.2),
        1.0,
    );
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
    let session = Session {
        device: &device,
        queue: &queue,
        target: &view,
        camera: &camera,
        section: &section,
        scene: &scene,
        volume_region: CORNER,
        section_region: WHOLE,
        region: Region::none(),
        time: 0,
    };
    settle(&mut renderer, session, |request| {
        let grid = BrickGrid::new(volume.extent());
        volume.brick(&grid, request.coord)
    });
    for _ in 0..SAMPLES {
        renderer.draw(&device, &queue, common::frame(&session));
    }
    renderer.set_transparent(true);
    renderer.draw(&device, &queue, common::frame(&session));
    let clear = read_pixels(&device, &queue, &texture, SIDE);
    let signal = pixel(&clear, SIDE / 2, SIDE / 2);
    assert!(
        signal[3] > 200,
        "the section signal carries alpha in a transparent export: {signal:?}"
    );
    let empty = pixel(&clear, 30, 30);
    assert_eq!(
        empty[3], 0,
        "the empty section carries no alpha in a transparent export: {empty:?}"
    );
    renderer.set_transparent(false);
    renderer.draw(&device, &queue, common::frame(&session));
    let opaque = read_pixels(&device, &queue, &texture, SIDE);
    assert_eq!(
        pixel(&opaque, 30, 30)[3],
        255,
        "an opaque export fills the section background"
    );
}

#[test]
fn needs_capped_geometry_to_fill_open_surfaces() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let volume = empty_volume();
    let styles = [Style::new([1.0, 0.12, 0.12], true, false, 1.0)];
    let open = PackedMesh::new(box_mesh(false));
    let capped = PackedMesh::new(box_mesh(true));
    let open_pixels = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: CORNER,
            section_region: WHOLE,
            focus_z: 0.0,
            mesh: Some((&open, &styles)),
        },
    );
    assert_eq!(
        red_count(&open_pixels, SECTION_AREA),
        0,
        "an open surface fills its cut face without a cap"
    );
    let capped_pixels = scene_render(
        &device,
        &queue,
        Job {
            texture: &texture,
            volume: &volume,
            volume_region: CORNER,
            section_region: WHOLE,
            focus_z: 0.0,
            mesh: Some((&capped, &styles)),
        },
    );
    assert!(
        red_count(&capped_pixels, SECTION_AREA) > 500,
        "a capped surface does not fill its cut face"
    );
}

#[test]
fn resolves_volume_picks_at_the_requested_pixel() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let mesh = PackedMesh::new(left_half_plane());
    let styles = [Style::new([1.0, 0.12, 0.12], true, false, 1.0)];
    let layers = [Layer {
        key: LayerKey::at_frame(1, 0, 0, 1),
        mesh: Arc::new(mesh.clone()),
        style_start: 0,
        style_count: 1,
    }];
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        markers: &[],
    };
    let mut camera = Camera::new(35.0);
    camera.restore(CameraState {
        pivot: [0.0, 0.0, 0.0],
        yaw: 0.0,
        pitch: 0.0,
        distance: 5.0,
        fov_y: 35.0f32.to_radians(),
    });
    let section = SectionCamera::new(Axis::Z);
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
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
            region: Region::none(),
            time: 0,
        },
        |request| {
            let grid = BrickGrid::new(volume.extent());
            volume.brick(&grid, request.coord)
        },
    );
    assert_eq!(
        pick(
            &mut renderer,
            &device,
            &queue,
            PickPane::Volume,
            [SIDE / 4, SIDE / 2]
        ),
        PickId::Style(0),
        "the left half plane resolves its style row"
    );
    assert_eq!(
        pick(
            &mut renderer,
            &device,
            &queue,
            PickPane::Volume,
            [SIDE * 3 / 4, SIDE / 2]
        ),
        PickId::None,
        "the empty half resolves no identity"
    );
}

#[test]
fn picks_markers_before_meshes_and_cut_faces() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let mesh = PackedMesh::new(box_mesh(true));
    let styles = [Style::new([1.0, 0.12, 0.12], true, false, 0.5)];
    let layers = [Layer {
        key: LayerKey::at_frame(1, 0, 0, 1),
        mesh: Arc::new(mesh.clone()),
        style_start: 0,
        style_count: 1,
    }];
    let markers = [Marker::new([0.0, 0.0, 0.2], 0.4, [0.2, 0.6, 1.0], 1.0)];
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        markers: &markers,
    };
    let mut camera = Camera::new(35.0);
    camera.restore(CameraState {
        pivot: [0.0, 0.0, 0.0],
        yaw: 0.0,
        pitch: 0.0,
        distance: 5.0,
        fov_y: 35.0f32.to_radians(),
    });
    let mut section = SectionCamera::new(Axis::Z);
    section.fit(
        Vec3::from(volume.world_extent()),
        Vec3::new(0.1, 0.1, 0.1),
        1.0,
    );
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_dataset(&device, &pyramid);
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
            section_region: WHOLE,
            region: Region::none(),
            time: 0,
        },
        |request| {
            let grid = BrickGrid::new(volume.extent());
            volume.brick(&grid, request.coord)
        },
    );
    assert_eq!(
        pick(
            &mut renderer,
            &device,
            &queue,
            PickPane::Volume,
            [SIDE / 2, SIDE / 2]
        ),
        PickId::Marker(0),
        "a marker in front of the box wins the volume pick"
    );
    assert_eq!(
        pick(
            &mut renderer,
            &device,
            &queue,
            PickPane::Section,
            [SIDE / 2, SIDE / 2]
        ),
        PickId::Marker(0),
        "a marker ahead of the section plane wins the section pick"
    );
    assert_eq!(
        pick(
            &mut renderer,
            &device,
            &queue,
            PickPane::Section,
            [SIDE / 2 + 45, SIDE / 2]
        ),
        PickId::Style(0),
        "a cut face resolves its style row beside the marker"
    );
    assert_eq!(
        pick(
            &mut renderer,
            &device,
            &queue,
            PickPane::Section,
            [SIDE / 8, SIDE / 8]
        ),
        PickId::None,
        "the empty section resolves no identity"
    );
}

fn pick(
    renderer: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pane: PickPane,
    point: [u32; 2],
) -> PickId {
    assert!(
        renderer.request_pick(device, queue, pane, point),
        "the pick request lands in its pane"
    );
    for _ in 0..64 {
        if let Some(id) = renderer.poll_pick(device) {
            return id;
        }
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("failed to wait for the gpu");
    }
    panic!("the pick never resolved");
}

fn left_half_plane() -> Mesh {
    let corners = [
        [-1.6f32, -1.6, 0.0],
        [0.0, -1.6, 0.0],
        [0.0, 1.6, 0.0],
        [-1.6, 1.6, 0.0],
    ];
    let vertices = corners
        .into_iter()
        .map(|corner| MeshVertex::new(corner, [0.0, 0.0, 1.0], 0))
        .collect::<Vec<_>>();
    Mesh::new(vertices, vec![0, 1, 2, 0, 2, 3])
}

#[test]
fn antialiases_transparent_silhouettes() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, SIDE);
    let view = texture.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let mesh = PackedMesh::new(rotated_plane(&camera, 0.25, 0.5));
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let render = |visible: bool| {
        let style = Style::new([1.0, 1.0, 1.0], visible, false, 0.35);
        let styles = [style];
        let layers = [Layer {
            key: LayerKey::at_frame(1, 0, 0, 1),
            mesh: Arc::new(mesh.clone()),
            style_start: 0,
            style_count: 1,
        }];
        let scene = SceneFrame {
            channels: &channels,
            shading: Shading::default(),
            step_scale: 1.0,
            styles: &styles,
            layers: &layers,
            markers: &[],
        };
        let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
        renderer.set_dataset(&device, &pyramid);
        let frame = || Frame {
            target: &view,
            size: [SIDE, SIDE],
            volume_region: WHOLE,
            section_region: CORNER,
            time: 0,
            camera: &camera,
            section: &section,
            region: Region::none(),
            clip: Clip::none(),
            exposure: 1.0,
            samples: SAMPLES,
            scene: &scene,
            prefetch: false,
        };
        let mut info = renderer.draw(&device, &queue, frame());
        while info.samples < SAMPLES {
            info = renderer.draw(&device, &queue, frame());
        }
        (info, read_pixels(&device, &queue, &texture, SIDE))
    };
    let (info, shown) = render(true);
    let (_, hidden) = render(false);
    assert!(info.oit.layers >= 1, "the transparent surface keeps layers");
    let centre = pixel(&shown, SIDE / 2, SIDE / 2);
    assert!(
        centre.map(u32::from).iter().sum::<u32>() > 40,
        "the transparent plane is missing: {centre:?}"
    );
    let partial = shown
        .iter()
        .zip(&hidden)
        .filter(|(shown, hidden)| {
            (0..3).any(|channel| shown[channel].abs_diff(centre[channel]) > 6)
                && (0..3).any(|channel| shown[channel].abs_diff(hidden[channel]) > 6)
        })
        .count();
    assert!(
        partial > 20,
        "the silhouette of a transparent object carries no partial pixels: {partial}"
    );
}
