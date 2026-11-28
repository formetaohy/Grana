use std::sync::Arc;

use glam::{Vec2, Vec3, Vec4};
use grana_display::{Camera, Shading, Slice, TransferFunction, VolumeStyle};
use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_render::{
    Frame, Layer, LayerKey, PickId, Renderer, Requests, SceneFrame, SplatChunk, Splats, Style,
};
use grana_volume::{
    BrickCoord, BrickGrid, BrickRequest, Calibration, Channel, ChannelSet, Clip, Damage, Extent,
    Level, PackedBrick, Pyramid, Region, Spacing, Volume, VolumeSeries, VoxelData,
};

use crate::support::{
    Gpu, Hue, Snapshot, budget, gpu, read_pixels, revision, spheres, splat, versions,
};

const SIDE: u32 = 128;
const FRAMES: u32 = 8;
const SAMPLES: u32 = 160;
const BUDGET: u64 = 256 << 20;
const WHOLE: [u32; 4] = [0, 0, SIDE, SIDE];
const EMPTY: Extent = Extent::new(16, 16, 16);

struct View<'a> {
    target: &'a wgpu::TextureView,
    camera: &'a Camera,
    region: Region,
    scene: &'a SceneFrame<'a>,
    viewport: [u32; 4],
    delivery: grana_render::Delivery,
}

fn frame<'a>(view: &View<'a>) -> Frame<'a, 'a> {
    Frame {
        target: view.target,
        size: [SIDE, SIDE],
        camera: view.camera,
        viewport: view.viewport,
        time: 0,
        region: view.region,
        clip: Clip::none(),
        exposure: 1.0,
        scene: view.scene,
        delivery: view.delivery,
    }
}

fn texture(gpu: &Gpu, side: u32) -> wgpu::Texture {
    gpu.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("grana.test.target"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn channels(pyramid: &Pyramid, transfers: &[TransferFunction]) -> Vec<grana_render::ChannelRow> {
    pyramid
        .channels()
        .iter()
        .zip(transfers)
        .map(|(channel, transfer)| grana_render::ChannelRow {
            visible: true,
            opacity: 1.0,
            intensity: channel.intensity(),
            transfer: transfer.clone(),
            volume: VolumeStyle::default(),
        })
        .collect()
}

fn build_renderer(gpu: &Gpu, pyramid: &Pyramid) -> Renderer {
    let mut renderer = Renderer::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        &crate::support::ledger(),
        BUDGET,
        gpu.pipelines(),
    );
    renderer
        .set_dataset(
            gpu.device(),
            pyramid,
            budget(BUDGET, [SIDE, SIDE]),
            [SIDE, SIDE],
        )
        .expect("the test budget holds the volume");
    renderer
}

fn bricked_volume(block: Option<[u32; 3]>) -> Volume {
    let extent = Extent::new(128, 128, 64);
    let spacing = Spacing::new(0.05, 0.05, 0.1);
    let mut values = vec![0u16; extent.voxel_count() as usize];
    let mut second = vec![0u16; extent.voxel_count() as usize];
    let blobs = [
        ([40.0f32, 16.0, 32.0], 18.0f32, &mut values),
        ([88.0f32, 16.0, 32.0], 16.0f32, &mut second),
    ];
    for (centre, radius, target) in blobs {
        for z in 0..extent.z() {
            for y in 0..extent.y() {
                for x in 0..extent.x() {
                    let point = [x as f32, y as f32, z as f32];
                    let distance = (0..3)
                        .map(|axis| (point[axis] - centre[axis]).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    if distance <= radius {
                        let unit = 1.0 - 0.5 * distance / radius;
                        target[extent.index(x, y, z) as usize] = (unit * 60_000.0) as u16;
                    }
                }
            }
        }
    }
    if let Some(coord) = block {
        let grid = BrickGrid::new(extent);
        let cell = BrickCoord::new(coord[0], coord[1], coord[2]);
        let origin = grid.origin(cell);
        let size = grid.extent_of(cell);
        for z in origin[2]..origin[2] + size.z() {
            for y in origin[1]..origin[1] + size.y() {
                for x in origin[0]..origin[0] + size.x() {
                    values[extent.index(x, y, z) as usize] = 60_000;
                }
            }
        }
    }
    Volume::new(
        extent,
        spacing,
        vec![
            Channel::new("first", [0.2, 0.4, 1.0], VoxelData::U16(values)),
            Channel::new("second", [0.2, 1.0, 0.3], VoxelData::U16(second)),
        ],
    )
}

fn sphere_volume() -> Volume {
    let extent = Extent::new(64, 64, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.2);
    spheres(extent, spacing, &[([32.0, 32.0, 16.0], 12.0)], 60_000)
}

fn pack(volume: &Volume, requests: &[BrickRequest]) -> Vec<(BrickRequest, PackedBrick)> {
    let grid = BrickGrid::new(volume.extent());
    let pyramid = volume.pyramid();
    requests
        .iter()
        .map(|request| {
            let data = volume.brick(&grid, request.coord);
            (*request, PackedBrick::of(&data, &pyramid, request.channels))
        })
        .collect()
}

fn settle(gpu: &Gpu, renderer: &mut Renderer, view: &View<'_>, volume: &Volume) -> Requests {
    settle_with(gpu, renderer, || frame(view), volume)
}

fn settle_with<'a, 'b: 'a>(
    gpu: &Gpu,
    renderer: &mut Renderer,
    build: impl Fn() -> Frame<'a, 'b>,
    volume: &Volume,
) -> Requests {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut latest = Requests::default();
    let mut refilled = false;
    loop {
        let info = renderer
            .draw(gpu.device(), gpu.queue(), build())
            .expect("the frame draws");
        if refilled && info.drawn {
            assert_eq!(
                info.age, 1,
                "newly arrived bricks did not restart the temporal accumulation"
            );
            refilled = false;
        }
        if renderer.requests_pending() {
            if let Some(requests) = next_demand(gpu, renderer) {
                let bricks = pack(volume, &requests.volume);
                let generation = renderer.generation();
                renderer.upload(&bricks);
                refilled = renderer.generation() != generation;
                latest = requests;
            }
        } else if info.drawn && info.settled && info.age >= FRAMES && !info.fading {
            return latest;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the stream never settled: age {}, settled {}, pending {}",
            info.age,
            info.settled,
            renderer.requests_pending()
        );
    }
}

fn next_demand(gpu: &Gpu, renderer: &mut Renderer) -> Option<Requests> {
    for _ in 0..4 {
        match renderer
            .poll_requests(gpu.device(), &std::collections::HashSet::new())
            .expect("the demand polls")
        {
            Some(requests) => return Some(requests),
            None => {
                gpu.device()
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("failed to wait for the gpu");
            }
        }
    }
    None
}

fn background(
    gpu: &Gpu,
    renderer: &mut Renderer,
    view: &View<'_>,
    texture: &wgpu::Texture,
    volume: &Volume,
) -> Vec<[u8; 4]> {
    let rows = view
        .scene
        .channels
        .iter()
        .map(|row| grana_render::ChannelRow {
            visible: false,
            opacity: row.opacity,
            intensity: row.intensity,
            transfer: row.transfer.clone(),
            volume: row.volume,
        })
        .collect::<Vec<_>>();
    let scene = SceneFrame {
        versions: versions(),
        channels: &rows,
        shading: view.scene.shading,
        step_scale: view.scene.step_scale,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let blank = View {
        target: view.target,
        camera: view.camera,
        region: view.region,
        scene: &scene,
        viewport: view.viewport,
        delivery: view.delivery,
    };
    settle(gpu, renderer, &blank, volume);
    read_pixels(gpu, texture, [SIDE, SIDE])
}

fn snapshot(
    gpu: &Gpu,
    renderer: &mut Renderer,
    view: &View<'_>,
    texture: &wgpu::Texture,
    volume: &Volume,
) -> Snapshot {
    let reference = background(gpu, renderer, view, texture, volume);
    settle(gpu, renderer, view, volume);
    let image = read_pixels(gpu, texture, [SIDE, SIDE]);
    Snapshot::new([SIDE, SIDE], reference, image)
}

#[test]
fn streams_and_shades_two_channels() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = bricked_volume(None);
    let pyramid = volume.pyramid();
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading {
            ambient: 0.15,
            diffuse: 1.0,
            shadows: 1.0,
            ..Shading::default()
        },
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    assert_eq!(
        renderer.slot_bytes(),
        u64::from(grana_volume::PADDED_SIDE).pow(3) * 4
            + u64::from(grana_volume::PADDED_SIDE / 2 + 2).pow(3) * 4,
        "a two channel slot holds one packed source texel and one half resolution structure texel"
    );
    let view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    let requests = settle_with(&gpu, &mut renderer, || frame(&view), &volume);
    assert!(requests.view.visible > 0, "the view sees no brick");
    assert_eq!(
        requests.view.resident, requests.view.demand,
        "the stream never completed"
    );
    let shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
    assert!(
        shot.painted(shot.all()) > (SIDE * SIDE / 32) as usize,
        "the volume barely rendered: {} painted pixels",
        shot.painted(shot.all())
    );
    assert!(
        shot.painted(shot.middle()) * 2 > shot.painted(shot.all()),
        "the volume lands outside its view"
    );
    assert!(
        shot.hue(shot.all(), Hue::Blue) > 100,
        "the blue channel barely rendered"
    );
    assert!(
        shot.hue(shot.all(), Hue::Green) > 100,
        "the green channel barely rendered"
    );
    settle_with(&gpu, &mut renderer, || frame(&view), &volume);
    let shaded = settled_pixels(&gpu, &mut renderer, &view, &target, SAMPLES);
    let occluder = bricked_volume(Some([1, 1, 0]));
    let region = brick_region(&pyramid, 0, BrickCoord::new(1, 1, 0));
    let damage = Damage::new(region, ChannelSet::of(0)).expect("the damage names a channel");
    renderer.damage(&[damage]);
    let mut nudged = camera.clone();
    nudged.orbit(Vec2::new(9.0, 4.0));
    for held in [&nudged, &camera] {
        let moved = View {
            target: view.target,
            camera: held,
            region: view.region,
            scene: view.scene,
            viewport: view.viewport,
            delivery: view.delivery,
        };
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&moved))
            .expect("the moved frame draws");
    }
    refresh(&gpu, &mut renderer, &view, &occluder);
    settle_with(&gpu, &mut renderer, || frame(&view), &occluder);
    let streamed = settled_pixels(&gpu, &mut renderer, &view, &target, SAMPLES);
    let mut cold = build_renderer(&gpu, &occluder.pyramid());
    settle_with(&gpu, &mut cold, || frame(&view), &occluder);
    let rebuilt = settled_pixels(&gpu, &mut cold, &view, &target, SAMPLES);
    let moved = mean_difference(&shaded, &streamed);
    assert!(
        moved > 2.0,
        "the streamed brick left the shading unchanged: {moved:.3}"
    );
    assert_eq!(
        max_difference(&streamed, &rebuilt),
        0,
        "the streamed brick departs from a full rebuild"
    );
    let repeated = settled_pixels(&gpu, &mut renderer, &view, &target, 2 * SAMPLES);
    let drift = mean_difference(&repeated, &streamed);
    assert!(
        drift < 0.5,
        "the converged frame keeps drifting instead of converging: {drift:.3}"
    );
}

#[test]
fn revisits_a_hollow_frame_without_asking_for_its_bricks_again() {
    let Some(gpu) = gpu() else {
        return;
    };
    let extent = Extent::new(64, 64, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.2);
    let blank = spheres(extent, spacing, &[], 60_000);
    let count = 12u32;
    let frames = (0..count).map(|_| blank.clone()).collect::<Vec<_>>();
    let pyramid = VolumeSeries::new(frames.clone(), 1.0).pyramid().clone();
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let rows = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(frames[0].world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &rows,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    let first = stream_frame(&gpu, &mut renderer, &view, &frames, 0);
    assert!(
        first > 0,
        "the first look at an empty frame reads its bricks"
    );
    for time in 1..count {
        stream_frame(&gpu, &mut renderer, &view, &frames, time);
    }
    let again = stream_frame(&gpu, &mut renderer, &view, &frames, 0);
    assert_eq!(
        again, 0,
        "a hollow frame asked for {again} bricks again when the view returned to it"
    );
}

fn stream_frame(
    gpu: &Gpu,
    renderer: &mut Renderer,
    view: &View<'_>,
    frames: &[Volume],
    time: u32,
) -> usize {
    let mut requested = 0;
    for _ in 0..8 {
        let info = renderer
            .draw(
                gpu.device(),
                gpu.queue(),
                Frame {
                    time,
                    ..frame(view)
                },
            )
            .expect("the frame draws");
        assert!(info.drawn, "the frame draws");
        if let Some(requests) = next_demand(gpu, renderer) {
            requested += requests.volume.len();
            renderer.upload(&pack(&frames[requests.time as usize], &requests.volume));
        }
    }
    requested
}

fn brick_region(pyramid: &Pyramid, level: usize, coord: BrickCoord) -> Region {
    let source = pyramid.level(level);
    let basis = source.basis().matrix();
    let grid = BrickGrid::new(source.extent());
    let origin = grid.origin(coord);
    let extent = grid.extent_of(coord);
    let low = Vec3::from(source.origin())
        + basis * Vec3::new(origin[0] as f32, origin[1] as f32, origin[2] as f32);
    let high = low
        + basis
            * Vec3::new(
                (origin[0] + extent.x()) as f32,
                (origin[1] + extent.y()) as f32,
                (origin[2] + extent.z()) as f32,
            );
    Region::axis_aligned(low, high)
}

fn refresh(gpu: &Gpu, renderer: &mut Renderer, view: &View<'_>, volume: &Volume) {
    for _ in 0..8 {
        renderer
            .draw(gpu.device(), gpu.queue(), frame(view))
            .expect("the damaged frame draws");
        let Some(requests) = next_demand(gpu, renderer) else {
            return;
        };
        let bricks = pack(volume, &requests.volume);
        if bricks.is_empty() {
            return;
        }
        renderer.upload(&bricks);
    }
}

fn settled_pixels(
    gpu: &Gpu,
    renderer: &mut Renderer,
    view: &View<'_>,
    target: &wgpu::Texture,
    samples: u32,
) -> Vec<[u8; 4]> {
    let mut info = renderer
        .draw(gpu.device(), gpu.queue(), frame(view))
        .expect("the converged frame draws");
    for _ in 0..1024 {
        if info.age >= samples {
            break;
        }
        info = renderer
            .draw(gpu.device(), gpu.queue(), frame(view))
            .expect("the converged frame draws");
    }
    assert!(
        info.age >= samples,
        "the accumulation stopped before it held the samples under test"
    );
    assert!(
        info.converged,
        "a frame holding {samples} samples reports no convergence"
    );
    read_pixels(gpu, target, [SIDE, SIDE])
}

fn max_difference(left: &[[u8; 4]], right: &[[u8; 4]]) -> u8 {
    left.iter()
        .zip(right)
        .flat_map(|(left, right)| (0..3).map(move |channel| left[channel].abs_diff(right[channel])))
        .max()
        .unwrap_or(0)
}
fn mean_difference(left: &[[u8; 4]], right: &[[u8; 4]]) -> f32 {
    let total = left
        .iter()
        .zip(right)
        .map(|(left, right)| {
            (0..3)
                .map(|channel| f32::from(left[channel].abs_diff(right[channel])))
                .sum::<f32>()
        })
        .sum::<f32>();
    total / (left.len() * 3) as f32
}

fn placed_quads(quads: &[([f32; 3], f32)]) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, (center, half)) in quads.iter().enumerate() {
        let base = vertices.len() as u32;
        for (across, down) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            vertices.push(MeshVertex::new(
                [
                    center[0] + across * half,
                    center[1] + down * half,
                    center[2],
                ],
                [0.0, 0.0, 1.0],
                object as u32,
            ));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(vertices, indices)
}

fn around(point: [u32; 2], radius: u32) -> [u32; 4] {
    let left = point[0].saturating_sub(radius).min(SIDE - 2 * radius);
    let top = point[1].saturating_sub(radius).min(SIDE - 2 * radius);
    [left, top, left + 2 * radius, top + 2 * radius]
}

#[test]
fn restyles_and_hides_objects_without_rebuilding_the_meshes() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = Volume::new(
        EMPTY,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "empty",
            [0.3, 0.6, 1.0],
            VoxelData::U16(vec![0; EMPTY.voxel_count() as usize]),
        )],
    );
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let centers = [([-3.4, 0.0, 0.0], 1.6), ([3.4, 0.0, 0.0], 1.6)];
    let mesh = Arc::new(PackedMesh::new(placed_quads(&centers)));
    let stamp = revision();
    let styles = Arc::from([
        Style::new([1.0, 0.1, 0.1], true, false, 1.0),
        Style::new([0.1, 0.3, 1.0], true, false, 1.0),
    ]);
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let core = |index: usize| {
        around(
            project(&camera, [centers[index].0[0], centers[index].0[1], 0.0]),
            6,
        )
    };
    let drawn = {
        let layers = [Layer::new(
            LayerKey::new(1, 0, 0),
            stamp,
            stamp,
            Arc::clone(&mesh),
            Arc::clone(&styles),
            0,
        )];
        let scene = SceneFrame {
            versions: grana_render::Versions::new(stamp, stamp, stamp, stamp),
            channels: &channels,
            shading: Shading::default(),
            step_scale: 1.0,
            layers: &layers,
            splats: Splats::empty(),
            overlays: &[],
        };
        let view = View {
            target: &view_texture,
            camera: &camera,
            region: Region::none(),
            scene: &scene,
            viewport: WHOLE,
            delivery: grana_render::Delivery::Immediate,
        };
        settle(&gpu, &mut renderer, &view, &volume);
        read_pixels(&gpu, &target, [SIDE, SIDE])
    };
    let mut changed = |styles: Arc<[Style]>, versions: grana_render::Versions| {
        let layers = [Layer::new(
            LayerKey::new(1, 0, 0),
            stamp,
            versions.appearance(),
            Arc::clone(&mesh),
            Arc::clone(&styles),
            0,
        )];
        let scene = SceneFrame {
            versions,
            channels: &channels,
            shading: Shading::default(),
            step_scale: 1.0,
            layers: &layers,
            splats: Splats::empty(),
            overlays: &[],
        };
        let view = View {
            target: &view_texture,
            camera: &camera,
            region: Region::none(),
            scene: &scene,
            viewport: WHOLE,
            delivery: grana_render::Delivery::Immediate,
        };
        let info = renderer
            .draw(gpu.device(), gpu.queue(), frame(&view))
            .expect("the restyled frame draws");
        (info, read_pixels(&gpu, &target, [SIDE, SIDE]))
    };
    let selected = Arc::from([
        Style::new([1.0, 0.1, 0.1], true, true, 1.0),
        Style::new([0.1, 0.3, 1.0], true, false, 1.0),
    ]);
    let (restyled, tinted) = changed(
        selected,
        grana_render::Versions::new(stamp, stamp, stamp.wrapping_add(1), stamp.wrapping_add(3)),
    );
    assert!(!restyled.rebuilt, "a style change rebuilt the mesh passes");
    assert!(
        !restyled.history,
        "a style change kept the converged history"
    );
    assert_eq!(
        restyled.age, 1,
        "a style change did not restart the temporal accumulation"
    );
    assert!(
        rect_difference(&drawn, &tinted, core(0)) > 10.0,
        "the selected object kept its style"
    );
    let hidden = Arc::from([
        Style::new([1.0, 0.1, 0.1], true, false, 1.0),
        Style::new([0.1, 0.3, 1.0], false, false, 1.0),
    ]);
    let (staged, cleared) = changed(
        hidden,
        grana_render::Versions::new(
            stamp,
            stamp.wrapping_add(1),
            stamp.wrapping_add(2),
            stamp.wrapping_add(3),
        ),
    );
    assert!(
        !staged.rebuilt,
        "a visibility change rebuilt the mesh passes"
    );
    assert!(
        !staged.history,
        "a visibility change kept the converged history"
    );
    assert!(
        rect_difference(&tinted, &cleared, core(1)) > 10.0,
        "the hidden object stayed drawn"
    );
}

fn rect_difference(left: &[[u8; 4]], right: &[[u8; 4]], rect: [u32; 4]) -> f32 {
    let mut total = 0i64;
    let mut count = 0i64;
    for y in rect[1]..rect[3] {
        for x in rect[0]..rect[2] {
            let index = (y * SIDE + x) as usize;
            total += (0..3)
                .map(|channel| {
                    (i32::from(left[index][channel]) - i32::from(right[index][channel])).abs()
                        as i64
                })
                .max()
                .unwrap_or(0);
            count += 1;
        }
    }
    total as f32 / count.max(1) as f32
}

#[test]
fn composites_a_mesh_layer() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = Volume::new(
        EMPTY,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "empty",
            [0.3, 0.6, 1.0],
            VoxelData::U16(vec![0; EMPTY.voxel_count() as usize]),
        )],
    );
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let centers = [
        ([-3.4, 0.0, 0.0], 1.6),
        ([3.4, 0.0, 0.0], 1.6),
        ([0.0, 3.4, 0.0], 1.6),
    ];
    let mesh = PackedMesh::new(placed_quads(&centers));
    let styles = Arc::from([
        Style::new([1.0, 0.1, 0.1], true, false, 1.0),
        Style::new([0.1, 0.3, 1.0], true, false, 0.5),
        Style::new([0.1, 1.0, 0.1], false, false, 1.0),
    ]);
    let layers = [Layer::new(
        LayerKey::new(1, 0, 0),
        1,
        1,
        Arc::new(mesh),
        Arc::clone(&styles),
        0,
    )];
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        layers: &layers,
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    let shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
    let opaque = around(project(&camera, [centers[0].0[0], centers[0].0[1], 0.0]), 7);
    let translucent = around(project(&camera, [centers[1].0[0], centers[1].0[1], 0.0]), 7);
    let hidden = project(&camera, [centers[2].0[0], centers[2].0[1], 0.0]);
    assert!(
        shot.hue(opaque, Hue::Red) > 100,
        "the opaque object barely rendered: {} red pixels",
        shot.hue(opaque, Hue::Red)
    );
    assert!(
        shot.coverage(opaque) > 0.6,
        "the opaque object does not cover its place: {:.2}",
        shot.coverage(opaque)
    );
    assert!(
        shot.hue(translucent, Hue::Blue) > 100,
        "the translucent object faded: {} blue pixels",
        shot.hue(translucent, Hue::Blue)
    );
    assert!(
        shot.coverage(translucent) > 0.5,
        "the translucent object barely covers its centre: {:.2}",
        shot.coverage(translucent)
    );
    assert!(
        shot.contrast(translucent) > 10.0,
        "the translucent object is invisible against the background: {:.1}",
        shot.contrast(translucent)
    );
    assert_eq!(
        shot.hue(WHOLE, Hue::Green),
        0,
        "an invisible object painted its style"
    );
    assert_eq!(
        shot.painted([0, 0, SIDE / 8, SIDE / 8]),
        0,
        "the mesh spilled into the corner"
    );
    assert!(renderer.request_pick(
        gpu.device(),
        gpu.queue(),
        project(&camera, [centers[0].0[0], centers[0].0[1], 0.0])
    ));
    assert_eq!(picked(&gpu, &mut renderer), PickId::Style(0));
    assert!(renderer.request_pick(
        gpu.device(),
        gpu.queue(),
        project(&camera, [centers[1].0[0], centers[1].0[1], 0.0])
    ));
    assert_eq!(picked(&gpu, &mut renderer), PickId::Style(1));
    assert!(renderer.request_pick(gpu.device(), gpu.queue(), hidden));
    assert_eq!(picked(&gpu, &mut renderer), PickId::None);
}

fn empty_channel(pyramid: &Pyramid, visible: bool) -> Vec<grana_render::ChannelRow> {
    pyramid
        .channels()
        .iter()
        .map(|channel| grana_render::ChannelRow {
            visible,
            opacity: 1.0,
            intensity: channel.intensity(),
            transfer: TransferFunction::tinted(channel.tint()),
            volume: VolumeStyle::default(),
        })
        .collect()
}

fn shadow_samples(
    gpu: &Gpu,
    volume: &Volume,
    camera: &Camera,
    mesh: Arc<PackedMesh>,
    style: [Style; 2],
    shading: Shading,
) -> Vec<[u8; 4]> {
    let pyramid = volume.pyramid();
    let rows = empty_channel(&pyramid, false);
    let layers = [Layer::new(
        LayerKey::new(1, 0, 0),
        1,
        1,
        Arc::clone(&mesh),
        Arc::from(style),
        0,
    )];
    let scene = SceneFrame {
        versions: versions(),
        channels: &rows,
        shading,
        step_scale: 1.0,
        layers: &layers,
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(gpu, &pyramid);
    let target = texture(gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    settle(gpu, &mut renderer, &view, volume);
    read_pixels(gpu, &target, [SIDE, SIDE])
}

fn brightness(pixels: &[[u8; 4]], at: [u32; 2]) -> f32 {
    let pixel = pixels[(at[1] * SIDE + at[0]) as usize];
    (0..3).map(|channel| f32::from(pixel[channel])).sum::<f32>() / 3.0
}

#[test]
fn casts_mesh_shadows_across_a_coarse_volume() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = Volume::new(
        EMPTY,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "empty",
            [0.3, 0.6, 1.0],
            VoxelData::U16(vec![0; EMPTY.voxel_count() as usize]),
        )],
    );
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let placed = [([0.0, 0.0, -1.0], 6.0), ([0.0, 0.0, 1.0], 1.5)];
    let mesh = Arc::new(PackedMesh::new(placed_quads(&placed)));
    let lit = Shading {
        shadows: 1.0,
        azimuth: 40.0,
        elevation: 50.0,
        ..Shading::default()
    };
    let receiver = Style::new([0.9, 0.9, 0.9], true, false, 1.0);
    let caster = Style::new([1.0, 0.1, 0.1], true, false, 1.0);
    let light = lit.light();
    let cast = [-2.0 * light.x / light.z, -2.0 * light.y / light.z, -1.0];
    let style = [receiver, caster];
    let drawn = shadow_samples(&gpu, &volume, &camera, Arc::clone(&mesh), style, lit);
    let off = shadow_samples(
        &gpu,
        &volume,
        &camera,
        Arc::clone(&mesh),
        style,
        Shading {
            shadows: 0.0,
            ..lit
        },
    );
    let hidden = shadow_samples(
        &gpu,
        &volume,
        &camera,
        mesh,
        [receiver, Style::new([1.0, 0.1, 0.1], false, false, 1.0)],
        lit,
    );
    assert!(
        max_difference(&drawn, &off) > 24,
        "the key light casts no mesh shadow: {}",
        max_difference(&drawn, &off)
    );
    let under = project(&camera, cast);
    assert!(
        brightness(&hidden, under) - brightness(&drawn, under) > 16.0,
        "hiding the caster left the receiver as dark as it was: {:.1} against {:.1}",
        brightness(&hidden, under),
        brightness(&drawn, under)
    );
    let clear = project(&camera, [-cast[0], -cast[1], -1.0]);
    assert!(
        (brightness(&drawn, clear) - brightness(&off, clear)).abs() < 8.0,
        "the shadow spilled away from the caster: {:.1} against {:.1}",
        brightness(&drawn, clear),
        brightness(&off, clear)
    );
}

fn far_pair_volume() -> Volume {
    let extent = Extent::new(96, 48, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.1);
    let left = spheres(extent, spacing, &[([20.0, 24.0, 16.0], 6.0)], 60_000);
    let right = spheres(extent, spacing, &[([76.0, 24.0, 16.0], 6.0)], 60_000);
    Volume::new(
        extent,
        spacing,
        vec![
            Channel::new("left", [0.2, 0.4, 1.0], left.channels()[0].data().clone()),
            Channel::new("right", [0.2, 1.0, 0.3], right.channels()[0].data().clone()),
        ],
    )
}

#[test]
fn clips_an_oriented_region() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = far_pair_volume();
    let pyramid = volume.pyramid();
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let open = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    let shot = snapshot(&gpu, &mut renderer, &open, &target, &volume);
    assert!(
        shot.hue(shot.left(), Hue::Blue) > shot.hue(shot.right(), Hue::Blue),
        "the left ball does not land on the left"
    );
    assert!(
        shot.hue(shot.right(), Hue::Green) > shot.hue(shot.left(), Hue::Green),
        "the right ball does not land on the right"
    );
    let across = Vec3::new(1.0, 0.0, 2.0).normalize();
    let down = Vec3::new(-2.0, 0.0, 1.0).normalize();
    let region = Region::new(
        -1.1 * across,
        [across, Vec3::Y, down],
        Vec3::new(1.1, 2.4, 4.8),
    );
    let clipped = View { region, ..open };
    let shot = snapshot(&gpu, &mut renderer, &clipped, &target, &volume);
    assert!(
        shot.hue(shot.left(), Hue::Blue) > 50,
        "the oriented region hid the kept ball: {} pixels",
        shot.hue(shot.left(), Hue::Blue)
    );
    assert!(
        shot.hue(shot.right(), Hue::Green) < 20,
        "the oriented region kept the dropped ball: {} pixels",
        shot.hue(shot.right(), Hue::Green)
    );
}

fn assure(
    gpu: &Gpu,
    renderer: &mut Renderer,
    view: &View<'_>,
    volume: &Volume,
) -> grana_render::FrameInfo {
    let mut info = renderer
        .draw(gpu.device(), gpu.queue(), frame(view))
        .expect("the frame draws");
    for _ in 0..512 {
        if info.drawn && info.settled {
            assert!(
                !info.truncated,
                "a complete assured frame lost bricks to the budget"
            );
            return info;
        }
        if let Some(requests) = next_demand(gpu, renderer) {
            let bricks = pack(volume, &requests.volume);
            renderer.upload(&bricks);
        }
        info = renderer
            .draw(gpu.device(), gpu.queue(), frame(view))
            .expect("the frame draws");
    }
    panic!("the assured frame never settled");
}

fn assured_pixels(
    gpu: &Gpu,
    pyramid: &Pyramid,
    volume: &Volume,
    channels: &[grana_render::ChannelRow],
    camera: &Camera,
) -> Vec<[u8; 4]> {
    let scene = SceneFrame {
        versions: versions(),
        channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(gpu, pyramid);
    let target = texture(gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Assured,
    };
    for _ in 0..4 {
        let info = renderer
            .draw(gpu.device(), gpu.queue(), frame(&view))
            .expect("the frame draws");
        assert!(
            !info.drawn,
            "an assured frame drew before its bricks arrived"
        );
    }
    let info = assure(gpu, &mut renderer, &view, volume);
    assert!(info.drawn, "the assured frame stopped short");
    read_pixels(gpu, &target, [SIDE, SIDE])
}

#[test]
fn assures_complete_reproducible_frames() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let first = assured_pixels(&gpu, &pyramid, &volume, &channels, &camera);
    let second = assured_pixels(&gpu, &pyramid, &volume, &channels, &camera);
    assert_eq!(first, second, "two assured exports of one pose diverge");
    let lit = first
        .iter()
        .filter(|pixel| pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32 > 24)
        .count();
    assert!(
        lit > (SIDE * SIDE / 32) as usize,
        "the assured frame is empty"
    );
}

fn layered_volume(near: u16, far: u16) -> Volume {
    let extent = Extent::new(32, 32, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.1);
    let mut values = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let value = if (22..26).contains(&z) {
                    near
                } else if (6..10).contains(&z) {
                    far
                } else {
                    0
                };
                values[extent.index(x, y, z) as usize] = value;
            }
        }
    }
    Volume::new(
        extent,
        spacing,
        vec![Channel::new(
            "layers",
            [0.3, 0.6, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn gradient_volume() -> Volume {
    let extent = Extent::new(64, 64, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.2);
    let centre = [32.0f32, 32.0, 16.0];
    let radius = 12.0f32;
    let mut values = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let point = [x as f32, y as f32, z as f32];
                let distance = (0..3)
                    .map(|axis| (point[axis] - centre[axis]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                if distance <= radius {
                    let unit = 1.0 - distance / radius;
                    values[extent.index(x, y, z) as usize] = (unit * 60_000.0) as u16;
                }
            }
        }
    }
    Volume::new(
        extent,
        spacing,
        vec![Channel::new(
            "gradient",
            [0.3, 0.6, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn axis_camera(volume: &Volume) -> Camera {
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mut state = camera.state();
    state.yaw = 0.0;
    state.pitch = 0.0;
    state.aperture = 0.0;
    state.projection = grana_display::CameraProjection::Orthographic;
    camera.restore(state);
    camera
}

fn render_volume(
    gpu: &Gpu,
    volume: &Volume,
    style: VolumeStyle,
    camera: &Camera,
    visible: bool,
) -> Vec<[u8; 4]> {
    let transfer = TransferFunction::tinted(volume.pyramid().channels()[0].tint());
    render_volume_with(gpu, volume, style, camera, transfer, visible)
}

fn render_volume_with(
    gpu: &Gpu,
    volume: &Volume,
    style: VolumeStyle,
    camera: &Camera,
    transfer: TransferFunction,
    visible: bool,
) -> Vec<[u8; 4]> {
    let pyramid = volume.pyramid();
    let channels = vec![grana_render::ChannelRow {
        visible,
        opacity: 1.0,
        intensity: pyramid.channels()[0].intensity(),
        transfer,
        volume: style,
    }];
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(gpu, &pyramid);
    let target = texture(gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Assured,
    };
    settle(gpu, &mut renderer, &view, volume);
    read_pixels(gpu, &target, [SIDE, SIDE])
}

#[test]
fn an_indexed_channel_paints_every_index_but_its_background() {
    let Some(gpu) = gpu() else {
        return;
    };
    let extent = Extent::new(64, 64, 16);
    let mut values = vec![5u8; extent.voxel_count() as usize];
    for z in 4..12u32 {
        for y in 24..40u32 {
            for x in 24..40u32 {
                values[extent.index(x, y, z) as usize] = 200;
            }
        }
    }
    let colors = (0..256u32)
        .map(|index| {
            [
                (index % 5) as f32 / 4.0,
                (index % 3) as f32 / 2.0,
                (index % 7) as f32 / 6.0,
            ]
        })
        .collect::<Vec<_>>();
    let table = grana_volume::ColorTable::new(colors.clone());
    let volume = Volume::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        vec![
            Channel::new("indexed", [0.3, 0.6, 1.0], VoxelData::U8(values)).with_color_table(table),
        ],
    );
    let camera = axis_camera(&volume);
    let transfer = TransferFunction::indexed(colors);
    let style = VolumeStyle::new(grana_display::VolumeMode::Blend, 0.5);
    let background = render_volume_with(&gpu, &volume, style, &camera, transfer.clone(), false);
    let image = render_volume_with(&gpu, &volume, style, &camera, transfer, true);
    assert_eq!(
        rect_difference(&image, &background, [4, 4, 20, 20]),
        0.0,
        "the background index painted the empty corner of its tile"
    );
    assert!(
        rect_difference(&image, &background, [50, 50, 78, 78]) > 10.0,
        "the labelled blob never rendered"
    );
    assert!(
        painted(&image, &background) * 8 < (SIDE * SIDE) as usize,
        "the background index was painted as a solid tile of volume"
    );
}

fn difference(left: &[[u8; 4]], right: &[[u8; 4]]) -> f32 {
    let total = left
        .iter()
        .zip(right)
        .map(|(a, b)| {
            (0..3)
                .map(|channel| (i32::from(a[channel]) - i32::from(b[channel])).abs())
                .max()
                .unwrap_or(0)
        })
        .sum::<i32>();
    total as f32 / left.len() as f32
}

fn painted(pixels: &[[u8; 4]], reference: &[[u8; 4]]) -> usize {
    pixels
        .iter()
        .zip(reference)
        .filter(|(pixel, background)| {
            (0..3).any(|channel| {
                (i32::from(pixel[channel]) - i32::from(background[channel])).abs() >= 8
            })
        })
        .count()
}

#[test]
fn an_indexed_transfer_holds_its_entries_at_the_texels_their_indices_sample() {
    let colors = (0..256u32)
        .map(|index| {
            [
                (index % 7) as f32 / 6.0,
                (index % 11) as f32 / 10.0,
                (index % 13) as f32 / 12.0,
            ]
        })
        .collect::<Vec<_>>();
    let table = grana_volume::ColorTable::new(colors.clone());
    let transfer = TransferFunction::indexed(table.colors());
    let lut = transfer.lut(1024);
    let domain = table.indices();
    for (index, color) in colors.iter().enumerate() {
        let position = (index as f32 - domain.min()) / domain.span();
        let held = sample_lut(&lut, position);
        let expected = grana_display::srgb_to_linear(*color);
        for channel in 0..3 {
            assert!(
                (held[channel] - expected[channel]).abs() < 1.0e-3,
                "entry {index} channel {channel} reads {} against {}",
                held[channel],
                expected[channel]
            );
        }
        assert_eq!(
            held[3] > 0.5,
            index > 0,
            "only the entry an unlabelled voxel reads stays transparent"
        );
    }
}

fn sample_lut(lut: &[[f32; 4]], position: f32) -> [f32; 4] {
    let x = position * lut.len() as f32 - 0.5;
    let lower = (x.floor().max(0.0) as usize).min(lut.len() - 1);
    let upper = (lower + 1).min(lut.len() - 1);
    let fraction = (x - lower as f32).clamp(0.0, 1.0);
    std::array::from_fn(|component| {
        lut[lower][component] + (lut[upper][component] - lut[lower][component]) * fraction
    })
}

#[test]
fn projects_the_maximum_and_surfaces_the_isovalue() {
    let Some(gpu) = gpu() else {
        return;
    };
    let near_bright = layered_volume(60_000, 20_000);
    let far_bright = layered_volume(20_000, 60_000);
    let camera = axis_camera(&near_bright);
    let maximum = VolumeStyle::new(grana_display::VolumeMode::Maximum, 0.5);
    let blend = VolumeStyle::new(grana_display::VolumeMode::Blend, 0.5);
    let near_maximum = render_volume(&gpu, &near_bright, maximum, &camera, true);
    let far_maximum = render_volume(&gpu, &far_bright, maximum, &camera, true);
    let projected = difference(&near_maximum, &far_maximum);
    let differing = near_maximum
        .iter()
        .zip(&far_maximum)
        .filter(|(near, far)| near != far)
        .count();
    let peak = near_maximum
        .iter()
        .zip(&far_maximum)
        .flat_map(|(near, far)| {
            (0..3).map(move |channel| i32::from(near[channel]) - i32::from(far[channel]))
        })
        .map(i32::abs)
        .max()
        .unwrap_or(0);
    assert!(
        projected < 1.0,
        "the maximum projection follows depth: {projected} ({differing} pixels differ, peak {peak})"
    );
    let near_blend = render_volume(&gpu, &near_bright, blend, &camera, true);
    let far_blend = render_volume(&gpu, &far_bright, blend, &camera, true);
    assert!(
        difference(&near_blend, &far_blend) > 1.0,
        "blending ignores depth"
    );
    let gradient = gradient_volume();
    let mut gradient_camera = Camera::new(35.0);
    gradient_camera.fit(Vec3::from(gradient.world_extent()), 1.0);
    let surface = grana_display::VolumeMode::Isosurface;
    let background = render_volume(&gpu, &gradient, blend, &gradient_camera, false);
    let coarse = painted(
        &render_volume(
            &gpu,
            &gradient,
            VolumeStyle::new(surface, 0.25),
            &gradient_camera,
            true,
        ),
        &background,
    );
    let fine = painted(
        &render_volume(
            &gpu,
            &gradient,
            VolumeStyle::new(surface, 0.75),
            &gradient_camera,
            true,
        ),
        &background,
    );
    assert!(coarse > 0, "the isosurface is empty");
    assert!(
        coarse > 4 * fine,
        "the isovalue does not shrink the surface: {coarse} and {fine}"
    );
}

fn ladder_volumes() -> Vec<Volume> {
    [
        (Extent::new(128, 128, 64), Spacing::new(1.0, 1.0, 1.0), 12.0),
        (Extent::new(64, 64, 32), Spacing::new(2.0, 2.0, 2.0), 6.0),
        (Extent::new(32, 32, 16), Spacing::new(4.0, 4.0, 4.0), 3.0),
    ]
    .into_iter()
    .map(|(extent, spacing, radius)| {
        let centre = [
            0.5 * extent.x() as f32,
            0.5 * extent.y() as f32,
            0.5 * extent.z() as f32,
        ];
        spheres(extent, spacing, &[(centre, radius)], 60_000)
    })
    .collect()
}

fn ladder_pyramid(volumes: &[Volume]) -> Pyramid {
    let base = volumes[0].pyramid();
    let fine = base.level(0);
    Pyramid::placed(
        vec![fine, fine.coarser(), fine.coarser().coarser()],
        volumes.len(),
        base.channels().to_vec(),
        1,
        1.0,
        [0.0; 3],
        base.calibration(),
    )
}

fn stream_levels(
    gpu: &Gpu,
    renderer: &mut Renderer,
    view: &View<'_>,
    volumes: &[Volume],
) -> grana_render::FrameInfo {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        let info = renderer
            .draw(gpu.device(), gpu.queue(), frame(view))
            .expect("the frame draws");
        if renderer.requests_pending() {
            if let Some(requests) = next_demand(gpu, renderer) {
                let bricks = requests
                    .volume
                    .iter()
                    .map(|request| {
                        let volume = &volumes[request.level];
                        let grid = BrickGrid::new(volume.extent());
                        let data = volume.brick(&grid, request.coord);
                        (
                            *request,
                            PackedBrick::of(&data, &volume.pyramid(), request.channels),
                        )
                    })
                    .collect::<Vec<_>>();
                renderer.upload(&bricks);
            }
        } else if info.drawn && info.settled && info.age >= FRAMES && !info.fading {
            return info;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the stream never settled: age {}, settled {}, pending {}",
            info.age,
            info.settled,
            renderer.requests_pending()
        );
    }
}

fn ladder_view<'a>(
    target: &'a wgpu::TextureView,
    camera: &'a Camera,
    scene: &'a SceneFrame<'a>,
) -> View<'a> {
    View {
        target,
        camera,
        region: Region::none(),
        scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    }
}

#[test]
fn fits_the_volume_level_to_the_residency_budget() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volumes = ladder_volumes();
    let pyramid = ladder_pyramid(&volumes);
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(pyramid.level(0).world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let generous = {
        let mut renderer = Renderer::new(
            gpu.device(),
            gpu.queue(),
            wgpu::TextureFormat::Rgba8Unorm,
            &crate::support::ledger(),
            128 << 20,
            gpu.pipelines(),
        );
        renderer
            .set_dataset(
                gpu.device(),
                &pyramid,
                budget(128 << 20, [SIDE, SIDE]),
                [SIDE, SIDE],
            )
            .expect("the test budget holds the volume");
        let target = texture(&gpu, SIDE);
        let view_texture = target.create_view(&Default::default());
        let view = ladder_view(&view_texture, &camera, &scene);
        stream_levels(&gpu, &mut renderer, &view, &volumes)
    };
    assert!(
        generous.drawn,
        "the unconstrained view never resolved a frame"
    );
    assert_eq!(
        generous.view.level, 0,
        "a static camera stayed at the coarse level {}",
        generous.view.level
    );
    let mut renderer = Renderer::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        &crate::support::ledger(),
        45 << 20,
        gpu.pipelines(),
    );
    renderer
        .set_dataset(
            gpu.device(),
            &pyramid,
            budget(45 << 20, [SIDE, SIDE]),
            [SIDE, SIDE],
        )
        .expect("the test budget holds the volume");
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = ladder_view(&view_texture, &camera, &scene);
    let info = stream_levels(&gpu, &mut renderer, &view, &volumes);
    assert!(info.drawn, "the constrained view never resolved");
    assert!(
        info.view.level > 0,
        "the budget still asks for the finest level"
    );
    assert_eq!(
        info.view.visible, info.view.demand,
        "the fitting level still truncates its bricks"
    );
    assert!(!info.truncated, "the fitting level reports truncation");
    holds_a_settled_frame(&gpu, &mut renderer, &view);
}

fn holds_a_settled_frame(gpu: &Gpu, renderer: &mut Renderer, view: &View<'_>) {
    for _ in 0..4 {
        let info = renderer
            .draw(gpu.device(), gpu.queue(), frame(view))
            .expect("the frame draws");
        assert!(info.settled, "an idle fitted frame replans");
        assert!(info.drawn, "an idle fitted frame draws");
        assert!(!info.truncated, "an idle fitted frame truncates");
    }
}
fn slice_camera(pyramid: &Pyramid, index: u32) -> Camera {
    let level = pyramid.level(0);
    let frame = level.slice_frame(2);
    let mut camera = Camera::new(35.0);
    camera.set_slice(Slice::new(2, frame.right(), frame.up(), frame.step()));
    camera.fit_slice(
        Vec3::from(level.slice_center(2, index)),
        frame.half_width(),
        frame.half_height(),
        1.0,
    );
    camera
}

#[test]
fn samples_one_unlit_slice_and_rejects_other_planes() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let target = texture(&gpu, SIDE);
    let view = target.create_view(&Default::default());
    let mut renderer = build_renderer(&gpu, &pyramid);
    let camera = slice_camera(&pyramid, 16);
    let taken = snapshot(
        &gpu,
        &mut renderer,
        &View {
            target: &view,
            camera: &camera,
            region: Region::none(),
            scene: &scene,
            viewport: WHOLE,
            delivery: grana_render::Delivery::Assured,
        },
        &target,
        &volume,
    );
    let coverage = taken.coverage(WHOLE);
    assert!(
        (0.04..0.2).contains(&coverage),
        "the slice covers {coverage} of the target"
    );
    let centre = [SIDE / 2 - 2, SIDE / 2 - 2, SIDE / 2 + 2, SIDE / 2 + 2];
    assert_eq!(
        taken.painted(centre),
        16,
        "the slice misses the middle of the sphere"
    );
    assert_eq!(
        taken.painted([2, 2, 12, 12]),
        0,
        "the slice paints outside the sphere"
    );
    let empty = slice_camera(&pyramid, 0);
    let blank = snapshot(
        &gpu,
        &mut renderer,
        &View {
            target: &view,
            camera: &empty,
            region: Region::none(),
            scene: &scene,
            viewport: WHOLE,
            delivery: grana_render::Delivery::Assured,
        },
        &target,
        &volume,
    );
    assert_eq!(
        blank.painted(WHOLE),
        0,
        "a plane away from the sphere samples it anyway"
    );

    let plane = pyramid.level(0).slice_center(2, 16);
    let mut vertices = Vec::new();
    for (across, down, through) in [
        (-1.0f32, -1.0, -1.0),
        (1.0, -1.0, -1.0),
        (1.0, 1.0, -1.0),
        (-1.0, 1.0, -1.0),
        (-1.0, -1.0, 1.0),
        (1.0, -1.0, 1.0),
        (1.0, 1.0, 1.0),
        (-1.0, 1.0, 1.0),
    ] {
        vertices.push(MeshVertex::new(
            [
                plane[0] + 2.6 + across * 0.3,
                plane[1] - 2.6 + down * 0.3,
                plane[2] + through * 0.5,
            ],
            [0.0, 0.0, 1.0],
            0,
        ));
    }
    let indices = vec![
        0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6,
        1, 3, 5, 3, 7, 5,
    ];
    let styles: Arc<[Style]> = Arc::from([Style::new([1.0, 0.1, 0.1], true, false, 1.0)]);
    let layers = [Layer::new(
        LayerKey::new(1, 0, 0),
        1,
        1,
        Arc::new(PackedMesh::new(Mesh::new(vertices, indices))),
        styles,
        0,
    )];
    let sectioned = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &layers,
        splats: Splats::empty(),
        overlays: &[],
    };
    let target_section = [plane[0] + 2.6, plane[1] - 2.6, plane[2]];
    let section = snapshot(
        &gpu,
        &mut renderer,
        &View {
            target: &view,
            camera: &camera,
            region: Region::none(),
            scene: &sectioned,
            viewport: WHOLE,
            delivery: grana_render::Delivery::Assured,
        },
        &target,
        &volume,
    );
    assert!(
        section.hue(around(project(&camera, target_section), 6), Hue::Red) > 20,
        "the slice draws the cross-section of a surface that crosses it"
    );
}
fn project(camera: &Camera, point: [f32; 3]) -> [u32; 2] {
    let clip = camera.view_projection(1.0, 1.0) * Vec4::new(point[0], point[1], point[2], 1.0);
    let x = (clip.x / clip.w * 0.5 + 0.5) * SIDE as f32;
    let y = (0.5 - clip.y / clip.w * 0.5) * SIDE as f32;
    [x.floor() as u32, y.floor() as u32]
}

fn picked(gpu: &Gpu, renderer: &mut Renderer) -> PickId {
    for _ in 0..64 {
        gpu.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("failed to wait for the gpu");
        if let Some(picked) = renderer.poll_pick(gpu.device()).expect("the pick polls") {
            return picked;
        }
    }
    panic!("the pick never resolved");
}

#[test]
fn switches_a_splat_level_without_rebuilding_the_volume_passes() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let markers = [
        splat([0.0, 0.0, 0.1], 0.3, [1.0, 0.2, 0.2], 1.0),
        splat([0.0, 0.5, 0.1], 0.3, [0.2, 0.2, 1.0], 1.0),
    ];
    let stamp = revision();
    let coarse = [SplatChunk::new(
        7,
        1,
        true,
        grana_render::SplatLevel::of(stamp, vec![markers[0]]),
    )];
    let fine = [SplatChunk::new(
        7,
        3,
        true,
        grana_render::SplatLevel::of(stamp.wrapping_add(1), vec![markers[1]]),
    )];
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let camera = slice_camera(&pyramid, 16);
    let drawn = |renderer: &mut Renderer, chunks: &[SplatChunk], splats: u64, point: [f32; 3]| {
        let scene = SceneFrame {
            versions: grana_render::Versions::new(stamp, stamp, stamp, splats),
            channels: &channels,
            shading: Shading::default(),
            step_scale: 0.5,
            layers: &[],
            splats: Splats::new(chunks),
            overlays: &[],
        };
        let view = View {
            target: &view_texture,
            camera: &camera,
            region: Region::none(),
            scene: &scene,
            viewport: WHOLE,
            delivery: grana_render::Delivery::Immediate,
        };
        settle(&gpu, renderer, &view, &volume);
        assert!(renderer.request_pick(gpu.device(), gpu.queue(), project(&camera, point)));
        let picked = picked(&gpu, renderer);
        let info = renderer
            .draw(gpu.device(), gpu.queue(), frame(&view))
            .expect("a splat level draws");
        (info, picked)
    };
    let (first, coarse_pick) = drawn(&mut renderer, &coarse, stamp, [0.0, 0.0, 0.1]);
    assert_eq!(
        coarse_pick,
        PickId::Splat {
            key: 7,
            level: 1,
            index: 0
        },
        "the coarse level holds the marker the pick lands on"
    );
    let (second, fine_pick) = drawn(&mut renderer, &fine, stamp.wrapping_add(1), [0.0, 0.5, 0.1]);
    assert!(
        !first.rebuilt && !second.rebuilt,
        "switching a splat level rebuilt the volume and mesh passes"
    );
    assert_eq!(
        fine_pick,
        PickId::Splat {
            key: 7,
            level: 3,
            index: 0
        },
        "the finer level replaced the coarse one"
    );
    assert_eq!(
        drawn(&mut renderer, &coarse, stamp, [0.0, 0.0, 0.1]).1,
        PickId::Splat {
            key: 7,
            level: 1,
            index: 0
        },
        "switching back to the coarse level uploads it again"
    );
}

#[test]
fn picks_only_the_objects_of_the_slice_plane() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let markers = [
        splat([0.0, 0.0, 0.1], 0.3, [1.0, 0.2, 0.2], 1.0),
        splat([-1.0, 0.0, -1.0], 0.3, [0.2, 1.0, 0.2], 1.0),
        splat([0.0, 0.6, 0.1], 0.3, [0.2, 0.2, 1.0], 1.0),
    ];
    let chunks = [SplatChunk::single(0, revision(), markers.to_vec())];
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::new(&chunks),
        overlays: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let camera = slice_camera(&pyramid, 16);
    let view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    settle(&gpu, &mut renderer, &view, &volume);
    let on_plane = project(&camera, [0.0, 0.0, 0.1]);
    assert!(renderer.request_pick(gpu.device(), gpu.queue(), on_plane));
    assert!(matches!(
        picked(&gpu, &mut renderer),
        PickId::Splat {
            level: 0,
            index: 0,
            ..
        }
    ));
    let off_plane = project(&camera, [-1.0, 0.0, -1.0]);
    assert!(renderer.request_pick(gpu.device(), gpu.queue(), off_plane));
    assert_eq!(picked(&gpu, &mut renderer), PickId::None);
    let second = project(&camera, [0.0, 0.6, 0.1]);
    assert!(renderer.request_pick(gpu.device(), gpu.queue(), second));
    assert!(matches!(
        picked(&gpu, &mut renderer),
        PickId::Splat {
            level: 0,
            index: 2,
            ..
        }
    ));
    let filler = vec![splat([0.0, 0.0, 0.1], 0.3, [0.0, 0.0, 0.0], 0.0); 256];
    let shrunk = [
        SplatChunk::single(0, revision(), vec![markers[0]]),
        SplatChunk::new(
            1,
            0,
            false,
            grana_render::SplatLevel::of(revision(), filler),
        ),
    ];
    let shrunk_scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::new(&shrunk),
        overlays: &[],
    };
    let shrunk_view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &shrunk_scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    settle(&gpu, &mut renderer, &shrunk_view, &volume);
    assert!(renderer.request_pick(gpu.device(), gpu.queue(), second));
    assert_eq!(
        picked(&gpu, &mut renderer),
        PickId::None,
        "a shrunk splat slot drops the instances it no longer holds"
    );
}

#[test]
fn composites_soft_splats_through_the_depth_layers() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers)
        .into_iter()
        .map(|row| grana_render::ChannelRow {
            visible: false,
            ..row
        })
        .collect::<Vec<_>>();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let forward = camera.forward();
    let near = (-forward * 0.3).to_array();
    let far = (forward * 0.3).to_array();
    let splats = |alpha: f32| {
        vec![
            splat(near, 0.4, [1.0, 0.1, 0.1], alpha),
            splat(far, 0.4, [0.1, 0.2, 1.0], alpha),
        ]
    };
    let solid_chunks = [SplatChunk::single(0, revision(), splats(1.0))];
    let faint_chunks = [SplatChunk::single(0, revision(), splats(0.25))];
    let solid_scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::new(&solid_chunks),
        overlays: &[],
    };
    let faint_scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::new(&faint_chunks),
        overlays: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let solid_view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &solid_scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Assured,
    };
    let faint_view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &faint_scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Assured,
    };
    let solid = snapshot(&gpu, &mut renderer, &solid_view, &target, &volume);
    let faint = snapshot(&gpu, &mut renderer, &faint_view, &target, &volume);
    let centre = project(&camera, [0.0, 0.0, 0.0]);
    let core = [centre[0] - 2, centre[1] - 2, centre[0] + 2, centre[1] + 2];
    assert!(
        solid.painted(WHOLE) > 100,
        "the splats paint {} pixels",
        solid.painted(WHOLE)
    );
    assert_eq!(
        solid.hue(core, Hue::Blue),
        0,
        "the farther splat covers the nearer one"
    );
    assert_eq!(
        solid.hue(core, Hue::Red),
        solid.painted(core),
        "the nearer splat owns the core"
    );
    assert!(
        solid.contrast(core) > faint.contrast(core) * 1.5,
        "the opacity scales the splat: {} against {}",
        solid.contrast(core),
        faint.contrast(core)
    );
    let occluder = PackedMesh::new(placed_quads(&[([0.0, 0.0, 0.0], 2.5)]));
    let occluding = [Layer::new(
        LayerKey::new(1, 0, 0),
        1,
        1,
        Arc::new(occluder),
        Arc::from([Style::new([0.1, 1.0, 0.1], true, false, 1.0)]),
        0,
    )];
    let mut face_on = camera.clone();
    let mut state = face_on.state();
    state.yaw = 0.0;
    state.pitch = 0.0;
    face_on.restore(state);
    let occluded = |centre: f32| {
        let chunks = [SplatChunk::single(
            0,
            revision(),
            vec![splat([0.0, 0.0, centre], 0.5, [1.0, 0.1, 0.1], 1.0)],
        )];
        let scene = SceneFrame {
            versions: versions(),
            channels: &channels,
            shading: Shading::default(),
            step_scale: 0.5,
            layers: &occluding,
            splats: Splats::new(&chunks),
            overlays: &[],
        };
        let view = View {
            target: &view_texture,
            camera: &face_on,
            region: Region::none(),
            scene: &scene,
            viewport: WHOLE,
            delivery: grana_render::Delivery::Assured,
        };
        snapshot(
            &gpu,
            &mut build_renderer(&gpu, &pyramid),
            &view,
            &target,
            &volume,
        )
    };
    let behind = around(project(&face_on, [0.0, 0.0, 0.0]), 5);
    let raising = occluded(-0.2);
    let buried = occluded(-1.5);
    assert!(
        raising.hue(behind, Hue::Red) > 40,
        "the near surface of a splat behind an occluder shows through: {} red pixels",
        raising.hue(behind, Hue::Red)
    );
    assert_eq!(
        buried.hue(behind, Hue::Red),
        0,
        "a splat whose whole body lies behind the occluder stays hidden"
    );
}

#[test]
fn converges_the_brightness_of_overlapping_splats() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers)
        .into_iter()
        .map(|row| grana_render::ChannelRow {
            visible: false,
            ..row
        })
        .collect::<Vec<_>>();
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let mut renderer = build_renderer(&gpu, &pyramid);
    let [cx, cy] = project(&camera, [0.0, 0.0, 0.0]);
    let core = [cx - 1, cy - 1, cx + 2, cy + 2];
    let mut brightness = |count: usize| {
        let splats = (0..count)
            .map(|_| splat([0.0, 0.0, 0.0], 0.4, [1.0, 0.0, 0.0], 0.25))
            .collect::<Vec<_>>();
        let chunks = [SplatChunk::single(0, revision(), splats)];
        let scene = SceneFrame {
            versions: versions(),
            channels: &channels,
            shading: Shading::default(),
            step_scale: 0.5,
            layers: &[],
            splats: Splats::new(&chunks),
            overlays: &[],
        };
        let view = View {
            target: &view_texture,
            camera: &camera,
            region: Region::none(),
            scene: &scene,
            viewport: WHOLE,
            delivery: grana_render::Delivery::Assured,
        };
        snapshot(&gpu, &mut renderer, &view, &target, &volume).contrast(core)
    };
    let encoded = |alpha: f32| {
        let mapped = 0.8038 * alpha;
        let encoded = if mapped <= 0.0031308 {
            12.92 * mapped
        } else {
            1.055 * mapped.powf(1.0 / 2.4) - 0.055
        };
        255.0 * encoded
    };
    let expected = |count: i32| encoded(1.0 - 0.75f32.powi(count));
    let single = brightness(1);
    let eight = brightness(8);
    let many = brightness(64);
    assert!(
        (single - expected(1)).abs() < 10.0,
        "one splat paints {single} against {}",
        expected(1)
    );
    assert!(
        (eight - expected(8)).abs() < 10.0,
        "eight splats paint {eight} against {}",
        expected(8)
    );
    assert!(
        (many - expected(64)).abs() < 6.0,
        "sixty four splats paint {many} against {}",
        expected(64)
    );
    assert!(
        eight > single + 80.0,
        "overlapping splats keep the brightness of one: {eight} against {single}"
    );
}

#[test]
fn follows_a_dragged_camera_without_trailing_the_view_it_left() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = bricked_volume(None);
    let pyramid = volume.pyramid();
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let opening = camera.clone();
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading {
            ambient: 0.15,
            diffuse: 1.0,
            shadows: 1.0,
            ..Shading::default()
        },
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let start = View {
        target: &view_texture,
        camera: &opening,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    settle_with(&gpu, &mut renderer, || frame(&start), &volume);
    for _ in 0..192 {
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&start))
            .expect("the settled frame draws");
    }
    for _ in 0..12 {
        camera.orbit(Vec2::new(20.0, 0.0));
        let view = View {
            target: &view_texture,
            camera: &camera,
            region: Region::none(),
            scene: &scene,
            viewport: WHOLE,
            delivery: grana_render::Delivery::Immediate,
        };
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&view))
            .expect("the dragged frame draws");
    }
    let dragged = read_pixels(&gpu, &target, [SIDE, SIDE]);
    let end = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    for _ in 0..192 {
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&end))
            .expect("the resting frame draws");
    }
    let rest = read_pixels(&gpu, &target, [SIDE, SIDE]);
    let trail = mean_difference(&dragged, &rest);
    let peak = max_difference(&dragged, &rest);
    assert!(
        trail < 2.0,
        "the dragged view trails the view it left: {trail:.3} levels over a peak of {peak}"
    );
}

#[test]
fn refuses_a_transparency_store_that_cannot_hold_the_layers_it_needs() {
    let limits = wgpu::Limits::default();
    let size = [128, 128];
    let allowance = 3 << 20;
    assert!(
        grana_render::fragment_fit(size, allowance, &limits, 2).is_err(),
        "a view that needs two transparent layers must not fall back to one"
    );
    assert!(
        grana_render::fragment_fit(size, allowance, &limits, 1).is_ok(),
        "the same view still fits a single transparent layer"
    );
}

#[test]
fn claims_an_export_the_ceiling_cannot_hold_at_full_detail() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = bricked_volume(None);
    let pyramid = volume.pyramid();
    let mut renderer = build_renderer(&gpu, &pyramid);
    let detail = grana_render::Detail {
        level: 0,
        bricks: 100_000,
    };
    let ceiling = renderer.frames() + renderer.splat_budget() + (32 << 20);
    renderer
        .reserve(
            gpu.device(),
            &pyramid,
            budget(BUDGET, [SIDE, SIDE]),
            detail,
            grana_render::FrameBudget {
                size: [SIDE, SIDE],
                demand: grana_render::Demand::full(),
            },
            ceiling,
        )
        .expect("an export claims the bricks its ceiling can hold rather than refusing outright");
    assert!(
        renderer.slots() > 1 && renderer.slots() < detail.bricks,
        "the claim follows the ceiling and not the brick count of the whole ladder"
    );
    assert!(
        renderer.degraded().is_none(),
        "a fresh claim reports no degradation before a brick is denied"
    );
}

#[test]
fn abandons_the_bricks_a_reader_cannot_supply() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = bricked_volume(None);
    let fine = volume.pyramid().level(0);
    let extent = fine.extent();
    let spacing = fine.spacing();
    let pyramid = Pyramid::new(
        vec![
            fine,
            Level::new(
                Extent::new(
                    extent.x().div_ceil(2),
                    extent.y().div_ceil(2),
                    extent.z().div_ceil(2),
                ),
                Spacing::new(spacing.x() * 2.0, spacing.y() * 2.0, spacing.z() * 2.0),
            ),
        ],
        1,
        volume.pyramid().channels().to_vec(),
        1,
        1.0,
        Calibration::Recorded,
    );
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    let mut missing = Vec::new();
    for _ in 0..8 {
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&view))
            .expect("the frame draws");
        if let Some(requests) = next_demand(&gpu, &mut renderer) {
            missing = requests.volume;
        }
        if !missing.is_empty() {
            break;
        }
    }
    assert!(
        !missing.is_empty(),
        "the view demands bricks before any of them arrive"
    );
    renderer.abandon(&missing);
    let held = SceneFrame {
        versions: versions(),
        ..scene
    };
    let changed = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &held,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let base = loop {
        if let Some(requests) = next_demand(&gpu, &mut renderer) {
            assert!(
                requests.volume.is_empty(),
                "an abandoned region is demanded again: {} bricks",
                requests.volume.len()
            );
        }
        let info = renderer
            .draw(gpu.device(), gpu.queue(), frame(&changed))
            .expect("the changed frame draws");
        if info.settled {
            assert_eq!(
                info.view.resident + info.view.unavailable,
                info.view.demand,
                "an abandoned region is unfetchable rather than missing"
            );
            assert!(
                info.view.unavailable > 0,
                "an abandoned region is reported as unfetchable"
            );
            break info.view.level;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the view never settles over an abandoned region"
        );
    };
    let mut wide = Camera::new(35.0);
    wide.fit(Vec3::from(volume.world_extent()) * 16.0, 1.0);
    let far = View {
        target: &view_texture,
        camera: &wide,
        region: Region::none(),
        scene: &held,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    let mut coarse = base;
    for _ in 0..16 {
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&far))
            .expect("the coarse frame draws");
        next_demand(&gpu, &mut renderer);
        let info = renderer
            .draw(gpu.device(), gpu.queue(), frame(&far))
            .expect("the coarse frame draws");
        coarse = info.view.level;
        if coarse > base {
            break;
        }
    }
    assert!(
        coarse > base,
        "a sixteenfold wider view addresses a coarser level than the abandoned one"
    );
    let mut reissued = Vec::new();
    for _ in 0..8 {
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&changed))
            .expect("the restored frame draws");
        if let Some(requests) = next_demand(&gpu, &mut renderer) {
            reissued.extend(requests.volume);
        }
        if reissued.iter().any(|request| missing.contains(request)) {
            break;
        }
    }
    assert!(
        reissued.iter().any(|request| missing.contains(request)),
        "an unreadable mark only outlives the level that demanded it"
    );
}

#[test]
fn keeps_the_accumulated_image_through_a_quiet_camera_move() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = bricked_volume(None);
    let pyramid = volume.pyramid();
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading {
            ambient: 0.15,
            diffuse: 1.0,
            shadows: 1.0,
            ..Shading::default()
        },
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let start = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    settle_with(&gpu, &mut renderer, || frame(&start), &volume);
    for _ in 0..64 {
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&start))
            .expect("the resting frame draws");
    }
    let settled = read_pixels(&gpu, &target, [SIDE, SIDE]);
    let mut nudged = camera.clone();
    nudged.pan(Vec2::new(0.05, 0.0), SIDE as f32);
    let moved = View {
        target: &view_texture,
        camera: &nudged,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    let info = renderer
        .draw(gpu.device(), gpu.queue(), frame(&moved))
        .expect("the nudged frame draws");
    assert!(
        info.history,
        "a quiet camera move must keep accumulating the history it holds"
    );
    let image = read_pixels(&gpu, &target, [SIDE, SIDE]);
    let drift = mean_difference(&settled, &image);
    assert!(
        drift < 0.15,
        "the reused accumulation fell back to a fresh sample: {drift:.3} levels"
    );
}

#[test]
fn keeps_the_demand_plan_through_a_sub_brick_pan() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = bricked_volume(None);
    let pyramid = volume.pyramid();
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let resting = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    settle(&gpu, &mut renderer, &resting, &volume);
    assert!(!renderer.requests_pending());
    let mut quiet = camera.clone();
    quiet.pan(Vec2::new(1.0, 0.0), SIDE as f32);
    let nudged = View {
        target: &view_texture,
        camera: &quiet,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    renderer
        .draw(gpu.device(), gpu.queue(), frame(&nudged))
        .expect("the nudged frame draws");
    assert!(
        !renderer.requests_pending(),
        "a sub-brick pan keeps the demand plan it already holds"
    );
    let mut loud = camera.clone();
    loud.pan(Vec2::new(64.0, 0.0), SIDE as f32);
    let panned = View {
        target: &view_texture,
        camera: &loud,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    renderer
        .draw(gpu.device(), gpu.queue(), frame(&panned))
        .expect("the panned frame draws");
    assert!(
        renderer.requests_pending(),
        "a pan across bricks plans the demand again"
    );
}

#[test]
fn reports_a_converged_image_only_when_it_stops_changing() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = gradient_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers)
        .into_iter()
        .map(|row| grana_render::ChannelRow {
            volume: VolumeStyle::new(grana_display::VolumeMode::Maximum, 0.5),
            ..row
        })
        .collect::<Vec<_>>();
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    settle_with(&gpu, &mut renderer, || frame(&view), &volume);
    let mut info = renderer
        .draw(gpu.device(), gpu.queue(), frame(&view))
        .expect("the measured frame draws");
    let mut draws = 0;
    while !info.converged && draws < 512 {
        info = renderer
            .draw(gpu.device(), gpu.queue(), frame(&view))
            .expect("the measured frame draws");
        draws += 1;
    }
    assert!(
        info.converged,
        "a projection of a smooth volume never converged: {} levels at {} samples",
        info.error, info.age
    );
    assert!(
        info.error <= 2.0,
        "the reported error {} exceeds the target at convergence",
        info.error
    );
    let converged = read_pixels(&gpu, &target, [SIDE, SIDE]);
    for _ in 0..256 {
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&view))
            .expect("the deeper frame draws");
    }
    let deeper = read_pixels(&gpu, &target, [SIDE, SIDE]);
    let drift = mean_difference(&converged, &deeper);
    assert!(
        drift < 1.0,
        "the converged image still moved by {drift:.3} levels with more samples"
    );
}

fn beads_volume() -> Volume {
    let extent = Extent::new(64, 64, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.2);
    let mut values = vec![0u16; extent.voxel_count() as usize];
    for cell in 0..8usize {
        let centre = [
            (cell % 2) as f32 * 24.0 + 20.0,
            ((cell / 2) % 2) as f32 * 24.0 + 20.0,
            (cell / 4) as f32 * 8.0 + 8.0,
        ];
        for z in 0..extent.z() {
            for y in 0..extent.y() {
                for x in 0..extent.x() {
                    let point = [x as f32, y as f32, z as f32];
                    let distance = (0..3)
                        .map(|axis| (point[axis] - centre[axis]).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    if distance <= 5.0 {
                        values[extent.index(x, y, z) as usize] = 60_000;
                    }
                }
            }
        }
    }
    Volume::new(
        extent,
        spacing,
        vec![Channel::new(
            "beads",
            [1.0, 0.6, 0.2],
            VoxelData::U16(values),
        )],
    )
}

#[test]
fn converges_a_hard_edged_isosurface_before_the_sample_budget() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = beads_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([1.0, 0.6, 0.2])];
    let channels = channels(&pyramid, &transfers)
        .into_iter()
        .map(|row| grana_render::ChannelRow {
            volume: VolumeStyle::new(grana_display::VolumeMode::Isosurface, 0.5),
            ..row
        })
        .collect::<Vec<_>>();
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    settle_with(&gpu, &mut renderer, || frame(&view), &volume);
    let mut info = renderer
        .draw(gpu.device(), gpu.queue(), frame(&view))
        .expect("the isosurface draws");
    let mut draws = 0;
    while !info.converged && !info.exhausted && draws < 512 {
        info = renderer
            .draw(gpu.device(), gpu.queue(), frame(&view))
            .expect("the isosurface draws");
        draws += 1;
    }
    assert!(
        info.converged,
        "a hard edged isosurface ran to the sample budget at {} levels",
        info.error
    );
    assert!(
        info.error <= 2.0,
        "the reported error {} exceeds the target at convergence",
        info.error
    );
}

fn depth_quads(quads: &[([f32; 2], f32, f32)]) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, (center, depth, half)) in quads.iter().enumerate() {
        let base = vertices.len() as u32;
        for (across, down) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            vertices.push(MeshVertex::new(
                [center[0] + across * half, center[1] + down * half, *depth],
                [0.0, 0.0, 1.0],
                object as u32,
            ));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(vertices, indices)
}

#[test]
fn reveals_the_surface_behind_a_moving_occluder() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = Volume::new(
        EMPTY,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "empty",
            [0.3, 0.6, 1.0],
            VoxelData::U16(vec![0; EMPTY.voxel_count() as usize]),
        )],
    );
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.3, 0.6, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let stamp = revision();
    let mesh = Arc::new(PackedMesh::new(depth_quads(&[
        ([0.0, 0.0], 10.0, 2.4),
        ([0.0, 0.0], 0.0, 4.0),
    ])));
    let styles = Arc::from([
        Style::new([1.0, 0.1, 0.1], true, false, 1.0),
        Style::new([0.1, 0.3, 1.0], true, false, 1.0),
    ]);
    let layers = [Layer::new(
        LayerKey::new(1, 0, 0),
        stamp,
        stamp,
        Arc::clone(&mesh),
        Arc::clone(&styles),
        0,
    )];
    let scene = SceneFrame {
        versions: grana_render::Versions::new(stamp, stamp, stamp, stamp),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        layers: &layers,
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut moved = camera.clone();
    moved.pan(Vec2::new(26.0, 0.0), SIDE as f32);
    let near = project(&camera, [2.4, 0.0, 10.0]);
    let near_moved = project(&moved, [2.4, 0.0, 10.0]);
    assert!(
        near_moved[0] > near[0] + 4,
        "the occluder must cover {near:?} and slide to {near_moved:?}"
    );
    let strip = [near[0] + 2, 24, near_moved[0] - 2, SIDE - 24];
    let settled = |camera: &Camera, draws: usize| {
        let mut renderer = build_renderer(&gpu, &pyramid);
        let target = texture(&gpu, SIDE);
        let view_texture = target.create_view(&Default::default());
        let view = View {
            target: &view_texture,
            camera,
            region: Region::none(),
            scene: &scene,
            viewport: WHOLE,
            delivery: grana_render::Delivery::Immediate,
        };
        settle_with(&gpu, &mut renderer, || frame(&view), &volume);
        for _ in 0..draws {
            renderer
                .draw(gpu.device(), gpu.queue(), frame(&view))
                .expect("the probed frame draws");
        }
        read_pixels(&gpu, &target, [SIDE, SIDE])
    };
    let fresh = settled(&moved, 16);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let mut renderer = build_renderer(&gpu, &pyramid);
    let settled_view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    let moved_view = View {
        target: &view_texture,
        camera: &moved,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    settle_with(&gpu, &mut renderer, || frame(&settled_view), &volume);
    for _ in 0..16 {
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&settled_view))
            .expect("the settled frame draws");
    }
    for _ in 0..3 {
        renderer
            .draw(gpu.device(), gpu.queue(), frame(&moved_view))
            .expect("the revealed frame draws");
    }
    let revealed = read_pixels(&gpu, &target, [SIDE, SIDE]);
    let error = rect_difference(&revealed, &fresh, strip);
    let red = revealed
        .iter()
        .enumerate()
        .filter(|(index, pixel)| {
            let x = (index % SIDE as usize) as u32;
            let y = (index / SIDE as usize) as u32;
            x >= strip[0] && x < strip[2] && y >= strip[1] && y < strip[3] && pixel[0] > pixel[2]
        })
        .count();
    assert!(
        error < 6.0,
        "the revealed strip trails the occluder by {error:.3} levels ({red} red pixels)"
    );
}

#[test]
fn prefetches_the_carried_frame_into_vacant_slots() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = bricked_volume(None);
    let base = volume.pyramid();
    let pyramid = Pyramid::placed(
        base.levels().to_vec(),
        base.level_count(),
        base.channels().to_vec(),
        2,
        1.0,
        base.placement(),
        base.calibration(),
    );
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = Renderer::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        &crate::support::ledger(),
        128 << 20,
        gpu.pipelines(),
    );
    renderer
        .set_dataset(
            gpu.device(),
            &pyramid,
            budget(128 << 20, [SIDE, SIDE]),
            [SIDE, SIDE],
        )
        .expect("the test budget holds the volume");
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Awaited,
    };
    let mut issued = Vec::new();
    settle_time(
        &gpu,
        &mut renderer,
        &volume,
        &view,
        0,
        &mut Vec::new(),
        &mut issued,
    );
    assert!(
        !issued.is_empty(),
        "a settled playback frame prefetches the frame it carries towards"
    );
    assert!(
        issued.iter().all(|request| request.time == 1),
        "the prefetch names the carried frame: {:?}",
        issued
            .iter()
            .map(|request| request.time)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        renderer.statistics().evictions,
        0,
        "prefetching into vacant slots evicts nothing"
    );
    let mut demanded = Vec::new();
    settle_time(
        &gpu,
        &mut renderer,
        &volume,
        &view,
        1,
        &mut demanded,
        &mut Vec::new(),
    );
    assert!(
        renderer.statistics().prefetch_hits > 0,
        "the prefetched bricks arrive before the frame that needs them"
    );
    assert!(
        demanded.iter().all(|request| !issued.contains(request)),
        "the carried frame never demands a brick it prefetched"
    );
}

fn timed<'a>(view: &'a View<'a>, time: u32) -> Frame<'a, 'a> {
    Frame {
        time,
        delivery: grana_render::Delivery::Awaited,
        ..frame(view)
    }
}

fn settle_time(
    gpu: &Gpu,
    renderer: &mut Renderer,
    volume: &Volume,
    view: &View<'_>,
    time: u32,
    demanded: &mut Vec<BrickRequest>,
    prefetched: &mut Vec<BrickRequest>,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        let info = renderer
            .draw(gpu.device(), gpu.queue(), timed(view, time))
            .expect("the timed frame draws");
        if renderer.requests_pending() {
            if let Some(requests) = next_demand(gpu, renderer) {
                demanded.extend(requests.volume.iter().copied());
                prefetched.extend(requests.prefetch.iter().copied());
                let mut bricks = pack(volume, &requests.volume);
                bricks.extend(pack(volume, &requests.prefetch));
                renderer.upload(&bricks);
            }
        } else if info.drawn && info.settled && info.age >= FRAMES && !info.fading {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the timed stream never settled: age {}, settled {}, pending {}",
            info.age,
            info.settled,
            renderer.requests_pending()
        );
    }
}

const HOLLOW_BUDGET: u64 = 128 << 20;
const CLOSE_DISTANCE: f32 = 80.0;
const OVERVIEW_DISTANCE: f32 = 240.0;

#[test]
fn keeps_the_fitted_level_across_a_camera_move() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volumes = ladder_volumes();
    let pyramid = ladder_pyramid(&volumes);
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let mut renderer = Renderer::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        &crate::support::ledger(),
        HOLLOW_BUDGET,
        gpu.pipelines(),
    );
    renderer
        .set_dataset(
            gpu.device(),
            &pyramid,
            budget(HOLLOW_BUDGET, [SIDE, SIDE]),
            [SIDE, SIDE],
        )
        .expect("the hollow budget holds the volume");
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(pyramid.level(0).world_extent()), 1.0);
    let mut state = camera.state();
    state.distance = CLOSE_DISTANCE;
    camera.restore(state);
    let close = ladder_view(&view_texture, &camera, &scene);
    let fitted = stream_levels(&gpu, &mut renderer, &close, &volumes);
    assert_eq!(
        fitted.view.level, 0,
        "the close view settles on level {} instead of the finest one",
        fitted.view.level
    );
    let bricks = BrickGrid::new(pyramid.level(0).extent()).count();
    assert!(
        fitted.view.visible * 3 < bricks,
        "the close view counts {} of {bricks} bricks as occupied, so it holds too few hollow bricks to fit a level",
        fitted.view.visible
    );
    let mut overview = camera.clone();
    let mut state = overview.state();
    state.distance = OVERVIEW_DISTANCE;
    overview.restore(state);
    let view = ladder_view(&view_texture, &overview, &scene);
    let moved = renderer
        .draw(gpu.device(), gpu.queue(), frame(&view))
        .expect("the moved frame draws");
    assert_eq!(
        moved.view.level, fitted.view.level,
        "the first frame after the camera move drops from level {} to level {}",
        fitted.view.level, moved.view.level
    );
    let settled = stream_levels(&gpu, &mut renderer, &view, &volumes);
    assert_eq!(
        settled.view.level, fitted.view.level,
        "the overview settles on level {} after the move from level {}",
        settled.view.level, fitted.view.level
    );
}

fn frame_sized<'a, 'b: 'a>(
    target: &'a wgpu::TextureView,
    size: [u32; 2],
    camera: &'a Camera,
    scene: &'a SceneFrame<'b>,
) -> Frame<'a, 'b> {
    Frame {
        target,
        size,
        camera,
        viewport: [0, 0, size[0], size[1]],
        time: 0,
        region: Region::none(),
        clip: Clip::none(),
        exposure: 1.0,
        scene,
        delivery: grana_render::Delivery::Immediate,
    }
}

#[test]
fn carries_its_bricks_across_a_resize_that_reseats_the_atlas() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = bricked_volume(None);
    let fine = volume.pyramid().level(0);
    let pyramid = Pyramid::new(
        vec![fine],
        1,
        volume.pyramid().channels().to_vec(),
        1,
        1.0,
        Calibration::Recorded,
    );
    let transfers = pyramid
        .channels()
        .iter()
        .map(|channel| TransferFunction::tinted(channel.tint()))
        .collect::<Vec<_>>();
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading {
            ambient: 0.15,
            diffuse: 1.0,
            shadows: 1.0,
            ..Shading::default()
        },
        step_scale: 0.5,
        layers: &[],
        splats: Splats::empty(),
        overlays: &[],
    };
    let narrow = [SIDE, SIDE];
    let roomy = [SIDE * 8, SIDE * 8];
    let total = budget(96 << 20, roomy);
    let mut renderer = Renderer::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        &crate::support::ledger(),
        total,
        gpu.pipelines(),
    );
    renderer
        .set_dataset(gpu.device(), &pyramid, total, narrow)
        .expect("the test budget holds the volume");
    let narrow_target = texture(&gpu, narrow[0]);
    let narrow_view = narrow_target.create_view(&Default::default());
    let roomy_target = texture(&gpu, roomy[0]);
    let roomy_view = roomy_target.create_view(&Default::default());
    settle_with(
        &gpu,
        &mut renderer,
        || frame_sized(&narrow_view, narrow, &camera, &scene),
        &volume,
    );
    let slots = renderer.slots();
    let residency = renderer.budget();
    let uploads = renderer.statistics().uploads;
    let staged = renderer.statistics().staged;
    assert!(uploads > 0, "the settled view streams its bricks");
    assert!(staged > 0, "the settled view stages its bricks");
    renderer.restart();
    renderer
        .draw(
            gpu.device(),
            gpu.queue(),
            frame_sized(&narrow_view, narrow, &camera, &scene),
        )
        .expect("the reference frame draws");
    let reference = read_pixels(&gpu, &narrow_target, narrow);
    settle_with(
        &gpu,
        &mut renderer,
        || frame_sized(&roomy_view, roomy, &camera, &scene),
        &volume,
    );
    assert_ne!(
        renderer.budget(),
        residency,
        "the roomy view reseats the atlas with the same {residency} byte residency, so the resize never reached it"
    );
    assert_eq!(
        renderer.statistics().uploads,
        uploads,
        "the resize streamed bricks it already held"
    );
    assert!(
        renderer.statistics().staged >= staged,
        "the resize restarted the staging account at {} bytes from the {} it had streamed",
        renderer.statistics().staged,
        staged
    );
    let roomy_residency = renderer.budget();
    renderer
        .draw(
            gpu.device(),
            gpu.queue(),
            frame_sized(&narrow_view, narrow, &camera, &scene),
        )
        .expect("the first narrow frame draws");
    assert_ne!(
        renderer.budget(),
        roomy_residency,
        "the first frame back at the narrow view keeps the {roomy_residency} byte residency of the roomy view it replaced, so the atlas reseats once on that frame and once on the next"
    );
    let stepped = renderer.budget();
    renderer
        .draw(
            gpu.device(),
            gpu.queue(),
            frame_sized(&narrow_view, narrow, &camera, &scene),
        )
        .expect("the second narrow frame draws");
    assert_eq!(
        renderer.budget(),
        stepped,
        "the frame after the resize reseats the atlas again: {stepped} bytes became {}",
        renderer.budget()
    );
    settle_with(
        &gpu,
        &mut renderer,
        || frame_sized(&narrow_view, narrow, &camera, &scene),
        &volume,
    );
    assert_eq!(
        renderer.budget(),
        stepped,
        "the narrow view settles at a residency it reseated after its first frame"
    );
    assert_eq!(
        renderer.slots(),
        slots,
        "the narrow view reseats the atlas back to the slots it held"
    );
    assert_eq!(
        renderer.statistics().uploads,
        uploads,
        "resizing back streamed bricks it already held"
    );
    renderer.restart();
    renderer
        .draw(
            gpu.device(),
            gpu.queue(),
            frame_sized(&narrow_view, narrow, &camera, &scene),
        )
        .expect("the carried frame draws");
    let carried = read_pixels(&gpu, &narrow_target, narrow);
    assert_eq!(
        max_difference(&reference, &carried),
        0,
        "the carried bricks shade differently from the bricks the renderer streamed"
    );
}
