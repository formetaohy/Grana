use std::sync::Arc;

use glam::{Vec3, Vec4};
use grana_display::{Camera, Shading, Slice, TransferFunction, VolumeStyle};
use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_render::{
    Frame, Layer, LayerKey, PickId, Renderer, Requests, SceneFrame, SplatChunk, Splats, Style,
};
use grana_volume::{
    BrickCoord, BrickGrid, BrickRequest, Channel, ChannelSet, Clip, Damage, Extent, PackedBrick,
    Pyramid, Region, Spacing, Volume, VoxelData,
};

use crate::support::{Gpu, Hue, Snapshot, gpu, read_pixels, revision, spheres, splat, versions};

const SIDE: u32 = 128;
const CAP: usize = 0;
const FRAMES: u32 = 8;
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
    capped(view, usize::MAX)
}

fn capped<'a>(view: &View<'a>, level_cap: usize) -> Frame<'a, 'a> {
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
        level_cap,
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
            window: channel.window(),
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
        BUDGET,
        gpu.pipelines(),
    );
    renderer.set_dataset(gpu.device(), pyramid);
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
            .poll_requests(gpu.device())
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
            window: row.window,
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
    let view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        delivery: grana_render::Delivery::Immediate,
    };
    let requests = settle_with(&gpu, &mut renderer, || capped(&view, CAP), &volume);
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
    settle_with(&gpu, &mut renderer, || capped(&view, CAP), &volume);
    let shaded = settled_pixels(&gpu, &mut renderer, &view, &target, CAP);
    let occluder = bricked_volume(Some([1, 1, 0]));
    let region = voxel_region(&pyramid, 0, [48, 48, 16]);
    let damage = Damage::new(region, ChannelSet::of(0)).expect("the damage names a channel");
    renderer.damage(&[damage]);
    refresh(&gpu, &mut renderer, &view, &occluder, CAP);
    settle_with(&gpu, &mut renderer, || capped(&view, CAP), &occluder);
    let streamed = settled_pixels(&gpu, &mut renderer, &view, &target, CAP);
    let mut cold = build_renderer(&gpu, &occluder.pyramid());
    settle_with(&gpu, &mut cold, || capped(&view, CAP), &occluder);
    let rebuilt = settled_pixels(&gpu, &mut cold, &view, &target, CAP);
    let moved = mean_difference(&shaded, &streamed);
    assert!(
        moved > 2.0,
        "the streamed brick left the shading unchanged: {moved:.3}"
    );
    let stale = max_difference(&streamed, &rebuilt);
    assert!(
        stale <= 60,
        "the streamed shadow departs from a full rebuild by {stale}"
    );
}

fn voxel_region(pyramid: &Pyramid, level: usize, voxel: [u32; 3]) -> Region {
    let source = pyramid.level(level);
    let basis = source.basis().matrix();
    let low = Vec3::from(source.origin())
        + basis * Vec3::new(voxel[0] as f32, voxel[1] as f32, voxel[2] as f32);
    let high = low + basis * Vec3::new(1.0, 1.0, 1.0);
    Region::axis_aligned(low, high)
}

fn refresh(gpu: &Gpu, renderer: &mut Renderer, view: &View<'_>, volume: &Volume, cap: usize) {
    for _ in 0..8 {
        renderer
            .draw(gpu.device(), gpu.queue(), capped(view, cap))
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
    cap: usize,
) -> Vec<[u8; 4]> {
    for _ in 0..64 {
        renderer
            .draw(gpu.device(), gpu.queue(), capped(view, cap))
            .expect("the converged frame draws");
    }
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

fn placed_quads(quads: &[([f32; 2], f32)]) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, (center, half)) in quads.iter().enumerate() {
        let base = vertices.len() as u32;
        for (across, down) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            vertices.push(MeshVertex::new(
                [center[0] + across * half, center[1] + down * half, 0.0],
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
    let centers = [([-3.4, 0.0], 1.6), ([3.4, 0.0], 1.6)];
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
            versions: grana_render::Versions::new(stamp, stamp, stamp),
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
        grana_render::Versions::new(stamp, stamp, stamp.wrapping_add(1)),
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
        grana_render::Versions::new(stamp, stamp.wrapping_add(1), stamp.wrapping_add(2)),
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
    let centers = [([-3.4, 0.0], 1.6), ([3.4, 0.0], 1.6), ([0.0, 3.4], 1.6)];
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
    let pyramid = volume.pyramid();
    let channels = vec![grana_render::ChannelRow {
        visible,
        opacity: 1.0,
        window: pyramid.channels()[0].window(),
        transfer: TransferFunction::tinted(pyramid.channels()[0].tint()),
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
        base.axial(),
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
            16 << 20,
            gpu.pipelines(),
        );
        renderer.set_dataset(gpu.device(), &pyramid);
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
        400 << 10,
        gpu.pipelines(),
    );
    renderer.set_dataset(gpu.device(), &pyramid);
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
