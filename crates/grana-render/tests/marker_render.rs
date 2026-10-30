use grana_render::{
    Camera, ChannelDisplay, ChannelParams, Frame, Marker, RenderParams, Renderer, SectionCamera,
    TransferFunction,
};
use grana_volume::{Axis, Channel, Extent, Spacing, Volume, VoxelData};

const SIDE: u32 = 256;
const FEATURES: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
    .union(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);

#[test]
fn draws_markers_over_the_background() {
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
        label: Some("grana.marker.test"),
        required_features: FEATURES,
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .expect("failed to create a test device");
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grana.marker.test.target"),
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
    let view = target.create_view(&Default::default());
    let volume = empty_volume();
    let pyramid = volume.pyramid();
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, 32 << 20);
    renderer.set_pyramid(&device, &pyramid);
    let mut camera = Camera::new(35.0);
    camera.fit(glam::Vec3::from(volume.world_extent()), 1.0);
    let params = RenderParams {
        shading: Default::default(),
        step_scale: 1.0,
        channels: vec![ChannelParams {
            visible: true,
            opacity: 1.0,
            window: pyramid.channels()[0].window(),
            transfer: TransferFunction::tinted([0.3, 0.6, 1.0]),
            display: ChannelDisplay::Volume,
        }],
    };
    let markers = [Marker::new([0.0, 0.0, 0.0], 0.25, [1.0, 0.0, 0.0], 1.0)];
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.draw(
        &device,
        &queue,
        Frame {
            encoder: &mut encoder,
            target: &view,
            volume_region: [0, 0, SIDE, SIDE],
            section_region: [0, 0, 1, 1],
            camera: &camera,
            section: &SectionCamera::new(Axis::Z),
            params: &params,
            markers: &markers,
            volume_requests: &mut Vec::new(),
            section_requests: &mut Vec::new(),
        },
    );
    let row_bytes = SIDE * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grana.marker.test.readback"),
        size: u64::from(row_bytes * SIDE),
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

    let red = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > pixel[2] + 40 && pixel[0] > 40)
        .count();
    assert!(red > 200, "the marker barely rendered: {red} pixels");
    assert!(
        red < (SIDE * SIDE / 2) as usize,
        "the marker covers too much: {red} pixels"
    );
    let centre = pixel(&pixels, SIDE / 2, SIDE / 2);
    assert!(centre[0] > 120, "the marker centre is not lit: {centre:?}");
    let corner = pixel(&pixels, 4, SIDE - 4);
    assert!(
        corner.iter().map(|value| *value as u32).sum::<u32>() < 120,
        "the background must stay dark: {corner:?}"
    );
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let index = ((y * SIDE + x) * 4) as usize;
    [pixels[index], pixels[index + 1], pixels[index + 2]]
}

fn empty_volume() -> Volume {
    let extent = Extent::new(32, 32, 32);
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.1),
        vec![Channel::new(
            "empty",
            [0.3, 0.6, 1.0],
            VoxelData::U16(vec![0; extent.voxel_count() as usize]),
        )],
    )
}
