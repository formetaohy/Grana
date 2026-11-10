use std::sync::Arc;

use glam::Vec3;
use grana_display::{
    Camera, SectionCamera, SectionProjection, Shading, TransferFunction, VolumeStyle,
};
use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_render::{Frame, Layer, LayerKey, Renderer, Requests, SceneFrame, Style};
use grana_volume::{
    Basis, BrickCoord, BrickData, BrickGrid, BrickRequest, Channel, Clip, Extent, Level,
    PADDED_SIDE, Pyramid, Region, Spacing, Volume, VoxelData,
};

use crate::support::{Gpu, Hue, Snapshot, gpu, read_pixels, spheres};

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
    assured: bool,
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
        assured: view.assured,
        coarsen: 0,
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
            let bricks = requests
                .volume
                .iter()
                .chain(&requests.section)
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
        channels: &rows,
        shading: view.scene.shading,
        step_scale: view.scene.step_scale,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let blank = View {
        target: view.target,
        camera: view.camera,
        section: view.section,
        region: view.region,
        scene: &scene,
        volume_region: view.volume_region,
        section_region: view.section_region,
        assured: view.assured,
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
        assured: false,
    };
    let requests = settle(&gpu, &mut renderer, &view, &volume);
    assert!(requests.volume_view.visible > 0, "the view sees no brick");
    assert_eq!(
        requests.volume_view.resident, requests.volume_view.demand,
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
        "the volume lands outside its pane"
    );
    assert!(
        shot.hue(shot.all(), Hue::Blue) > 100,
        "the blue channel barely rendered"
    );
    assert!(
        shot.hue(shot.all(), Hue::Green) > 100,
        "the green channel barely rendered"
    );
    let profile = profile(&gpu, &mut renderer, &view);
    assert!(profile.target > 0.0, "the frame carries no budget");
    assert!(profile.penalty >= 1.0, "the sampling is finer than asked");
    if renderer.profiled() {
        assert!(profile.measured(), "the frame is not timed");
        assert!(profile.stage(grana_render::Stage::Volume) > 0.0);
    }
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
    let voxel = Vec3::from(level.voxel());
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    let mut section = SectionCamera::new();
    section.fit(world, voxel, 0.5);
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
        volume_region: [0, 0, SIDE / 2, SIDE],
        section_region: [SIDE / 2, 0, SIDE / 2, SIDE],
        assured: false,
    };
    let requests = settle(&gpu, &mut renderer, &view, &volume);
    assert!(
        requests.volume_view.visible > 0,
        "the rotated volume sees no brick"
    );
    let shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
    assert!(
        shot.painted(shot.left()) > (SIDE * SIDE / 64) as usize,
        "the rotated volume barely rendered: {} painted pixels",
        shot.painted(shot.left())
    );
    assert!(
        shot.painted(shot.right()) > (SIDE * SIDE / 64) as usize,
        "the rotated section barely rendered: {} painted pixels",
        shot.painted(shot.right())
    );
    let mut aligned_renderer = build_renderer(&gpu, &aligned);
    let aligned_target = texture(&gpu, SIDE);
    let aligned_texture = aligned_target.create_view(&Default::default());
    let aligned_view = View {
        target: &aligned_texture,
        camera: &camera,
        section: &section,
        region: Region::none(),
        scene: &scene,
        volume_region: [0, 0, SIDE / 2, SIDE],
        section_region: [SIDE / 2, 0, SIDE / 2, SIDE],
        assured: false,
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
        assured: false,
    };
    let requests = settle(&gpu, &mut renderer, &view, &volume);
    assert!(
        requests.section_view.visible > 0,
        "the section sees no brick"
    );
    let shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
    assert!(
        shot.coverage(shot.middle()) > 0.3,
        "the section barely covers its centre: {:.2}",
        shot.coverage(shot.middle())
    );
    assert!(
        shot.contrast(shot.middle()) > 30.0,
        "the section transfer is not calibrated: {:.1}",
        shot.contrast(shot.middle())
    );
    assert_eq!(
        shot.painted([0, 0, SIDE / 8, SIDE / 8]),
        1,
        "the section spills outside its disc"
    );
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
        assured: false,
    };
    let requests = settle(&gpu, &mut renderer, &view, &volume);
    assert!(
        requests.section_view.visible > 0,
        "the oblique section sees no brick"
    );
    let shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
    assert!(
        shot.painted(shot.all()) > 500,
        "the oblique section barely rendered: {} painted pixels",
        shot.painted(shot.all())
    );
    assert!(
        shot.coverage(shot.middle()) > 0.15,
        "the oblique section lands outside its centre: {:.2}",
        shot.coverage(shot.middle())
    );
}

#[test]
fn projects_a_slab_through_the_section() {
    let Some(gpu) = gpu() else {
        return;
    };
    let extent = Extent::new(48, 48, 48);
    let spacing = Spacing::new(0.5, 0.5, 0.5);
    let volume = spheres(
        extent,
        spacing,
        &[([24.0, 24.0, 17.5], 4.0), ([24.0, 24.0, 29.5], 4.0)],
        60_000,
    );
    let pyramid = volume.pyramid();
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
    macro_rules! view {
        () => {
            View {
                target: &view_texture,
                camera: &camera,
                section: &section,
                region: Region::none(),
                scene: &scene,
                volume_region: CORNER,
                section_region: WHOLE,
                assured: false,
            }
        };
    }
    let plane = snapshot(&gpu, &mut renderer, &view!(), &target, &volume);
    assert!(
        plane.painted(plane.all()) < 64,
        "the empty plane painted {} pixels",
        plane.painted(plane.all())
    );
    section.set_projection(SectionProjection::Maximum);
    section.set_thickness(10.0);
    let maximum = snapshot(&gpu, &mut renderer, &view!(), &target, &volume);
    assert!(
        maximum.painted(maximum.all()) > 200,
        "the projection lost the slab: {} painted pixels",
        maximum.painted(maximum.all())
    );
    assert!(
        maximum.coverage(maximum.middle()) > 0.04,
        "the projection lands outside its centre: {:.2}",
        maximum.coverage(maximum.middle())
    );
    section.set_projection(SectionProjection::Average);
    let average = snapshot(&gpu, &mut renderer, &view!(), &target, &volume);
    assert!(
        average.painted(average.all()) > 100,
        "the average lost the slab: {} painted pixels",
        average.painted(average.all())
    );
    assert!(
        maximum.contrast(maximum.all()) > 1.5 * average.contrast(average.all()),
        "the average is as bright as the maximum"
    );
    section.set_projection(SectionProjection::Maximum);
    let assured = View {
        target: &view_texture,
        camera: &camera,
        section: &section,
        region: Region::none(),
        scene: &scene,
        volume_region: CORNER,
        section_region: WHOLE,
        assured: true,
    };
    let info = assure(
        &gpu,
        &mut renderer,
        &assured,
        &volume,
        &BrickGrid::new(volume.extent()),
    );
    assert_eq!(
        info.samples, SAMPLES,
        "the assured projection stopped short"
    );
}

fn empty_brick(channels: usize) -> BrickData {
    BrickData::new(
        (0..channels)
            .map(|_| VoxelData::U16(vec![0; (PADDED_SIDE as usize).pow(3)]))
            .collect(),
    )
}

#[test]
fn probes_a_finer_section_level_for_a_sparse_slab() {
    let Some(gpu) = gpu() else {
        return;
    };
    let extent = Extent::new(128, 128, 64);
    let volume = spheres(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        &[([64.0, 64.0, 32.0], 10.0)],
        60_000,
    );
    let base = volume.pyramid();
    let fine = base.level(0);
    let pyramid = Pyramid::placed(
        vec![fine, fine.coarser(), fine.coarser().coarser()],
        3,
        base.channels().to_vec(),
        1,
        1.0,
        [0.0; 3],
    );
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let world = Vec3::from(fine.world_extent());
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    let mut section = SectionCamera::new();
    section.set_center(Vec3::ZERO);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 1.0,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = Renderer::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        25_000,
    );
    renderer.set_dataset(gpu.device(), &pyramid);
    let grid = BrickGrid::new(extent);
    let occupied = BrickCoord::new(2, 2, 1);
    let mut bricks = (0..grid.count())
        .map(|index| {
            let coord = grid.coord(index);
            let data = if coord == occupied {
                volume.brick(&grid, coord)
            } else {
                empty_brick(1)
            };
            (
                BrickRequest {
                    time: 0,
                    level: 0,
                    coord,
                },
                data,
            )
        })
        .collect::<Vec<_>>();
    let coarse = BrickGrid::new(pyramid.level(1).extent());
    bricks.extend((0..coarse.count()).map(|index| {
        (
            BrickRequest {
                time: 0,
                level: 1,
                coord: coarse.coord(index),
            },
            empty_brick(1),
        )
    }));
    renderer.upload(gpu.queue(), &bricks);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera: &camera,
        section: &section,
        region: Region::none(),
        scene: &scene,
        volume_region: [0, 0, SIDE / 2, SIDE],
        section_region: [SIDE / 2, 0, SIDE / 2, SIDE],
        assured: false,
    };
    let mut info = renderer.draw(gpu.device(), gpu.queue(), frame(&view));
    for _ in 0..64 {
        if let Some(requests) = next_demand(&gpu, &mut renderer) {
            let bricks = requests
                .volume
                .iter()
                .chain(&requests.section)
                .map(|request| {
                    let data = if request.level == 0 {
                        volume.brick(&grid, request.coord)
                    } else {
                        empty_brick(1)
                    };
                    (*request, data)
                })
                .collect::<Vec<_>>();
            renderer.upload(gpu.queue(), &bricks);
        }
        info = renderer.draw(gpu.device(), gpu.queue(), frame(&view));
    }
    assert_eq!(
        info.section.level, 0,
        "the sparse slab settled at level {}",
        info.section.level
    );
    assert!(
        info.section.visible <= 1,
        "the probed slab measures {} visible bricks",
        info.section.visible
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
        assured: false,
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
fn keeps_a_translucent_layer_across_pane_layouts() {
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
    let section = SectionCamera::new();
    let mesh = PackedMesh::new(quad(8.0));
    let styles = [Style::new([1.0, 0.1, 0.1], true, false, 0.5)];
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
    for (index, region) in [WHOLE, [0, 0, SIDE, SIDE / 2], [0, 0, SIDE / 2, SIDE / 2]]
        .into_iter()
        .enumerate()
    {
        let view = View {
            target: &view_texture,
            camera: &camera,
            section: &section,
            region: Region::none(),
            scene: &scene,
            volume_region: region,
            section_region: CORNER,
            assured: false,
        };
        let shot = snapshot(&gpu, &mut renderer, &view, &target, &volume);
        let painted = shot.painted(region);
        assert!(
            painted > 400,
            "the translucent layer faded at {region:?}: {painted} painted pixels"
        );
        let info = renderer.draw(gpu.device(), gpu.queue(), frame(&view));
        assert!(
            info.oit.layers > 0,
            "the translucent layer does not reach the fragment store"
        );
        if index > 0 {
            assert_eq!(
                shot.painted([SIDE - 1, SIDE - 1, SIDE, SIDE]),
                0,
                "the translucent layer spilled outside its pane"
            );
        }
    }
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
        assured: false,
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
    let clipped = View {
        region,
        volume_region: WHOLE,
        section_region: CORNER,
        ..open
    };
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
        gpu.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("failed to wait for the gpu");
        if let Some(requests) = next_demand(gpu, renderer) {
            let bricks = requests
                .volume
                .iter()
                .chain(&requests.section)
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
    section: &SectionCamera,
) -> Vec<[u8; 4]> {
    let scene = SceneFrame {
        channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = build_renderer(gpu, pyramid);
    let target = texture(gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera,
        section,
        region: Region::none(),
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
        assured: true,
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
    let section = SectionCamera::new();
    let first = assured_pixels(&gpu, &pyramid, &volume, &channels, &camera, &section);
    let second = assured_pixels(&gpu, &pyramid, &volume, &channels, &camera, &section);
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
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let section = SectionCamera::new();
    let mut renderer = build_renderer(gpu, &pyramid);
    let target = texture(gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = View {
        target: &view_texture,
        camera,
        section: &section,
        region: Region::none(),
        scene: &scene,
        volume_region: WHOLE,
        section_region: CORNER,
        assured: false,
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
