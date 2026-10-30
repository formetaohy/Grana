use glam::Vec3;
use grana_render::{
    BrickRequest, Camera, ChannelParams, Frame, Marker, RenderParams, Renderer, SectionCamera,
    Stop, TransferFunction,
};
use grana_volume::{Axis, BrickGrid, Channel, Extent, Spacing, Volume, VoxelData};

const SIDE: u32 = 256;
const BUDGET: u64 = 64 << 20;
const FEATURES: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
    .union(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);

#[test]
fn composites_objects_into_the_volume() {
    let Some((device, queue)) = gpu() else {
        return;
    };
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
    let empty = volume(0);
    let dense = volume(60_000);
    let mut camera = Camera::new(35.0);
    camera.fit(Vec3::from(empty.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let origin = [Marker::new([0.0, 0.0, 0.0], 0.4, [1.0, 0.1, 0.1], 1.0)];
    let whole = [0, 0, SIDE, SIDE];
    let corner = [0, 0, 1, 1];

    let visible = render(
        &device, &queue, &target, &empty, &camera, &section, &origin, whole, corner,
    );
    assert!(red(&visible, whole) > 200, "the object is not drawn");

    let hidden = render(
        &device, &queue, &target, &dense, &camera, &section, &origin, whole, corner,
    );
    assert!(
        red(&hidden, whole) < 20,
        "the volume does not occlude the object"
    );

    let sliced = render(
        &device, &queue, &target, &empty, &camera, &section, &origin, corner, whole,
    );
    assert!(
        red(&sliced, whole) > 200,
        "the section does not mark the object"
    );

    let mut moved = camera.clone();
    moved.set_focus_along(Axis::Z, 2.0 * section.span());
    let missed = render(
        &device, &queue, &target, &empty, &moved, &section, &origin, corner, whole,
    );
    assert_eq!(
        red(&missed, whole),
        0,
        "the section marks an object it does not intersect"
    );

    let background = pixel(&visible, 4, SIDE - 4);
    assert!(
        background
            .iter()
            .map(|value| u32::from(*value))
            .sum::<u32>()
            < 120,
        "the background must stay dark: {background:?}"
    );
}

fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let options = |fallback: bool| wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: fallback,
        compatible_surface: None,
        apply_limit_buckets: false,
    };
    let adapter = pollster::block_on(instance.request_adapter(&options(false)))
        .or_else(|_| pollster::block_on(instance.request_adapter(&options(true))))
        .ok()?;
    if !adapter.features().contains(FEATURES) {
        println!("the adapter lacks packed brick streaming, skipping");
        return None;
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
    Some((device, queue))
}

#[allow(clippy::too_many_arguments)]
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::Texture,
    volume: &Volume,
    camera: &Camera,
    section: &SectionCamera,
    markers: &[Marker],
    volume_region: [u32; 4],
    section_region: [u32; 4],
) -> Vec<u8> {
    let view = target.create_view(&Default::default());
    let pyramid = volume.pyramid();
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_pyramid(device, &pyramid);
    let params = RenderParams {
        shading: Default::default(),
        step_scale: 1.0,
        channels: vec![ChannelParams {
            visible: true,
            opacity: 1.0,
            window: pyramid.channels()[0].window(),
            transfer: TransferFunction::new(vec![
                Stop::new(0.0, [0.0, 0.0, 0.0], 0.0),
                Stop::new(1.0, [1.0, 1.0, 1.0], 1.0),
            ]),
        }],
    };
    let grid = BrickGrid::new(volume.extent());
    let mut volume_requests = Vec::new();
    let mut section_requests = Vec::new();
    for round in 0..64 {
        volume_requests.clear();
        section_requests.clear();
        let encoder = draw(
            device,
            &mut renderer,
            queue,
            &view,
            camera,
            section,
            &params,
            markers,
            volume_region,
            section_region,
            &mut volume_requests,
            &mut section_requests,
        );
        queue.submit([encoder.finish()]);
        if volume_requests.is_empty() && section_requests.is_empty() {
            break;
        }
        assert!(round < 63, "the stream never settled");
        for request in volume_requests.iter().chain(section_requests.iter()) {
            let brick = volume.brick(&grid, request.coord);
            assert!(renderer.upload(device, queue, *request, &brick));
        }
    }
    let mut encoder = draw(
        device,
        &mut renderer,
        queue,
        &view,
        camera,
        section,
        &params,
        markers,
        volume_region,
        section_region,
        &mut volume_requests,
        &mut section_requests,
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

#[allow(clippy::too_many_arguments)]
fn draw(
    device: &wgpu::Device,
    renderer: &mut Renderer,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    camera: &Camera,
    section: &SectionCamera,
    params: &RenderParams,
    markers: &[Marker],
    volume_region: [u32; 4],
    section_region: [u32; 4],
    volume_requests: &mut Vec<BrickRequest>,
    section_requests: &mut Vec<BrickRequest>,
) -> wgpu::CommandEncoder {
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.draw(
        device,
        queue,
        Frame {
            encoder: &mut encoder,
            target: view,
            size: [SIDE, SIDE],
            volume_region,
            section_region,
            time: 0,
            camera,
            section,
            params,
            markers,
            surface: None,
            volume_requests,
            section_requests,
        },
    );
    encoder
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let index = ((y * SIDE + x) * 4) as usize;
    [pixels[index], pixels[index + 1], pixels[index + 2]]
}

fn red(pixels: &[u8], region: [u32; 4]) -> usize {
    let mut count = 0;
    for y in region[1]..region[1] + region[3] {
        for x in region[0]..region[0] + region[2] {
            let [red, green, blue] = pixel(pixels, x, y).map(u32::from);
            if red > green + 40 && red > blue + 40 {
                count += 1;
            }
        }
    }
    count
}

fn volume(value: u16) -> Volume {
    let extent = Extent::new(64, 64, 16);
    let mut voxels = vec![value; extent.voxel_count() as usize];
    voxels[0] = 0;
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.5),
        vec![Channel::new(
            "density",
            [0.3, 0.6, 1.0],
            VoxelData::U16(voxels),
        )],
    )
}
