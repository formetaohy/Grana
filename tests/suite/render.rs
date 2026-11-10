use std::sync::Arc;

use glam::Vec3;
use grana_display::{
    Camera, SectionCamera, SectionProjection, Shading, TransferFunction, VolumeStyle,
};
use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_render::{
    Frame, Layer, LayerKey, Lens, PaneFrame, Renderer, Requests, SceneFrame, Style,
};
use grana_volume::{
    Basis, BrickCoord, BrickData, BrickGrid, BrickRequest, Channel, Clip, Extent, Level,
    PADDED_SIDE, Pyramid, Region, Spacing, Volume, VoxelData,
};

use crate::support::{Gpu, Hue, Snapshot, gpu, read_pixels, spheres};

const SIDE: u32 = 128;
const WIDE: u32 = 1536;
const SAMPLES: u32 = 4;
const BUDGET: u64 = 256 << 20;
const WHOLE: [u32; 4] = [0, 0, SIDE, SIDE];
const CORNER: [u32; 4] = [0, 0, 1, 1];
const EMPTY: Extent = Extent::new(16, 16, 16);

fn panes<'a>(
    camera: &'a Camera,
    section: &'a SectionCamera,
    volume_region: [u32; 4],
    section_region: [u32; 4],
) -> [Option<PaneFrame<'a>>; grana_render::PANE_COUNT] {
    [
        Some(PaneFrame::Volume {
            camera,
            lens: Lens {
                camera,
                aspect: volume_region[2].max(1) as f32 / volume_region[3].max(1) as f32,
            },
            region: volume_region,
        }),
        Some(PaneFrame::Section {
            section,
            region: section_region,
        }),
        None,
        None,
    ]
}

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
        panes: panes(
            view.camera,
            view.section,
            view.volume_region,
            view.section_region,
        ),
        time: 0,
        region: view.region,
        clip: Clip::none(),
        exposure: 1.0,
        samples: SAMPLES,
        scene: view.scene,
        pane_channels: [grana_display::ChannelMask::NONE; grana_render::PANE_COUNT],
        pane_quality: [grana_display::Quality::Balanced; grana_render::PANE_COUNT],
        prefetch: false,
        level_cap: usize::MAX,
        assured: view.assured,
    }
}

fn wide<'a, 'b>(
    target: &'a wgpu::TextureView,
    camera: &'a Camera,
    scene: &'a SceneFrame<'b>,
    assured: bool,
) -> Frame<'a, 'b> {
    Frame {
        target,
        size: [WIDE, WIDE],
        panes: [
            Some(PaneFrame::Volume {
                camera,
                lens: Lens {
                    camera,
                    aspect: 1.0,
                },
                region: [0, 0, WIDE, WIDE],
            }),
            None,
            None,
            None,
        ],
        time: 0,
        region: Region::none(),
        clip: Clip::none(),
        exposure: 1.0,
        samples: SAMPLES,
        scene,
        pane_channels: [grana_display::ChannelMask::NONE; grana_render::PANE_COUNT],
        pane_quality: [grana_display::Quality::Balanced; grana_render::PANE_COUNT],
        prefetch: false,
        level_cap: usize::MAX,
        assured,
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
                .chain(&requests.section)
                .map(|request| (*request, volume.brick(&grid, request.coord)))
                .collect::<Vec<_>>();
            renderer.upload(gpu.queue(), &bricks);
            latest = requests;
        }
    }
    panic!("the stream never settled");
}

fn lit(pixels: &[[u8; 4]], rect: [u32; 4]) -> usize {
    let [left, top, right, bottom] = rect;
    let mut count = 0;
    for y in top..bottom {
        for x in left..right {
            let pixel = pixels[(y * SIDE + x) as usize];
            if pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32 > 200 {
                count += 1;
            }
        }
    }
    count
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
    }
    profile
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
        assured: true,
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
    assert!(requests.panes[0].visible > 0, "the view sees no brick");
    assert_eq!(
        requests.panes[0].resident, requests.panes[0].demand,
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
        requests.panes[0].visible > 0,
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
    assert!(requests.panes[1].visible > 0, "the section sees no brick");
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
        requests.panes[1].visible > 0,
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
        info.panes[1].level, 0,
        "the sparse slab settled at level {}",
        info.panes[1].level
    );
    assert!(
        info.panes[1].visible <= 1,
        "the probed slab measures {} visible bricks",
        info.panes[1].visible
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

fn stream_wide(
    gpu: &Gpu,
    renderer: &mut Renderer,
    volume: &Volume,
    target: &wgpu::TextureView,
    scene: &SceneFrame<'_>,
    camera: &Camera,
) -> grana_render::FrameInfo {
    let grid = BrickGrid::new(volume.extent());
    renderer.draw(
        gpu.device(),
        gpu.queue(),
        wide(target, camera, scene, false),
    );
    for _ in 0..64 {
        if let Some(requests) = next_demand(gpu, renderer) {
            let bricks = requests
                .volume
                .iter()
                .chain(&requests.section)
                .map(|request| (*request, volume.brick(&grid, request.coord)))
                .collect::<Vec<_>>();
            renderer.upload(gpu.queue(), &bricks);
        }
        let info = renderer.draw(
            gpu.device(),
            gpu.queue(),
            wide(target, camera, scene, false),
        );
        if !renderer.requests_pending() && info.samples >= SAMPLES && info.settled {
            return info;
        }
    }
    panic!("the wide frame never settled");
}

fn wide_brightness(pixels: &[[u8; 4]]) -> f64 {
    let total = pixels
        .iter()
        .map(|pixel| pixel[0] as u64 + pixel[1] as u64 + pixel[2] as u64)
        .sum::<u64>();
    total as f64 / pixels.len() as f64
}

#[test]
fn scales_a_moving_volume_to_lower_resolution() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = sphere_volume();
    let pyramid = volume.pyramid();
    let transfers = [TransferFunction::tinted([0.4, 0.8, 1.0])];
    let channels = channels(&pyramid, &transfers);
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, WIDE);
    let view = target.create_view(&Default::default());
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let resting = stream_wide(&gpu, &mut renderer, &volume, &view, &scene, &camera);
    assert_eq!(
        resting.volume_scale[0], 1,
        "a resting view resolves at full volume resolution"
    );
    let settled = read_pixels(&gpu, &target, [WIDE, WIDE]);
    assert!(
        wide_brightness(&settled) > 1.0,
        "the resting volume rendered nothing"
    );
    let mut moved = Camera::new(35.0);
    moved.fit(Vec3::from(volume.world_extent()), 1.0);
    let mut state = moved.state();
    state.yaw = 0.35;
    moved.restore(state);
    let moving = renderer.draw(
        gpu.device(),
        gpu.queue(),
        wide(&view, &moved, &scene, false),
    );
    assert_eq!(
        moving.volume_scale[0], 2,
        "a moving view halves its volume resolution"
    );
    let coarse = read_pixels(&gpu, &target, [WIDE, WIDE]);
    let returning = stream_wide(&gpu, &mut renderer, &volume, &view, &scene, &moved);
    assert_eq!(
        returning.volume_scale[0], 1,
        "the volume returns to full resolution once the view rests"
    );
    let fine = read_pixels(&gpu, &target, [WIDE, WIDE]);
    let coarse_mean = wide_brightness(&coarse);
    let fine_mean = wide_brightness(&fine);
    assert!(
        fine_mean > 1.0,
        "the settled volume rendered nothing: {fine_mean}"
    );
    assert!(
        (coarse_mean - fine_mean).abs() < 0.5 * fine_mean,
        "the scaled volume diverged from the settled one: {coarse_mean} against {fine_mean}"
    );
    let mut other = Camera::new(35.0);
    other.fit(Vec3::from(volume.world_extent()), 1.0);
    let mut state = other.state();
    state.yaw = -0.4;
    other.restore(state);
    let assured = renderer.draw(gpu.device(), gpu.queue(), wide(&view, &other, &scene, true));
    assert_eq!(
        assured.volume_scale[0], 1,
        "an assured frame keeps the full volume resolution"
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
                .chain(&requests.section)
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
    section: &'a SectionCamera,
    scene: &'a SceneFrame<'a>,
) -> View<'a> {
    View {
        target,
        camera,
        section,
        region: Region::none(),
        scene,
        volume_region: WHOLE,
        section_region: CORNER,
        assured: false,
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
    let section = SectionCamera::new();
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut renderer = Renderer::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        1 << 20,
    );
    renderer.set_dataset(gpu.device(), &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let view = ladder_view(&view_texture, &camera, &section, &scene);
    let info = stream_levels(&gpu, &mut renderer, &view, &volumes);
    assert_eq!(info.samples, SAMPLES, "the constrained view never resolved");
    assert!(
        info.panes[0].level > 0,
        "the budget still asks for the finest level"
    );
    assert_eq!(
        info.panes[0].visible, info.panes[0].demand,
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
    let section = SectionCamera::new();
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let panes = [
        Some(PaneFrame::Volume {
            camera: &camera,
            lens: Lens {
                camera: &camera,
                aspect: 1.0,
            },
            region: WHOLE,
        }),
        None,
        None,
        None,
    ];
    let detail = grana_render::residency(&pyramid, &panes, Region::none());
    assert_eq!(
        detail.panes[0].expect("the volume is scheduled").level,
        0,
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
    let mut view = ladder_view(&view_texture, &camera, &section, &scene);
    view.assured = true;
    let info = stream_levels(&gpu, &mut renderer, &view, &volumes);
    assert_eq!(info.samples, SAMPLES, "the sized export never resolved");
    assert_eq!(
        info.panes[0].level, 0,
        "the sized export coarsens its level"
    );
    assert_eq!(
        info.panes[0].visible, info.panes[0].demand,
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
    let view = ladder_view(&view_texture, &camera, &section, &scene);
    let info = stream_levels(&gpu, &mut renderer, &view, &volumes);
    assert_eq!(info.samples, SAMPLES, "the roomy view never resolved");
    assert_eq!(info.panes[0].level, 0, "a roomy budget coarsens the volume");
    assert_eq!(info.panes[0].visible, info.panes[0].demand);
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

#[test]
fn renders_a_section_pane_without_a_volume() {
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
    let mut section = SectionCamera::new();
    section.fit(
        Vec3::from(volume.world_extent()),
        Vec3::from(pyramid.level(0).voxel()),
        1.0,
    );
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
    let panes = [
        None,
        Some(PaneFrame::Section {
            section: &section,
            region: WHOLE,
        }),
        None,
        None,
    ];
    let build = || Frame {
        target: &view_texture,
        size: [SIDE, SIDE],
        panes,
        time: 0,
        region: Region::none(),
        clip: Clip::none(),
        exposure: 1.0,
        samples: SAMPLES,
        scene: &scene,
        pane_channels: [grana_display::ChannelMask::NONE; grana_render::PANE_COUNT],
        pane_quality: [grana_display::Quality::Balanced; grana_render::PANE_COUNT],
        prefetch: false,
        level_cap: usize::MAX,
        assured: false,
    };
    let requests = settle_with(&gpu, &mut renderer, build, &volume);
    assert!(
        requests.panes[1].visible > 0,
        "the section pane sees no brick"
    );
    let pixels = read_pixels(&gpu, &target, [SIDE, SIDE]);
    let centre = pixels[(SIDE * SIDE / 2 + SIDE / 2) as usize];
    let corner = pixels[12 * SIDE as usize + 12];
    let centre = centre[0] as u32 + centre[1] as u32 + centre[2] as u32;
    let corner = corner[0] as u32 + corner[1] as u32 + corner[2] as u32;
    assert!(
        centre > corner + 90,
        "the lone section pane paints nothing at its centre: {centre} against {corner}"
    );
}

fn hue_pixels(pixels: &[[u8; 4]], rect: [u32; 4], channel: usize) -> usize {
    let [left, top, right, bottom] = rect;
    let mut count = 0;
    for y in top..bottom {
        for x in left..right {
            let pixel = pixels[(y * SIDE + x) as usize];
            let value = u32::from(pixel[channel]);
            if value > 24
                && (0..3).all(|other| other == channel || value > u32::from(pixel[other]) * 2)
            {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn keeps_pane_channel_sets_independent() {
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
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        markers: &[],
    };
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(volume.world_extent()), 1.0);
    let mut renderer = build_renderer(&gpu, &pyramid);
    let target = texture(&gpu, SIDE);
    let view_texture = target.create_view(&Default::default());
    let left_region = [0, 0, SIDE / 2, SIDE];
    let right_region = [SIDE / 2, 0, SIDE / 2, SIDE];
    let left_probe = [0, 0, SIDE / 2, SIDE];
    let right_probe = [SIDE / 2, 0, SIDE, SIDE];
    let lens = Lens {
        camera: &camera,
        aspect: 0.5,
    };
    let panes = [
        Some(PaneFrame::Volume {
            camera: &camera,
            lens,
            region: left_region,
        }),
        Some(PaneFrame::Volume {
            camera: &camera,
            lens,
            region: right_region,
        }),
        None,
        None,
    ];
    let mut left = grana_display::ChannelMask::NONE;
    left.set(1, false);
    let mut right = grana_display::ChannelMask::NONE;
    right.set(0, false);
    let build = || Frame {
        target: &view_texture,
        size: [SIDE, SIDE],
        panes,
        time: 0,
        region: Region::none(),
        clip: Clip::none(),
        exposure: 1.0,
        samples: SAMPLES,
        scene: &scene,
        pane_channels: [
            left,
            right,
            grana_display::ChannelMask::NONE,
            grana_display::ChannelMask::NONE,
        ],
        pane_quality: [grana_display::Quality::Balanced; grana_render::PANE_COUNT],
        prefetch: false,
        level_cap: usize::MAX,
        assured: false,
    };
    settle_with(&gpu, &mut renderer, build, &volume);
    let pixels = read_pixels(&gpu, &target, [SIDE, SIDE]);
    let blue_left = hue_pixels(&pixels, left_probe, 2);
    let green_left = hue_pixels(&pixels, left_probe, 1);
    let blue_right = hue_pixels(&pixels, right_probe, 2);
    let green_right = hue_pixels(&pixels, right_probe, 1);
    assert!(
        blue_left > green_left * 4,
        "the left pane hides its green channel: {blue_left} blue against {green_left} green"
    );
    assert!(
        green_right > blue_right * 4,
        "the right pane hides its blue channel: {green_right} green against {blue_right} blue"
    );
}

#[test]
fn keeps_two_volume_panes_independent() {
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
    let world = Vec3::from(volume.world_extent());
    let mut left = Camera::new(35.0);
    left.fit(world, 1.0);
    let mut right = Camera::new(35.0);
    right.restore(grana_display::CameraState {
        pivot: [10.0, 0.0, 0.0],
        yaw: 0.0,
        pitch: 0.0,
        distance: 1.0,
        fov_y: 0.6,
        projection: grana_display::CameraProjection::Perspective,
        aperture: 0.0,
        focus: 1.0,
    });
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
    let left_region = [0, 0, SIDE / 2, SIDE];
    let right_region = [SIDE / 2, 0, SIDE / 2, SIDE];
    let panes = [
        Some(PaneFrame::Volume {
            camera: &left,
            lens: Lens {
                camera: &left,
                aspect: left_region[2] as f32 / left_region[3] as f32,
            },
            region: left_region,
        }),
        Some(PaneFrame::Volume {
            camera: &right,
            lens: Lens {
                camera: &right,
                aspect: right_region[2] as f32 / right_region[3] as f32,
            },
            region: right_region,
        }),
        None,
        None,
    ];
    let build = || Frame {
        target: &view_texture,
        size: [SIDE, SIDE],
        panes,
        time: 0,
        region: Region::none(),
        clip: Clip::none(),
        exposure: 1.0,
        samples: SAMPLES,
        scene: &scene,
        pane_channels: [grana_display::ChannelMask::NONE; grana_render::PANE_COUNT],
        pane_quality: [grana_display::Quality::Balanced; grana_render::PANE_COUNT],
        prefetch: false,
        level_cap: usize::MAX,
        assured: false,
    };
    settle_with(&gpu, &mut renderer, build, &volume);
    let info = renderer.draw(gpu.device(), gpu.queue(), build());
    println!(
        "info {:?} settled {} truncated {}",
        info.panes, info.settled, info.truncated
    );
    let pixels = read_pixels(&gpu, &target, [SIDE, SIDE]);
    let lit_left = lit(&pixels, [0, 0, SIDE / 2, SIDE]);
    let lit_right = lit(&pixels, [SIDE / 2, 0, SIDE, SIDE]);
    assert!(
        lit_left > (SIDE * SIDE / 64) as usize,
        "the left pane is empty"
    );
    assert!(
        lit_right * 8 < lit_left,
        "the second pane reuses the first camera: {lit_left} against {lit_right}"
    );
}
