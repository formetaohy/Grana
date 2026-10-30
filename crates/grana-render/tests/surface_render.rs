use grana_render::{
    Camera, ChannelDisplay, ChannelParams, Frame, RenderParams, Renderer, SectionCamera, Surface,
    TransferFunction,
};
use grana_volume::{Axis, BrickGrid, Channel, Extent, Spacing, Volume, VoxelData};

const SIDE: u32 = 256;
const FEATURES: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
    .union(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);

#[test]
fn surfaces_shrink_with_the_threshold() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let options = |fallback: bool| wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: fallback,
        compatible_surface: None,
        apply_limit_buckets: false,
    };
    let adapter = pollster::block_on(instance.request_adapter(&options(false)))
        .or_else(|_| pollster::block_on(instance.request_adapter(&options(true))));
    let Ok(adapter) = adapter else {
        println!("no graphics adapter available, skipping");
        return;
    };
    if !adapter.features().contains(FEATURES) {
        println!("the adapter lacks packed brick streaming, skipping");
        return;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("grana.surface.test"),
        required_features: FEATURES,
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .expect("failed to create a test device");
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grana.surface.test.target"),
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

    let volume = sphere_volume();
    let wide = surface_render(&device, &queue, &target, &volume, 0.3);
    let narrow = surface_render(&device, &queue, &target, &volume, 0.7);
    let wide_lit = lit_pixels(&wide);
    let narrow_lit = lit_pixels(&narrow);
    assert!(wide_lit > 2000, "the surface barely rendered: {wide_lit}");
    assert!(
        wide_lit > narrow_lit + 200,
        "a higher threshold must shrink the surface: {wide_lit} against {narrow_lit}"
    );
    let corner = &wide[(8 * SIDE + 8) as usize * 4..][..3];
    assert!(
        corner.iter().all(|channel| *channel < 90),
        "the background must stay clear around the surface: {corner:?}"
    );
    let centre = &wide[(SIDE * SIDE / 2 + SIDE / 2) as usize * 4..][..3];
    assert!(
        centre.iter().map(|channel| *channel as u32).sum::<u32>() > 300,
        "the centre of the sphere must be lit: {centre:?}"
    );
}

fn surface_render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::Texture,
    volume: &Volume,
    threshold: f32,
) -> Vec<u8> {
    let view = target.create_view(&Default::default());
    let pyramid = volume.pyramid();
    let world = glam::Vec3::from(volume.world_extent());
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, 64 << 20);
    renderer.set_pyramid(device, &pyramid);
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    let section = SectionCamera::new(Axis::Z);
    let channel = &pyramid.channels()[0];
    let params = RenderParams {
        shading: Default::default(),
        step_scale: 1.0,
        channels: vec![ChannelParams {
            visible: true,
            opacity: 1.0,
            window: channel.window(),
            transfer: TransferFunction::tinted(channel.tint()),
            display: ChannelDisplay::Surface(Surface::new(
                channel.intensity().value(threshold),
                channel.tint(),
            )),
        }],
    };
    let grid = BrickGrid::new(volume.extent());
    let mut volume_requests = Vec::new();
    let mut section_requests = Vec::new();
    let mut encoder = device.create_command_encoder(&Default::default());
    for round in 0..32 {
        volume_requests.clear();
        section_requests.clear();
        let mut frame = device.create_command_encoder(&Default::default());
        renderer.draw(
            device,
            queue,
            Frame {
                encoder: &mut frame,
                target: &view,
                surface: [SIDE, SIDE],
                volume_region: [0, 0, SIDE, SIDE],
                section_region: [0, 0, 1, 1],
                camera: &camera,
                section: &section,
                params: &params,
                markers: &[],
                volume_requests: &mut volume_requests,
                section_requests: &mut section_requests,
            },
        );
        queue.submit([frame.finish()]);
        if volume_requests.is_empty() {
            break;
        }
        assert!(round < 31, "the stream never settled");
        for request in &volume_requests {
            let brick = volume.brick(&grid, request.coord);
            assert!(renderer.upload(device, queue, request.level, request.coord, &brick));
        }
    }
    renderer.draw(
        device,
        queue,
        Frame {
            encoder: &mut encoder,
            target: &view,
            surface: [SIDE, SIDE],
            volume_region: [0, 0, SIDE, SIDE],
            section_region: [0, 0, 1, 1],
            camera: &camera,
            section: &section,
            params: &params,
            markers: &[],
            volume_requests: &mut volume_requests,
            section_requests: &mut section_requests,
        },
    );
    let row_bytes = SIDE * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grana.surface.test.readback"),
        size: (row_bytes * SIDE) as u64,
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

fn lit_pixels(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32 > 300)
        .count()
}

fn sphere_volume() -> Volume {
    let extent = Extent::new(96, 96, 96);
    let mut data = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let radius =
                    glam::Vec3::new(x as f32, y as f32, z as f32) - glam::Vec3::splat(47.5);
                let profile = (1.0 - radius.length() / 36.0).max(0.0);
                data[extent.index(x, y, z) as usize] = (profile * 60000.0) as u16;
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.1),
        vec![Channel::new(
            "sphere",
            [0.3, 0.8, 1.0],
            VoxelData::U16(data),
        )],
    )
}
