use std::sync::Arc;

use glam::{Vec3, Vec4};
use grana_display::{Camera, Shading, Slice, TransferFunction, VolumeStyle};
use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_object::{IntensityTable, SampleStatistics, Spot, SpotSet};
use grana_render::{
    Frame, Layer, LayerKey, PickId, Renderer, Requests, SceneFrame, Splat, SplatChunk, Splats,
    Style, Versions,
};
use grana_scene::{Dataset, Item, ObjectFrame, ObjectKind, Scene, Selection};
use grana_volume::{
    Basis, BrickGrid, Channel, Clip, Extent, Level, Pyramid, Region, Spacing, Volume, VoxelData,
};

use crate::support::{Gpu, Hue, Snapshot, gpu, read_pixels, revision, spheres, versions};

const SIDE: u32 = 128;
const SAMPLES: u32 = 4;
const BUDGET: u64 = 256 << 20;
const WHOLE: [u32; 4] = [0, 0, SIDE, SIDE];
const EMPTY: Extent = Extent::new(16, 16, 16);

struct View<'a> {
    target: &'a wgpu::TextureView,
    camera: &'a Camera,
    region: Region,
    scene: &'a SceneFrame<'a>,
    viewport: [u32; 4],
    display: grana_render::Display,
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
        samples: SAMPLES,
        scene: view.scene,
        display: view.display,
        level_cap: usize::MAX,
        temporal: None,
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
    );
    renderer.set_dataset(gpu.device(), pyramid);
    renderer
}

fn two_channel_volume() -> Volume {
    let extent = Extent::new(64, 64, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.2);
    let first = spheres(extent, spacing, &[([24.0, 32.0, 16.0], 10.0)], 60_000);
    let second = spheres(extent, spacing, &[([44.0, 32.0, 16.0], 9.0)], 60_000);
    Volume::new(
        extent,
        spacing,
        vec![
            Channel::new("first", [0.2, 0.4, 1.0], first.channels()[0].data().clone()),
            Channel::new(
                "second",
                [0.2, 1.0, 0.3],
                second.channels()[0].data().clone(),
            ),
        ],
    )
}

fn sphere_volume() -> Volume {
    let extent = Extent::new(64, 64, 32);
    let spacing = Spacing::new(0.1, 0.1, 0.2);
    spheres(extent, spacing, &[([32.0, 32.0, 16.0], 12.0)], 60_000)
}

fn quad(half: f32) -> Mesh {
    let vertices = vec![
        MeshVertex::new([-half, -half, 0.0], [0.0, 0.0, 1.0], 0),
        MeshVertex::new([half, -half, 0.0], [0.0, 0.0, 1.0], 0),
        MeshVertex::new([half, half, 0.0], [0.0, 0.0, 1.0], 0),
        MeshVertex::new([-half, half, 0.0], [0.0, 0.0, 1.0], 0),
    ];
    Mesh::new(vertices, vec![0, 1, 2, 0, 2, 3])
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
    let mut latest = Requests::default();
    for _ in 0..64 {
        let mut info = renderer.draw(gpu.device(), gpu.queue(), build());
        if !renderer.requests_pending() {
            while info.samples < SAMPLES || info.fading {
                info = renderer.draw(gpu.device(), gpu.queue(), build());
            }
            return latest;
        }
        if let Some(requests) = next_demand(gpu, renderer) {
            let grid = BrickGrid::new(volume.extent());
            let bricks = requests
                .volume
                .iter()
                .map(|request| (*request, volume.brick(&grid, request.coord)))
                .collect::<Vec<_>>();
            renderer.upload(gpu.queue(), &bricks);
            latest = requests;
        }
    }
    panic!("the stream never settled");
}

fn next_demand(gpu: &Gpu, renderer: &mut Renderer) -> Option<Requests> {
    for _ in 0..4 {
        match renderer.poll_requests(gpu.device()) {
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
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
    };
    let blank = View {
        target: view.target,
        camera: view.camera,
        region: view.region,
        scene: &scene,
        viewport: view.viewport,
        display: view.display,
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
fn measures_the_convergence_of_a_volume() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
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
        display: grana_render::Display::Export,
    };
    settle(&gpu, &mut renderer, &view, &volume);
    renderer.set_exporting(true);
    let mut reset = frame(&view);
    reset.samples = 12;
    reset.exposure = 1.4;
    renderer.draw(gpu.device(), gpu.queue(), reset);
    let mut metrics = Vec::new();
    for _ in 0..24 {
        let mut drawn = frame(&view);
        drawn.samples = 12;
        drawn.exposure = 1.4;
        let info = renderer.draw(gpu.device(), gpu.queue(), drawn);
        gpu.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("failed to wait for the gpu");
        if let Some(metric) = renderer.convergence(&gpu) {
            metrics.push(metric);
        }
        if info.samples >= 12 {
            break;
        }
    }
    assert!(!metrics.is_empty(), "the render measures no convergence");
    assert!(
        metrics.first().is_some_and(|(samples, _)| *samples >= 2),
        "the first metric comes before a second sample"
    );
    let standard_error = |(samples, error): (u32, f32)| error / (samples as f32).sqrt();
    assert!(
        standard_error(*metrics.last().expect("a metric is measured"))
            < standard_error(*metrics.first().expect("a metric is measured")),
        "the standard error does not fall as samples accumulate: {metrics:?}"
    );
    let mut restarted = None;
    for _ in 0..4 {
        let mut drawn = frame(&view);
        drawn.samples = 12;
        drawn.exposure = 1.8;
        renderer.draw(gpu.device(), gpu.queue(), drawn);
        gpu.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("failed to wait for the gpu");
        if let Some((samples, _)) = renderer.convergence(&gpu) {
            restarted = Some(samples);
        }
    }
    assert!(
        restarted.is_some_and(|samples| samples <= 4),
        "a changed exposure keeps the samples of the previous frame: {restarted:?}"
    );
}

#[test]
fn streams_and_shades_two_channels() {
    let Some(gpu) = gpu() else {
        return;
    };
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
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
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
        display: grana_render::Display::Still,
    };
    let requests = settle(&gpu, &mut renderer, &view, &volume);
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
}

#[test]
fn renders_a_rotated_placement() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let aligned = volume.pyramid();
    let level = Level::of(
        aligned.level(0).extent(),
        Basis::new([[0.0, 0.1, 0.0], [-0.1, 0.0, 0.0], [0.0, 0.0, 0.2]]),
    );
    let pyramid = Pyramid::placed(
        vec![level],
        1,
        aligned.channels().to_vec(),
        1,
        1.0,
        [0.0; 3],
    );
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = pyramid
        .channels()
        .iter()
        .zip(&transfers)
        .map(|(channel, transfer)| grana_render::ChannelRow {
            visible: true,
            opacity: 1.0,
            window: grana_volume::Intensity::new(
                channel.window().min(),
                channel.window().max() * 2.0,
            ),
            transfer: transfer.clone(),
            volume: VolumeStyle::default(),
        })
        .collect::<Vec<_>>();
    let world = Vec3::from(level.world_extent());
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
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
        display: grana_render::Display::Still,
    };
    let requests = settle(&gpu, &mut renderer, &view, &volume);
    assert!(
        requests.view.visible > 0,
        "the rotated volume sees no brick"
    );
    let shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
    assert!(
        shot.painted(shot.all()) > (SIDE * SIDE / 64) as usize,
        "the rotated volume barely rendered: {} painted pixels",
        shot.painted(shot.all())
    );
    let mut aligned_renderer = build_renderer(&gpu, &aligned);
    let aligned_target = texture(&gpu, SIDE);
    let aligned_texture = aligned_target.create_view(&Default::default());
    let aligned_view = View {
        target: &aligned_texture,
        camera: &camera,
        region: Region::none(),
        scene: &scene,
        viewport: WHOLE,
        display: grana_render::Display::Still,
    };
    let aligned_shot = snapshot(
        &gpu,
        &mut aligned_renderer,
        &aligned_view,
        &aligned_target,
        &volume,
    );
    let rotated = shot.painted(shot.all());
    let reference = aligned_shot.painted(aligned_shot.all());
    assert!(
        rotated * 10 > reference * 9 && rotated * 9 < reference * 10,
        "the quarter turn changed the sphere: {rotated} against {reference} painted pixels"
    );
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
    let mesh = PackedMesh::new(quad(8.0));
    let styles = [Style::new([1.0, 0.1, 0.1], true, false, 1.0)];
    let layers = [Layer {
        key: LayerKey::stable(1, 0, 1),
        mesh: Arc::new(mesh),
        style_start: 0,
        style_count: 1,
    }];
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        splats: Splats::empty(),
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
        display: grana_render::Display::Still,
    };
    let shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
    assert!(
        shot.hue(shot.all(), Hue::Red) > 1_000,
        "the mesh barely rendered: {} red pixels",
        shot.hue(shot.all(), Hue::Red)
    );
    assert_eq!(
        shot.painted([0, 0, SIDE / 8, SIDE / 8]),
        0,
        "the mesh spilled into the corner"
    );
}

#[test]
fn keeps_a_translucent_layer() {
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
    let mut channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mesh = PackedMesh::new(quad(8.0));
    let styles = [Style::new([1.0, 0.1, 0.1], true, false, 0.5)];
    let layers = [Layer {
        key: LayerKey::stable(1, 0, 1),
        mesh: Arc::new(mesh),
        style_start: 0,
        style_count: 1,
    }];
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        splats: Splats::empty(),
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
        display: grana_render::Display::Still,
    };
    let shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
    let painted = shot.painted(WHOLE);
    assert!(
        painted > 400,
        "the translucent layer faded: {painted} painted pixels"
    );
    assert!(
        shot.coverage(shot.middle()) > 0.5,
        "the translucent layer barely covers its centre: {:.2}",
        shot.coverage(shot.middle())
    );
    assert!(
        shot.contrast(shot.middle()) > 10.0,
        "the translucent layer is invisible against the background: {:.1}",
        shot.contrast(shot.middle())
    );
    for channel in &mut channels {
        channel.visible = false;
    }
    let hidden = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &styles,
        layers: &layers,
        splats: Splats::empty(),
    };
    let view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &hidden,
        viewport: WHOLE,
        display: grana_render::Display::Still,
    };
    let hidden_shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
    assert!(
        hidden_shot.painted(WHOLE) > 400,
        "hiding every channel dropped the translucent layer: {} painted pixels",
        hidden_shot.painted(WHOLE)
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
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
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
        display: grana_render::Display::Still,
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
    grid: &BrickGrid,
) -> grana_render::FrameInfo {
    let mut info = renderer.draw(gpu.device(), gpu.queue(), frame(view));
    for _ in 0..512 {
        if info.samples >= SAMPLES && info.settled {
            assert!(
                !info.truncated,
                "a complete assured frame lost bricks to the budget"
            );
            return info;
        }
        if let Some(requests) = next_demand(gpu, renderer) {
            let bricks = requests
                .volume
                .iter()
                .map(|request| (*request, volume.brick(grid, request.coord)))
                .collect::<Vec<_>>();
            renderer.upload(gpu.queue(), &bricks);
        }
        info = renderer.draw(gpu.device(), gpu.queue(), frame(view));
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
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
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
        display: grana_render::Display::Export,
    };
    for _ in 0..4 {
        let info = renderer.draw(gpu.device(), gpu.queue(), frame(&view));
        assert_eq!(
            info.samples, 0,
            "an assured frame consumed samples before its bricks arrived"
        );
    }
    let grid = BrickGrid::new(volume.extent());
    let info = assure(gpu, &mut renderer, &view, volume, &grid);
    assert_eq!(info.samples, SAMPLES, "the assured frame stopped short");
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
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
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
        display: grana_render::Display::Still,
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
    assert!(
        projected < 1.0,
        "the maximum projection follows depth: {projected}"
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
    )
}

fn stream_levels(
    gpu: &Gpu,
    renderer: &mut Renderer,
    view: &View<'_>,
    volumes: &[Volume],
) -> grana_render::FrameInfo {
    for _ in 0..64 {
        let mut info = renderer.draw(gpu.device(), gpu.queue(), frame(view));
        if !renderer.requests_pending() {
            while info.samples < SAMPLES || info.fading {
                info = renderer.draw(gpu.device(), gpu.queue(), frame(view));
            }
            return info;
        }
        if let Some(requests) = next_demand(gpu, renderer) {
            let bricks = requests
                .volume
                .iter()
                .map(|request| {
                    let volume = &volumes[request.level];
                    let grid = BrickGrid::new(volume.extent());
                    (*request, volume.brick(&grid, request.coord))
                })
                .collect::<Vec<_>>();
            renderer.upload(gpu.queue(), &bricks);
        }
    }
    panic!("the stream never settled");
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
        display: grana_render::Display::Still,
    }
}

#[test]
fn degrades_the_volume_level_to_fit_a_residency_budget() {
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
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
    };
    let mut renderer = Renderer::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        400 << 10,
    );
    renderer.set_dataset(gpu.device(), &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = ladder_view(&view_texture, &camera, &scene);
    let info = stream_levels(&gpu, &mut renderer, &view, &volumes);
    assert_eq!(info.samples, SAMPLES, "the constrained view never resolved");
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

#[test]
fn rejects_a_residency_beyond_the_device() {
    let error = grana_render::residency_budget(64, 1 << 20, 128, 1 << 40)
        .expect_err("a tiny device holds no export at this size");
    assert!(
        error.contains("this device holds at most"),
        "the rejection names the device limit: {error}"
    );
    let error = grana_render::residency_budget(4096, 1 << 20, 2048, 1 << 30)
        .expect_err("a ceiling bounds the export residency");
    assert!(
        error.contains("export ceiling"),
        "the rejection names the export ceiling: {error}"
    );
}

#[test]
fn sizes_the_export_residency_to_the_finest_level() {
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
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
    };
    let detail = grana_render::residency(&pyramid, &camera, SIDE, Region::none());
    assert_eq!(
        detail.detail.level, 0,
        "a fitted export keeps the finest level"
    );
    let mut renderer = Renderer::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        1 << 20,
    );
    renderer.set_dataset(gpu.device(), &pyramid);
    let budget = grana_render::residency_budget(
        detail.slots,
        renderer.slot_bytes(),
        gpu.device().limits().max_texture_dimension_3d,
        grana_render::EXPORT_CEILING,
    )
    .expect("the device holds the export");
    renderer.set_budget(budget);
    renderer.set_dataset(gpu.device(), &pyramid);
    assert!(
        renderer.slots() >= detail.slots,
        "the reserved atlas holds every brick of the export"
    );
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let mut view = ladder_view(&view_texture, &camera, &scene);
    view.display = grana_render::Display::Export;
    let info = stream_levels(&gpu, &mut renderer, &view, &volumes);
    assert_eq!(info.samples, SAMPLES, "the sized export never resolved");
    assert_eq!(info.view.level, 0, "the sized export coarsens its level");
    assert_eq!(
        info.view.visible, info.view.demand,
        "the sized export still truncates its bricks"
    );
    assert!(!info.truncated, "the sized export reports truncation");
}

#[test]
fn keeps_the_finest_volume_level_when_the_budget_holds_it() {
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
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = ladder_view(&view_texture, &camera, &scene);
    let info = stream_levels(&gpu, &mut renderer, &view, &volumes);
    assert_eq!(info.samples, SAMPLES, "the roomy view never resolved");
    assert_eq!(info.view.level, 0, "a roomy budget coarsens the volume");
    assert_eq!(info.view.visible, info.view.demand);
    assert!(!info.truncated);
    holds_a_settled_frame(&gpu, &mut renderer, &view);
}

fn holds_a_settled_frame(gpu: &Gpu, renderer: &mut Renderer, view: &View<'_>) {
    for _ in 0..4 {
        let info = renderer.draw(gpu.device(), gpu.queue(), frame(view));
        assert!(info.settled, "an idle fitted frame replans");
        assert_eq!(info.samples, SAMPLES, "an idle fitted frame resamples");
        assert!(!info.truncated, "an idle fitted frame truncates");
    }
}

const TEMPORAL: f32 = 16.0;

fn deviation(first: &[[u8; 4]], second: &[[u8; 4]]) -> f64 {
    assert_eq!(first.len(), second.len(), "two takes cover one surface");
    let total = first
        .iter()
        .zip(second)
        .map(|(left, right)| {
            (0..3)
                .map(|channel| {
                    let delta = f64::from(left[channel]) - f64::from(right[channel]);
                    delta * delta
                })
                .sum::<f64>()
        })
        .sum::<f64>();
    total / (3 * first.len()) as f64
}

struct Take<'a, 'b> {
    texture: &'a wgpu::Texture,
    camera: &'a Camera,
    scene: &'a SceneFrame<'b>,
    time: u32,
    samples: u32,
    history: Option<f32>,
}

fn take(
    gpu: &Gpu,
    renderer: &mut Renderer,
    volume: &Volume,
    view: &Take<'_, '_>,
) -> (Vec<[u8; 4]>, grana_render::FrameInfo) {
    let target = view.texture.create_view(&Default::default());
    let build = || Frame {
        target: &target,
        size: [SIDE, SIDE],
        camera: view.camera,
        viewport: WHOLE,
        time: view.time,
        region: Region::none(),
        clip: Clip::none(),
        exposure: 1.0,
        samples: view.samples,
        scene: view.scene,
        display: grana_render::Display::Export,
        level_cap: usize::MAX,
        temporal: view.history.map(grana_render::Temporal::new),
    };
    let mut info = grana_render::FrameInfo::default();
    for _ in 0..256 {
        info = renderer.draw(gpu.device(), gpu.queue(), build());
        if renderer.requests_pending() {
            if let Some(requests) = next_demand(gpu, renderer) {
                let grid = BrickGrid::new(volume.extent());
                let bricks = requests
                    .volume
                    .iter()
                    .map(|request| (*request, volume.brick(&grid, request.coord)))
                    .collect::<Vec<_>>();
                renderer.upload(gpu.queue(), &bricks);
            }
            continue;
        }
        if info.samples >= view.samples && !info.fading {
            return (read_pixels(gpu, view.texture, [SIDE, SIDE]), info);
        }
    }
    panic!("the take never settled: {info:?}");
}

#[test]
fn reuses_the_history_of_a_still_camera() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let single = volume.pyramid();
    let pyramid = Pyramid::new(
        single.levels().to_vec(),
        single.stored(),
        single.channels().to_vec(),
        2,
        1.0,
    );
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
    };
    let target = texture(&gpu, SIDE);
    let mut renderer = build_renderer(&gpu, &pyramid);
    let (_first_pixels, first) = take(
        &gpu,
        &mut renderer,
        &volume,
        &Take {
            texture: &target,
            camera: &camera,
            scene: &scene,
            time: 0,
            samples: 8,
            history: None,
        },
    );
    assert!(!first.seeded, "the first take invents a history");
    let (reused, second) = take(
        &gpu,
        &mut renderer,
        &volume,
        &Take {
            texture: &target,
            camera: &camera,
            scene: &scene,
            time: 1,
            samples: 1,
            history: Some(TEMPORAL),
        },
    );
    assert!(second.seeded, "the still camera discards its history");
    assert!(second.history, "the history is not retained");
    let mut reference = build_renderer(&gpu, &pyramid);
    let (reference_pixels, _) = take(
        &gpu,
        &mut reference,
        &volume,
        &Take {
            texture: &target,
            camera: &camera,
            scene: &scene,
            time: 1,
            samples: 32,
            history: None,
        },
    );
    let mut control = build_renderer(&gpu, &pyramid);
    let (control_pixels, _) = take(
        &gpu,
        &mut control,
        &volume,
        &Take {
            texture: &target,
            camera: &camera,
            scene: &scene,
            time: 1,
            samples: 1,
            history: None,
        },
    );
    let seeded = deviation(&reused, &reference_pixels);
    let fresh = deviation(&control_pixels, &reference_pixels);
    assert!(
        seeded * 4.0 < fresh,
        "a still camera reuses {seeded:.1} against one sample of {fresh:.1}"
    );
}

#[test]
fn reprojects_the_history_of_a_moving_camera() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let single = volume.pyramid();
    let pyramid = Pyramid::new(
        single.levels().to_vec(),
        single.stored(),
        single.channels().to_vec(),
        2,
        1.0,
    );
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let mut channels = channels(&pyramid, &transfers);
    for row in &mut channels {
        row.volume = VolumeStyle::default().with_mode(grana_display::VolumeMode::Isosurface);
    }
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mut moved = camera.clone();
    moved.orbit(glam::Vec2::new(0.1, 0.05));
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
    };
    let target = texture(&gpu, SIDE);
    let mut renderer = build_renderer(&gpu, &pyramid);
    let (_first_pixels, first) = take(
        &gpu,
        &mut renderer,
        &volume,
        &Take {
            texture: &target,
            camera: &camera,
            scene: &scene,
            time: 0,
            samples: 8,
            history: None,
        },
    );
    assert!(!first.seeded, "the first take invents a history");
    let (reused, second) = take(
        &gpu,
        &mut renderer,
        &volume,
        &Take {
            texture: &target,
            camera: &moved,
            scene: &scene,
            time: 1,
            samples: 1,
            history: Some(TEMPORAL),
        },
    );
    assert!(second.seeded, "the moving camera discards its history");
    let mut reference = build_renderer(&gpu, &pyramid);
    let (reference_pixels, _) = take(
        &gpu,
        &mut reference,
        &volume,
        &Take {
            texture: &target,
            camera: &moved,
            scene: &scene,
            time: 1,
            samples: 32,
            history: None,
        },
    );
    let mut control = build_renderer(&gpu, &pyramid);
    let (control_pixels, _) = take(
        &gpu,
        &mut control,
        &volume,
        &Take {
            texture: &target,
            camera: &moved,
            scene: &scene,
            time: 1,
            samples: 1,
            history: None,
        },
    );
    let seeded = deviation(&reused, &reference_pixels);
    let fresh = deviation(&control_pixels, &reference_pixels);
    assert!(
        seeded * 4.0 < fresh,
        "a moving camera reprojects {seeded:.1} against one sample of {fresh:.1}"
    );
}

#[test]
fn drops_the_history_of_another_shot() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let single = volume.pyramid();
    let pyramid = Pyramid::new(
        single.levels().to_vec(),
        single.stored(),
        single.channels().to_vec(),
        2,
        1.0,
    );
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let first_transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let first_channels = channels(&pyramid, &first_transfers);
    let first = SceneFrame {
        versions: versions(),
        channels: &first_channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
    };
    let second_transfers = [TransferFunction::tinted([1.0, 0.3, 0.2])];
    let second_channels = channels(&pyramid, &second_transfers);
    let second = SceneFrame {
        versions: versions(),
        channels: &second_channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
    };
    let target = texture(&gpu, SIDE);
    let mut renderer = build_renderer(&gpu, &pyramid);
    let (settled_pixels, settled) = take(
        &gpu,
        &mut renderer,
        &volume,
        &Take {
            texture: &target,
            camera: &camera,
            scene: &first,
            time: 0,
            samples: 8,
            history: None,
        },
    );
    assert!(!settled.seeded, "the first take invents a history");
    let (reused, dropped) = take(
        &gpu,
        &mut renderer,
        &volume,
        &Take {
            texture: &target,
            camera: &camera,
            scene: &second,
            time: 1,
            samples: 1,
            history: Some(TEMPORAL),
        },
    );
    assert!(!dropped.seeded, "another shot reuses the old history");
    let mut control = build_renderer(&gpu, &pyramid);
    let (control_pixels, _) = take(
        &gpu,
        &mut control,
        &volume,
        &Take {
            texture: &target,
            camera: &camera,
            scene: &second,
            time: 1,
            samples: 1,
            history: None,
        },
    );
    let fresh = deviation(&reused, &control_pixels);
    let stale = deviation(&reused, &settled_pixels);
    assert!(
        fresh * 8.0 < stale,
        "the new shot carries the old frame ({fresh:.1} against {stale:.1})"
    );
}

fn drawn<'a, 'b>(
    target: &'a wgpu::TextureView,
    camera: &'a Camera,
    scene: &'a SceneFrame<'b>,
) -> Frame<'a, 'b> {
    Frame {
        target,
        size: [SIDE, SIDE],
        camera,
        viewport: WHOLE,
        time: 0,
        region: Region::none(),
        clip: Clip::none(),
        exposure: 1.0,
        samples: 8,
        scene,
        display: grana_render::Display::Still,
        level_cap: usize::MAX,
        temporal: None,
    }
}

#[test]
fn separates_data_object_and_style_versions() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let other_transfers = [TransferFunction::tinted([1.0, 0.4, 0.2])];
    let other_channels = channels(&pyramid, &other_transfers);
    let channels = channels(&pyramid, &transfers);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let base = versions();
    let target = texture(&gpu, SIDE);
    let view = target.create_view(&Default::default());
    let mut renderer = build_renderer(&gpu, &pyramid);
    let draw = |renderer: &mut Renderer, scene: &SceneFrame<'_>| {
        renderer.draw(gpu.device(), gpu.queue(), drawn(&view, &camera, scene))
    };
    let first = draw(&mut renderer, &bare_scene(base, &channels));
    assert!(first.rebuilt, "the first frame reused an empty scene");
    assert_eq!(
        first.samples, 1,
        "the first frame reused accumulated samples"
    );
    let still = draw(&mut renderer, &bare_scene(base, &channels));
    assert!(
        !still.rebuilt,
        "an unchanged scene rebuilt the object buffers"
    );
    assert_eq!(still.samples, 2, "an unchanged scene dropped its samples");
    let mut shifted = camera.clone();
    shifted.orbit(glam::Vec2::new(0.2, 0.1));
    let orbiting = renderer.draw(
        gpu.device(),
        gpu.queue(),
        drawn(&view, &shifted, &bare_scene(base, &channels)),
    );
    assert!(
        !orbiting.rebuilt,
        "a moved camera rebuilt the object buffers"
    );
    let styled = Versions::new(base.data(), base.objects(), revision());
    let restyled = draw(&mut renderer, &bare_scene(styled, &channels));
    assert!(
        restyled.rebuilt,
        "a style version kept the previous object styles"
    );
    assert_eq!(restyled.samples, 1, "a style version kept the samples");
    let channel = draw(&mut renderer, &bare_scene(styled, &other_channels));
    assert!(
        !channel.rebuilt,
        "a channel style rebuilt the object buffers"
    );
    assert_eq!(channel.samples, 1, "a channel style kept the samples");
    let reloaded = draw(
        &mut renderer,
        &bare_scene(
            Versions::new(revision(), styled.objects(), styled.style()),
            &channels,
        ),
    );
    assert!(
        !reloaded.rebuilt,
        "a data version rebuilt the object buffers"
    );
    assert_eq!(reloaded.samples, 1, "a data version kept the samples");
    let moved = draw(
        &mut renderer,
        &bare_scene(
            Versions::new(styled.data(), revision(), styled.style()),
            &channels,
        ),
    );
    assert!(
        moved.rebuilt,
        "an object version kept the previous geometry"
    );
}

fn bare_scene<'a>(versions: Versions, channels: &'a [grana_render::ChannelRow]) -> SceneFrame<'a> {
    SceneFrame {
        versions,
        channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
    }
}

#[test]
fn keys_the_scene_by_its_render_versions() {
    let volume = two_channel_volume();
    let pyramid = volume.pyramid();
    let mut scene = Scene::new(Dataset::synthetic("keyed", pyramid));
    let set = scene.add_object(ObjectKind::Spots, 0);
    let statistics = SampleStatistics::new(60.0, 40.0, 80.0, 2.0, 60.0);
    scene.install(
        set,
        0,
        ObjectFrame::Spots(SpotSet::new(
            0,
            vec![Spot::new([32.0, 32.0, 16.0], 4.0, 1.0)],
            IntensityTable::new(2, vec![statistics; 2]),
        )),
    );
    let base = grana_engine::key(&scene);
    assert_eq!(base, grana_engine::key(&scene), "an idle scene rekeyed");
    scene.select_channel(1);
    assert_eq!(
        grana_engine::key(&scene),
        base,
        "a channel selection rebuilt the render state"
    );
    scene
        .objects
        .get_mut(set)
        .expect("the set exists")
        .set_color([0.2, 0.8, 0.3]);
    let styled = grana_engine::key(&scene);
    assert_ne!(styled, base, "an object colour kept the render state");
    scene.select(Some(Selection {
        set,
        item: Item::Object(0),
    }));
    let selected = grana_engine::key(&scene);
    assert_ne!(selected, styled, "a selection kept the render state");
    scene.channels[1].opacity = 0.3;
    assert_ne!(
        grana_engine::key(&scene),
        selected,
        "a channel style kept the render state"
    );
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
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
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
            display: grana_render::Display::Export,
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
            display: grana_render::Display::Export,
        },
        &target,
        &volume,
    );
    assert_eq!(
        blank.painted(WHOLE),
        0,
        "a plane away from the sphere samples it anyway"
    );
}

fn box_mesh(center: [f32; 3], half: f32) -> Mesh {
    let mut vertices = Vec::new();
    for corner in 0..8u32 {
        let position = [
            center[0] + if corner & 1 == 0 { -half } else { half },
            center[1] + if corner & 2 == 0 { -half } else { half },
            center[2] + if corner & 4 == 0 { -half } else { half },
        ];
        vertices.push(MeshVertex::new(position, [0.0, 0.0, 1.0], 0));
    }
    let indices = vec![
        0, 2, 3, 0, 3, 1, 4, 5, 7, 4, 7, 6, 0, 1, 5, 0, 5, 4, 2, 6, 7, 2, 7, 3, 0, 4, 6, 0, 6, 2,
        1, 3, 7, 1, 7, 5,
    ];
    Mesh::new(vertices, indices)
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
        if let Some(picked) = renderer.poll_pick(gpu.device()) {
            return picked;
        }
    }
    panic!("the pick never resolved");
}

#[test]
fn cuts_the_object_surfaces_of_a_slice() {
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
    let styles = [Style::new([1.0, 0.1, 0.1], true, false, 1.0)];
    let layers = [Layer {
        key: LayerKey::stable(1, 0, 1),
        mesh: Arc::new(PackedMesh::new(box_mesh([0.0, 0.0, 0.1], 1.0))),
        style_start: 0,
        style_count: 1,
    }];
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &styles,
        layers: &layers,
        splats: Splats::empty(),
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
        display: grana_render::Display::Still,
    };
    let shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
    let painted = shot.painted(WHOLE);
    assert!(
        (400..4_000).contains(&painted),
        "the section paints {painted} pixels of the slice"
    );
    assert!(
        shot.hue(WHOLE, Hue::Red) > 300,
        "the section lost the surface colour"
    );
    let middle = [SIDE / 2 - 1, SIDE / 2 - 1, SIDE / 2 + 1, SIDE / 2 + 1];
    assert_eq!(
        shot.painted(middle),
        4,
        "the section leaves its interior open"
    );
    let inside = project(&camera, [0.0, 0.0, 0.1]);
    assert!(renderer.request_pick(gpu.device(), gpu.queue(), inside));
    assert_eq!(picked(&gpu, &mut renderer), PickId::Style(0));
    let outside = project(&camera, [2.5, 2.5, 0.1]);
    assert!(renderer.request_pick(gpu.device(), gpu.queue(), outside));
    assert_eq!(picked(&gpu, &mut renderer), PickId::None);
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
        Splat::point([0.0, 0.0, 0.1], 0.3, [1.0, 0.2, 0.2], 1.0),
        Splat::point([-1.0, 0.0, -1.0], 0.3, [0.2, 1.0, 0.2], 1.0),
    ];
    let chunks = [SplatChunk::new(0, markers.to_vec())];
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::new(&chunks),
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
        display: grana_render::Display::Still,
    };
    settle(&gpu, &mut renderer, &view, &volume);
    let on_plane = project(&camera, [0.0, 0.0, 0.1]);
    assert!(renderer.request_pick(gpu.device(), gpu.queue(), on_plane));
    assert_eq!(picked(&gpu, &mut renderer), PickId::Splat(0));
    let off_plane = project(&camera, [-1.0, 0.0, -1.0]);
    assert!(renderer.request_pick(gpu.device(), gpu.queue(), off_plane));
    assert_eq!(picked(&gpu, &mut renderer), PickId::None);
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
            Splat::point(near, 0.4, [1.0, 0.1, 0.1], alpha),
            Splat::point(far, 0.4, [0.1, 0.2, 1.0], alpha),
        ]
    };
    let solid_chunks = [SplatChunk::new(0, splats(1.0))];
    let faint_chunks = [SplatChunk::new(0, splats(0.25))];
    let solid_scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::new(&solid_chunks),
    };
    let faint_scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::new(&faint_chunks),
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
        display: grana_render::Display::Export,
    };
    let faint_view = View {
        target: &view_texture,
        camera: &camera,
        region: Region::none(),
        scene: &faint_scene,
        viewport: WHOLE,
        display: grana_render::Display::Export,
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
fn keeps_the_history_of_a_panning_slice_and_drops_it_for_another_plane() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let single = volume.pyramid();
    let pyramid = Pyramid::new(
        single.levels().to_vec(),
        single.stored(),
        single.channels().to_vec(),
        2,
        1.0,
    );
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        versions: versions(),
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        splats: Splats::empty(),
    };
    let target = texture(&gpu, SIDE);
    let camera = slice_camera(&pyramid, 16);
    let mut panned = camera.clone();
    panned.pan(glam::Vec2::new(6.0, -4.0), SIDE as f32);
    let mut renderer = build_renderer(&gpu, &pyramid);
    let (_, first) = take(
        &gpu,
        &mut renderer,
        &volume,
        &Take {
            texture: &target,
            camera: &camera,
            scene: &scene,
            time: 0,
            samples: 8,
            history: None,
        },
    );
    assert!(!first.seeded, "the first take invents a history");
    let (_, second) = take(
        &gpu,
        &mut renderer,
        &volume,
        &Take {
            texture: &target,
            camera: &panned,
            scene: &scene,
            time: 1,
            samples: 1,
            history: Some(TEMPORAL),
        },
    );
    assert!(second.seeded, "a panned slice discards its history");
    assert!(second.history, "the panned slice holds no history");
    let moved = slice_camera(&pyramid, 18);
    let (_, third) = take(
        &gpu,
        &mut renderer,
        &volume,
        &Take {
            texture: &target,
            camera: &moved,
            scene: &scene,
            time: 1,
            samples: 1,
            history: Some(TEMPORAL),
        },
    );
    assert!(
        !third.seeded,
        "another plane reuses the history of the old one"
    );
}
