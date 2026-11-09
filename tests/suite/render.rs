use std::sync::Arc;

use glam::Vec3;
use grana_display::{Camera, SectionCamera, Shading, TransferFunction};
use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_render::{Frame, Layer, LayerKey, Renderer, Requests, SceneFrame, Style};
use grana_volume::{BrickGrid, Channel, Clip, Extent, Pyramid, Region, Spacing, Volume, VoxelData};

use crate::support::{Gpu, blue, gpu, green, lit, read_pixels, red, spheres};

const SIDE: u32 = 128;
const SAMPLES: u32 = 4;
const BUDGET: u64 = 256 << 20;
const WHOLE: [u32; 4] = [0, 0, SIDE, SIDE];
const CORNER: [u32; 4] = [0, 0, 1, 1];
const EMPTY: Extent = Extent::new(16, 16, 16);

struct View<'a> {
    target: &'a wgpu::TextureView,
    camera: &'a Camera,
    section: &'a SectionCamera,
    region: Region,
    scene: &'a SceneFrame<'a>,
    volume_region: [u32; 4],
    section_region: [u32; 4],
}

fn frame<'a>(view: &View<'a>) -> Frame<'a, 'a> {
    Frame {
        target: view.target,
        size: [SIDE, SIDE],
        volume_region: view.volume_region,
        section_region: view.section_region,
        time: 0,
        camera: view.camera,
        section: view.section,
        region: view.region,
        clip: Clip::none(),
        exposure: 1.0,
        samples: SAMPLES,
        scene: view.scene,
        prefetch: false,
        level_cap: usize::MAX,
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

fn channels<'a>(
    pyramid: &Pyramid,
    transfers: &'a [TransferFunction],
) -> Vec<grana_render::ChannelRow<'a>> {
    pyramid
        .channels()
        .iter()
        .zip(transfers)
        .map(|(channel, transfer)| grana_render::ChannelRow {
            visible: true,
            opacity: 1.0,
            window: channel.window(),
            transfer,
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
    let mut latest = Requests::default();
    for _ in 0..64 {
        let mut info = renderer.draw(gpu.device(), gpu.queue(), frame(view));
        if !renderer.requests_pending() {
            while info.samples < SAMPLES || info.fading {
                info = renderer.draw(gpu.device(), gpu.queue(), frame(view));
            }
            return latest;
        }
        if let Some(requests) = next_demand(gpu, renderer) {
            let grid = BrickGrid::new(volume.extent());
            for request in requests.volume.iter().chain(&requests.section) {
                let brick = volume.brick(&grid, request.coord);
                let _ = renderer.upload(gpu.queue(), *request, &brick);
            }
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

fn profile(gpu: &Gpu, renderer: &mut Renderer, view: &View<'_>) -> grana_render::FrameProfile {
    let mut profile = grana_render::FrameProfile::default();
    for _ in 0..16 {
        profile = renderer
            .draw(gpu.device(), gpu.queue(), frame(view))
            .profile;
        if profile.measured() {
            break;
        }
        gpu.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("failed to wait for the gpu");
    }
    profile
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
    let section = SectionCamera::new();
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        section: &section,
        region: Region::none(),
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
    };
    let requests = settle(&gpu, &mut renderer, &view, &volume);
    assert!(requests.volume_view.visible > 0, "the view sees no brick");
    assert_eq!(
        requests.volume_view.resident, requests.volume_view.demand,
        "the stream never completed"
    );
    let pixels = read_pixels(&gpu, &target, [SIDE, SIDE]);
    assert!(
        lit(&pixels) > (SIDE * SIDE / 20) as usize,
        "the volume barely rendered: {} lit pixels",
        lit(&pixels)
    );
    assert!(blue(&pixels) > 100, "the blue channel barely rendered");
    assert!(green(&pixels) > 100, "the green channel barely rendered");
    let profile = profile(&gpu, &mut renderer, &view);
    assert!(profile.target > 0.0, "the frame carries no budget");
    assert!(profile.penalty >= 1.0, "the sampling is finer than asked");
    if renderer.profiled() {
        assert!(profile.measured(), "the frame is not timed");
        assert!(profile.stage(grana_render::Stage::Volume) > 0.0);
    }
}

#[test]
fn holds_the_frame_budget() {
    let mut budget = grana_render::Budget::new(16.0);
    assert_eq!(budget.penalty(), 1.0, "a fresh budget samples as asked");
    for _ in 0..64 {
        budget.observe(32.0);
    }
    assert!(budget.measured() > 16.0, "the overrun is not smoothed");
    assert!(budget.penalty() > 1.0, "an overrun never coarsens");
    assert_eq!(budget.penalty(), 4.0, "the penalty is unbounded");
    for _ in 0..64 {
        budget.observe(1.0);
    }
    assert!(budget.measured() < 16.0, "the headroom is not smoothed");
    assert_eq!(budget.penalty(), 1.0, "spare budget never refines again");
}

#[test]
fn reveals_a_section_plane() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let world = Vec3::from(volume.world_extent());
    let voxel = Vec3::new(
        volume.spacing().x(),
        volume.spacing().y(),
        volume.spacing().z(),
    );
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    let mut section = SectionCamera::new();
    section.fit(world, voxel, 1.0);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        section: &section,
        region: Region::none(),
        scene: &scene,
        volume_region: CORNER,
        section_region: WHOLE,
    };
    let requests = settle(&gpu, &mut renderer, &view, &volume);
    assert!(
        requests.section_view.visible > 0,
        "the section sees no brick"
    );
    let pixels = read_pixels(&gpu, &target, [SIDE, SIDE]);
    assert!(lit(&pixels) > 500, "the section barely rendered");
}

#[test]
fn reveals_an_oblique_section_plane() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let world = Vec3::from(volume.world_extent());
    let voxel = Vec3::new(
        volume.spacing().x(),
        volume.spacing().y(),
        volume.spacing().z(),
    );
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    let mut section = SectionCamera::new();
    section.set_orientation(0.7, 0.45);
    section.fit(world, voxel, 1.0);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        section: &section,
        region: Region::none(),
        scene: &scene,
        volume_region: CORNER,
        section_region: WHOLE,
    };
    let requests = settle(&gpu, &mut renderer, &view, &volume);
    assert!(
        requests.section_view.visible > 0,
        "the oblique section sees no brick"
    );
    let pixels = read_pixels(&gpu, &target, [SIDE, SIDE]);
    assert!(
        lit(&pixels) > 500,
        "the oblique section barely rendered: {} lit pixels",
        lit(&pixels)
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
    let section = SectionCamera::new();
    let mesh = PackedMesh::new(quad(8.0));
    let styles = [Style::new([1.0, 0.1, 0.1], true, false, 1.0)];
    let layers = [Layer {
        key: LayerKey::stable(1, 0, 1),
        mesh: Arc::new(mesh),
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
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        section: &section,
        region: Region::none(),
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
    };
    settle(&gpu, &mut renderer, &view, &volume);
    let pixels = read_pixels(&gpu, &target, [SIDE, SIDE]);
    assert!(
        red(&pixels) > 1_000,
        "the mesh barely rendered: {} red pixels",
        red(&pixels)
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
    let section = SectionCamera::new();
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let open = View {
        target: &view_texture,
        camera: &camera,
        section: &section,
        region: Region::none(),
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
    };
    settle(&gpu, &mut renderer, &open, &volume);
    let pixels = read_pixels(&gpu, &target, [SIDE, SIDE]);
    assert!(blue(&pixels) > 100, "the left ball barely rendered");
    assert!(green(&pixels) > 100, "the right ball barely rendered");
    let across = Vec3::new(1.0, 0.0, 2.0).normalize();
    let down = Vec3::new(-2.0, 0.0, 1.0).normalize();
    let region = Region::new(
        -1.1 * across,
        [across, Vec3::Y, down],
        Vec3::new(1.1, 2.4, 4.8),
    );
    let clipped = View {
        region,
        volume_region: WHOLE,
        section_region: CORNER,
        ..open
    };
    settle(&gpu, &mut renderer, &clipped, &volume);
    let pixels = read_pixels(&gpu, &target, [SIDE, SIDE]);
    assert!(
        blue(&pixels) > 50,
        "the oriented region hid the kept ball: {} pixels",
        blue(&pixels)
    );
    assert!(
        green(&pixels) < 20,
        "the oriented region kept the dropped ball: {} pixels",
        green(&pixels)
    );
}
