use glam::Vec3;
use grana_display::{
    Camera, CameraProjection, SectionCamera, Shading, TransferFunction, VolumeStyle,
};
use grana_render::{ChannelRow, Frame, Lens, Marker, PaneFrame, Renderer, Requests, SceneFrame};
use grana_volume::{
    BrickData, BrickGrid, BrickRequest, Clip, Extent, Pyramid, Region, Spacing, Volume,
};

use crate::support::{Gpu, gpu, read_pixels, spheres};

const SIDE: u32 = 384;
const SAMPLES: u32 = 48;
const WHOLE: [u32; 4] = [0, 0, SIDE, SIDE];
const CORNER: [u32; 4] = [0, 0, 1, 1];

#[test]
fn keeps_one_scale_across_depth_and_spreads_the_aperture() {
    let Some(gpu) = gpu() else {
        return;
    };
    let volume = volume();
    let pyramid = volume.pyramid();
    let channels = hidden(&pyramid);
    let section = SectionCamera::new();
    let mut renderer = Renderer::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        256 << 20,
    );
    renderer.set_dataset(gpu.device(), &pyramid);
    let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("grana.optics.target"),
        size: wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::splat(6.0), 1.0);
    let eye = camera.eye();
    let forward = camera.forward();
    let right = camera.right();
    let focus = camera.focus();
    let markers = vec![
        Marker::new(
            (eye + forward * focus - right * 2.0).to_array(),
            0.25,
            [1.0, 0.1, 0.1],
            1.0,
        ),
        Marker::new(
            (eye + forward * (1.5 * focus) + right * 2.0).to_array(),
            0.25,
            [0.1, 1.0, 0.1],
            1.0,
        ),
    ];
    let scene = SceneFrame {
        channels: &channels,
        shading: Shading::default(),
        step_scale: 0.5,
        styles: &[],
        layers: &[],
        markers: &markers,
    };
    let mut render = |camera: &Camera| {
        let frame = Frame {
            target: &view,
            size: [SIDE, SIDE],
            panes: panes(camera, &section, WHOLE, CORNER),
            time: 0,
            region: Region::none(),
            clip: Clip::none(),
            exposure: 1.0,
            samples: SAMPLES,
            scene: &scene,
            prefetch: false,
            level_cap: usize::MAX,
            assured: true,
            coarsen: 0,
        };
        settle(&gpu, &mut renderer, &frame, &volume);
        read_pixels(&gpu, &texture, [SIDE, SIDE])
    };
    let perspective = {
        camera.set_projection(CameraProjection::Perspective);
        camera.set_aperture(0.0);
        spread(&render(&camera))
    };
    let aperture = {
        camera.set_aperture(1.0);
        spread(&render(&camera))
    };
    let parallel = {
        camera.set_projection(CameraProjection::Orthographic);
        camera.set_aperture(0.0);
        spread(&render(&camera))
    };
    assert!(
        perspective.1 * 2 < perspective.0,
        "a parallel projection shrinks the far marker: {perspective:?}"
    );
    assert!(
        parallel.1.abs_diff(parallel.0) * 4 < parallel.0,
        "an orthographic projection keeps one scale across depth: {parallel:?}"
    );
    assert!(
        aperture.1 > 2 * perspective.1,
        "the aperture spreads an off focus marker: {aperture:?} against {perspective:?}"
    );
    assert!(
        aperture.1.abs_diff(aperture.0) * 2 > aperture.0,
        "the focus plane stays sharp under the aperture: {aperture:?}"
    );
}

fn volume() -> Volume {
    let extent = Extent::new(32, 32, 16);
    let spacing = Spacing::new(0.1, 0.1, 0.2);
    spheres(extent, spacing, &[([16.0, 16.0, 8.0], 6.0)], 60_000)
}

fn hidden(pyramid: &Pyramid) -> Vec<ChannelRow> {
    pyramid
        .channels()
        .iter()
        .map(|channel| ChannelRow {
            visible: false,
            opacity: 1.0,
            window: channel.window(),
            transfer: TransferFunction::tinted(channel.tint()),
            volume: VolumeStyle::default(),
        })
        .collect()
}

fn spread(pixels: &[[u8; 4]]) -> (usize, usize) {
    let count = |channel: usize| {
        pixels
            .iter()
            .filter(|pixel| {
                (0..3).all(|other| other == channel || pixel[other] < pixel[channel])
                    && pixel[channel] > 90
            })
            .count()
    };
    (count(0), count(1))
}

fn settle(gpu: &Gpu, renderer: &mut Renderer, frame: &Frame<'_, '_>, volume: &Volume) {
    for _ in 0..64 {
        let mut info = renderer.draw(gpu.device(), gpu.queue(), copy(frame));
        if !renderer.requests_pending() {
            while info.samples < SAMPLES || info.fading {
                info = renderer.draw(gpu.device(), gpu.queue(), copy(frame));
            }
            return;
        }
        if let Some(requests) = demand(gpu, renderer) {
            let grid = BrickGrid::new(volume.extent());
            let bricks = requests
                .volume
                .iter()
                .chain(&requests.section)
                .map(|request| (*request, volume.brick(&grid, request.coord)))
                .collect::<Vec<(BrickRequest, BrickData)>>();
            renderer.upload(gpu.queue(), &bricks);
        }
    }
    panic!("the optics view never settled");
}

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
    ]
}

fn copy<'a, 'b>(frame: &Frame<'a, 'b>) -> Frame<'a, 'b> {
    Frame {
        target: frame.target,
        size: frame.size,
        panes: frame.panes,
        time: frame.time,
        region: frame.region,
        clip: frame.clip,
        exposure: frame.exposure,
        samples: frame.samples,
        scene: frame.scene,
        prefetch: frame.prefetch,
        level_cap: frame.level_cap,
        assured: frame.assured,
        coarsen: frame.coarsen,
    }
}

fn demand(gpu: &Gpu, renderer: &mut Renderer) -> Option<Requests> {
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
