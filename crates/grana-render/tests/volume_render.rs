use grana_render::{Camera, RenderParams, Renderer, TransferFunction};
use grana_volume::{Extent, Spacing, Volume, VoxelData};

const SIDE: u32 = 256;

#[test]
fn renders_a_volume() {
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
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("grana.test"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .expect("failed to create a test device");

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grana.test.target"),
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
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let volume = sphere_volume();
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    renderer.set_volume(&device, &queue, &volume);
    renderer.set_transfer_function(&queue, &TransferFunction::fluorescence());

    let mut camera = Camera::new(35.0);
    camera.fit(glam::Vec3::from(volume.world_extent()), 1.0);

    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.draw(
        &queue,
        &mut encoder,
        &view,
        [0, 0, SIDE, SIDE],
        &camera,
        &RenderParams::default(),
    );

    let row_bytes = SIDE * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grana.test.readback"),
        size: (row_bytes * SIDE) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
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
    let pixels = slice.get_mapped_range().expect("mapped range").to_vec();

    let lit = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32 > 120)
        .count();
    assert!(
        lit > (SIDE * SIDE / 20) as usize,
        "the volume barely rendered: {lit} lit pixels"
    );
}

fn sphere_volume() -> Volume {
    let extent = Extent::new(128, 128, 128);
    let mut data = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let radius = ((x as f32 - 64.0).powi(2)
                    + (y as f32 - 64.0).powi(2)
                    + (z as f32 - 64.0).powi(2))
                .sqrt();
                let density = (1.0 - radius / 44.0).max(0.0);
                data[extent.index(x, y, z) as usize] = (density * density * 60000.0) as u16;
            }
        }
    }
    Volume::new(extent, Spacing::new(0.1, 0.1, 0.1), VoxelData::U16(data))
}
