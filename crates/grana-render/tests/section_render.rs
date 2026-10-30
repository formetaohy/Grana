use grana_render::{Camera, ChannelParams, Frame, RenderParams, Renderer, SectionCamera};
use grana_volume::{Axis, BrickGrid, Channel, Extent, Spacing, Volume, VoxelData};

const SIDE: u32 = 256;
const FEATURES: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
    .union(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);

#[test]
fn sections_planes_and_layers() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let options = |fallback: bool| wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: fallback,
        compatible_surface: None,
        apply_limit_buckets: false,
    };
    let adapter = pollster::block_on(instance.request_adapter(&options(false)))
        .or_else(|_| pollster::block_on(instance.request_adapter(&options(true))))
        .expect("no graphics adapter available");
    if !adapter.features().contains(FEATURES) {
        println!("the adapter lacks packed brick streaming, skipping");
        return;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("grana.section.test"),
        required_features: FEATURES,
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .expect("failed to create a test device");
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grana.section.test.target"),
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

    let layer = flat_volume(|_, _, z| if z == 8 { 60000 } else { 0 });
    let plane = section_render(&device, &queue, &target, &layer, Axis::Z, [0.0, 0.0, 0.0]);
    assert!(bright_fraction(&plane, 0.38, 0.62, 0.38, 0.62) > 0.98);
    let layer = flat_volume(|_, _, z| if z == 8 { 60000 } else { 0 });
    let neighbour = section_render(&device, &queue, &target, &layer, Axis::Z, [0.0, 0.0, 0.5]);
    assert!(dark_fraction(&neighbour, 0.0, 1.0, 0.0, 1.0) > 0.98);

    let half = flat_volume(|_, y, _| if y >= 32 { 60000 } else { 0 });
    let plane = section_render(&device, &queue, &target, &half, Axis::X, [0.0, 0.0, 0.0]);
    assert!(dark_fraction(&plane, 0.0, 0.33, 0.0, 1.0) > 0.98);
    assert!(bright_fraction(&plane, 0.67, 1.0, 0.2, 0.8) > 0.30);
}

fn section_render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::Texture,
    volume: &Volume,
    axis: Axis,
    focus: [f32; 3],
) -> Vec<u8> {
    let view = target.create_view(&Default::default());
    let pyramid = volume.pyramid();
    let world = glam::Vec3::from(volume.world_extent());
    let voxel = glam::Vec3::new(
        volume.spacing().x(),
        volume.spacing().y(),
        volume.spacing().z(),
    );
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, 64 << 20);
    renderer.set_pyramid(device, &pyramid);
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    camera.set_focus_along(Axis::X, focus[0]);
    camera.set_focus_along(Axis::Y, focus[1]);
    camera.set_focus_along(Axis::Z, focus[2]);
    let mut section = SectionCamera::new(axis);
    section.fit(world, voxel, 1.0);
    let transfer = grana_render::TransferFunction::new(vec![
        grana_render::Stop::new(0.0, [0.0, 0.0, 0.0], 0.0),
        grana_render::Stop::new(1.0, [1.0, 1.0, 1.0], 1.0),
    ]);
    let params = RenderParams {
        shading: Default::default(),
        step_scale: 1.0,
        channels: vec![ChannelParams {
            visible: true,
            opacity: 1.0,
            window: pyramid.channels()[0].window(),
            transfer,
        }],
    };
    let grid = BrickGrid::new(volume.extent());
    let mut volume_requests = Vec::new();
    let mut section_requests = Vec::new();
    for round in 0..32 {
        volume_requests.clear();
        section_requests.clear();
        let mut encoder = device.create_command_encoder(&Default::default());
        let info = renderer.draw(
            device,
            queue,
            Frame {
                encoder: &mut encoder,
                target: &view,
                size: [SIDE, SIDE],
                volume_region: [0, 0, SIDE, SIDE],
                section_region: [0, 0, SIDE, SIDE],
                camera: &camera,
                section: &section,
                params: &params,
                markers: &[],
                surface: None,
                volume_requests: &mut volume_requests,
                section_requests: &mut section_requests,
            },
        );
        queue.submit([encoder.finish()]);
        if section_requests.is_empty() {
            assert_eq!(info.section.resident, info.section.visible);
            break;
        }
        assert!(round < 31, "the section stream never settled");
        for request in &section_requests {
            let brick = volume.brick(&grid, request.coord);
            assert!(renderer.upload(device, queue, request.level, request.coord, &brick));
        }
    }

    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.draw(
        device,
        queue,
        Frame {
            encoder: &mut encoder,
            target: &view,
            size: [SIDE, SIDE],
            volume_region: [0, 0, SIDE, SIDE],
            section_region: [0, 0, SIDE, SIDE],
            camera: &camera,
            section: &section,
            params: &params,
            markers: &[],
            surface: None,
            volume_requests: &mut volume_requests,
            section_requests: &mut section_requests,
        },
    );
    let row_bytes = SIDE * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grana.section.test.readback"),
        size: u64::from(row_bytes * SIDE),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(SIDE),
            },
        },
        wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).expect("callback fires once");
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("failed to wait for the gpu");
    receiver.recv().expect("map callback").expect("map failed");
    slice.get_mapped_range().expect("mapped range").to_vec()
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

fn pixel(pixels: &[u8], x: u32, y: u32) -> u32 {
    let index = ((y * SIDE + x) * 4) as usize;
    u32::from(pixels[index]) + u32::from(pixels[index + 1]) + u32::from(pixels[index + 2])
}

fn bright_fraction(pixels: &[u8], x0: f32, x1: f32, y0: f32, y1: f32) -> f32 {
    fraction(pixels, x0, x1, y0, y1, |value| value > 660)
}

fn dark_fraction(pixels: &[u8], x0: f32, x1: f32, y0: f32, y1: f32) -> f32 {
    fraction(pixels, x0, x1, y0, y1, |value| value < 180)
}

fn fraction(
    pixels: &[u8],
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    accept: impl Fn(u32) -> bool,
) -> f32 {
    let left = (x0 * SIDE as f32) as u32;
    let right = (x1 * SIDE as f32) as u32;
    let top = (y0 * SIDE as f32) as u32;
    let bottom = (y1 * SIDE as f32) as u32;
    let mut accepted = 0;
    let mut total = 0;
    for y in top..bottom {
        for x in left..right {
            total += 1;
            if accept(pixel(pixels, x, y)) {
                accepted += 1;
            }
        }
    }
    accepted as f32 / total as f32
}
