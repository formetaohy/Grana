use glam::Vec3;
use grana_object::{Mesh, MeshVertex};
use grana_render::{
    Camera, ChannelParams, Frame, RenderParams, Renderer, SectionCamera, SurfaceFrame,
    SurfaceStyle, TransferFunction,
};
use grana_volume::{Axis, BrickGrid, Channel, Extent, Spacing, Volume, VoxelData};

const SIDE: u32 = 256;
const BUDGET: u64 = 64 << 20;
const FEATURES: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
    .union(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);

#[test]
fn renders_surface_objects_and_sections() {
    let Some((device, queue)) = gpu() else {
        return;
    };
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
    let volume = empty_volume();
    let mesh = cubes();
    let whole = [0, 0, SIDE, SIDE];
    let corner = [0, 0, 1, 1];
    let visible = [
        SurfaceStyle::new([1.0, 0.12, 0.12], true, false),
        SurfaceStyle::new([0.12, 1.0, 0.12], true, false),
    ];
    let selected = [
        SurfaceStyle::new([1.0, 0.12, 0.12], true, true),
        SurfaceStyle::new([0.12, 1.0, 0.12], true, false),
    ];
    let hidden = [
        SurfaceStyle::new([1.0, 0.12, 0.12], false, false),
        SurfaceStyle::new([0.12, 1.0, 0.12], false, false),
    ];

    let pixels = render(
        &device, &queue, &target, &volume, &mesh, &visible, whole, corner, 0.0,
    );
    let red = red_pixels(&pixels);
    let green = green_pixels(&pixels);
    assert!(red > 1500, "the red object is not drawn: {red}");
    assert!(green > 1500, "the green object is not drawn: {green}");
    let background = pixel(&pixels, 4, SIDE - 4);
    assert!(
        background
            .iter()
            .map(|value| u32::from(*value))
            .sum::<u32>()
            < 120,
        "the background must stay dark: {background:?}"
    );

    let changed = render(
        &device, &queue, &target, &volume, &mesh, &selected, whole, corner, 0.0,
    );
    assert!(
        green_sum(&changed) > green_sum(&pixels) + (SIDE * SIDE) as u64,
        "selection does not highlight the object"
    );

    let cleared = render(
        &device, &queue, &target, &volume, &mesh, &hidden, whole, corner, 0.0,
    );
    assert_eq!(
        red_pixels(&cleared) + green_pixels(&cleared),
        0,
        "a hidden object is still drawn"
    );

    let section = render(
        &device, &queue, &target, &volume, &mesh, &visible, corner, whole, 0.0,
    );
    let inside = red_pixels(&section);
    assert!(
        inside > 1500,
        "the section does not fill the object: {inside}"
    );

    let missed = render(
        &device, &queue, &target, &volume, &mesh, &visible, corner, whole, 1.2,
    );
    assert_eq!(
        red_pixels(&missed),
        0,
        "the section fills an object the plane misses"
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
        label: Some("grana.surface.test"),
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
    mesh: &Mesh,
    styles: &[SurfaceStyle],
    volume_region: [u32; 4],
    section_region: [u32; 4],
    focus_z: f32,
) -> Vec<u8> {
    let view = target.create_view(&Default::default());
    let pyramid = volume.pyramid();
    let world = Vec3::from(volume.world_extent());
    let voxel = Vec3::new(
        volume.spacing().x(),
        volume.spacing().y(),
        volume.spacing().z(),
    );
    let mut renderer = Renderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, BUDGET);
    renderer.set_pyramid(device, &pyramid);
    renderer.set_surface(device, Some(mesh));
    let mut camera = Camera::new(35.0);
    camera.fit(world, 1.0);
    camera.set_focus_along(Axis::Z, focus_z);
    let mut section = SectionCamera::new(Axis::Z);
    section.fit(world, voxel, 1.0);
    let params = RenderParams {
        shading: Default::default(),
        step_scale: 1.0,
        channels: vec![ChannelParams {
            visible: true,
            opacity: 1.0,
            window: pyramid.channels()[0].window(),
            transfer: TransferFunction::new(vec![
                grana_render::Stop::new(0.0, [0.0, 0.0, 0.0], 0.0),
                grana_render::Stop::new(1.0, [1.0, 1.0, 1.0], 1.0),
            ]),
        }],
    };
    let grid = BrickGrid::new(volume.extent());
    let mut volume_requests = Vec::new();
    let mut section_requests = Vec::new();
    for round in 0..64 {
        volume_requests.clear();
        section_requests.clear();
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.draw(
            device,
            queue,
            Frame {
                encoder: &mut encoder,
                target: &view,
                size: [SIDE, SIDE],
                volume_region,
                section_region,
                camera: &camera,
                section: &section,
                params: &params,
                markers: &[],
                surface: Some(SurfaceFrame { styles }),
                volume_requests: &mut volume_requests,
                section_requests: &mut section_requests,
            },
        );
        queue.submit([encoder.finish()]);
        if volume_requests.is_empty() && section_requests.is_empty() {
            break;
        }
        assert!(round < 63, "the stream never settled");
        for request in volume_requests.iter().chain(section_requests.iter()) {
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
            volume_region,
            section_region,
            camera: &camera,
            section: &section,
            params: &params,
            markers: &[],
            surface: Some(SurfaceFrame { styles }),
            volume_requests: &mut volume_requests,
            section_requests: &mut section_requests,
        },
    );
    let row_bytes = SIDE * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grana.surface.test.readback"),
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

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let index = ((y * SIDE + x) * 4) as usize;
    [pixels[index], pixels[index + 1], pixels[index + 2]]
}

fn red_pixels(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| {
            pixel[0] as u32 > pixel[1] as u32 + 40 && pixel[0] as u32 > pixel[2] as u32 + 40
        })
        .count()
}

fn green_pixels(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| {
            pixel[1] as u32 > pixel[0] as u32 + 40 && pixel[1] as u32 > pixel[2] as u32 + 40
        })
        .count()
}

fn green_sum(pixels: &[u8]) -> u64 {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| u64::from(pixel[1]))
        .sum()
}

fn cubes() -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (object, centre) in [-0.7f32, 0.7].into_iter().enumerate() {
        let base = vertices.len() as u32;
        let corners = [
            [-1.0f32, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ];
        for corner in corners {
            let position =
                Vec3::new(corner[0], corner[1], corner[2]) * 0.6 + Vec3::new(centre, 0.0, 0.0);
            let normal = (position - Vec3::new(centre, 0.0, 0.0)).normalize();
            vertices.push(MeshVertex::new(
                position.to_array(),
                normal.to_array(),
                object as u32,
            ));
        }
        for face in [
            [0u32, 1, 2],
            [0, 2, 3],
            [4, 6, 5],
            [4, 7, 6],
            [0, 4, 5],
            [0, 5, 1],
            [1, 5, 6],
            [1, 6, 2],
            [2, 6, 7],
            [2, 7, 3],
            [3, 7, 4],
            [3, 4, 0],
        ] {
            indices.extend(face.map(|corner| base + corner));
        }
    }
    Mesh::new(vertices, indices)
}

fn empty_volume() -> Volume {
    let extent = Extent::new(32, 32, 32);
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.1),
        vec![Channel::new(
            "empty",
            [0.4, 0.6, 1.0],
            VoxelData::U16(vec![0; extent.voxel_count() as usize]),
        )],
    )
}
