use grana_render::{
    Camera, ChannelParams, Frame, FrameInfo, RenderParams, Renderer, SectionCamera,
    TransferFunction,
};
use grana_volume::{Axis, BrickGrid, Channel, Extent, Movie, Spacing, Volume, VoxelData};

const SIDE: u32 = 256;
const FEATURES: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
    .union(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);

#[test]
fn streams_two_channels() {
    let Some((device, queue)) = open_device() else {
        return;
    };
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

    let volume = two_channel_volume();
    let pyramid = volume.pyramid();
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, 512 << 20);
    renderer.set_pyramid(&device, &pyramid);

    let mut camera = Camera::new(35.0);
    camera.fit(glam::Vec3::from(volume.world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let params = RenderParams {
        shading: Default::default(),
        step_scale: 0.5,
        channels: pyramid
            .channels()
            .iter()
            .map(|channel| ChannelParams {
                visible: true,
                opacity: 1.0,
                window: channel.window(),
                transfer: TransferFunction::tinted(channel.tint()),
            })
            .collect(),
    };

    let grid = BrickGrid::new(volume.extent());
    let mut volume_requests = Vec::new();
    let mut section_requests = Vec::new();
    for round in 0..10 {
        volume_requests.clear();
        section_requests.clear();
        let mut encoder = device.create_command_encoder(&Default::default());
        let info = renderer.draw(
            &device,
            &queue,
            Frame {
                encoder: &mut encoder,
                target: &view,
                size: [SIDE, SIDE],
                volume_region: [0, 0, SIDE, SIDE],
                section_region: [0, 0, 1, 1],
                time: 0,
                camera: &camera,
                section: &section,
                params: &params,
                markers: &[],
                meshes: &[],
                volume_requests: &mut volume_requests,
                section_requests: &mut section_requests,
            },
        );
        queue.submit([encoder.finish()]);
        assert_eq!(
            info.volume.resident + volume_requests.len(),
            info.volume.visible
        );
        if volume_requests.is_empty() {
            break;
        }
        assert!(round < 9, "the stream never settled");
        for request in &volume_requests {
            let brick = volume.brick(&grid, request.coord);
            assert!(renderer.upload(&device, &queue, *request, &brick));
        }
    }

    let mut encoder = device.create_command_encoder(&Default::default());
    let info: FrameInfo = renderer.draw(
        &device,
        &queue,
        Frame {
            encoder: &mut encoder,
            target: &view,
            size: [SIDE, SIDE],
            volume_region: [0, 0, SIDE, SIDE],
            section_region: [0, 0, 1, 1],
            time: 0,
            camera: &camera,
            section: &section,
            params: &params,
            markers: &[],
            meshes: &[],
            volume_requests: &mut volume_requests,
            section_requests: &mut section_requests,
        },
    );
    assert_eq!(info.volume.resident, info.volume.visible);

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

    let pixels = pixels.as_chunks::<4>().0;
    let lit = pixels
        .iter()
        .filter(|pixel| pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32 > 120)
        .count();
    let blue = pixels
        .iter()
        .filter(|pixel| pixel[2] > pixel[0] + 20 && pixel[2] > 40)
        .count();
    let green = pixels
        .iter()
        .filter(|pixel| pixel[1] > pixel[0] + 20 && pixel[1] > 40)
        .count();
    assert!(
        lit > (SIDE * SIDE / 20) as usize,
        "the volume barely rendered: {lit} lit pixels"
    );
    assert!(
        blue > 200,
        "the blue channel barely rendered: {blue} pixels"
    );
    assert!(
        green > 200,
        "the green channel barely rendered: {green} pixels"
    );
}

fn two_channel_volume() -> Volume {
    let extent = Extent::new(128, 128, 96);
    let count = extent.voxel_count() as usize;
    let mut blue = vec![0u16; count];
    let mut green = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let index = extent.index(x, y, z) as usize;
                blue[index] = density(x, y, z, 40.0);
                green[index] = density(x, y, z, 88.0);
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.13),
        vec![
            Channel::new("nuclei", [0.2, 0.4, 1.0], VoxelData::U16(blue)),
            Channel::new("membrane", [0.2, 1.0, 0.3], VoxelData::U16(green)),
        ],
    )
}

fn density(x: u32, y: u32, z: u32, centre_x: f32) -> u16 {
    let radius =
        ((x as f32 - centre_x).powi(2) + (y as f32 - 64.0).powi(2) + (z as f32 - 48.0).powi(2))
            .sqrt();
    let density = (1.0 - radius / 30.0).max(0.0);
    (density * density * 60000.0) as u16
}

#[test]
fn switches_frames() {
    let Some((device, queue)) = open_device() else {
        return;
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grana.test.frame.target"),
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

    let movie = two_frame_movie();
    let pyramid = movie.pyramid().clone();
    let mut renderer = Renderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, 512 << 20);
    renderer.set_pyramid(&device, &pyramid);
    let mut camera = Camera::new(35.0);
    camera.fit(glam::Vec3::from(movie.frame(0).world_extent()), 1.0);
    let section = SectionCamera::new(Axis::Z);
    let params = RenderParams {
        shading: Default::default(),
        step_scale: 0.5,
        channels: pyramid
            .channels()
            .iter()
            .map(|channel| ChannelParams {
                visible: true,
                opacity: 1.0,
                window: channel.window(),
                transfer: TransferFunction::tinted(channel.tint()),
            })
            .collect(),
    };
    let grid = BrickGrid::new(movie.frame(0).extent());
    let mut volume_requests = Vec::new();
    let mut section_requests = Vec::new();

    let mut settle = |time: u32| {
        for round in 0..32 {
            volume_requests.clear();
            section_requests.clear();
            let mut encoder = device.create_command_encoder(&Default::default());
            let info = renderer.draw(
                &device,
                &queue,
                Frame {
                    encoder: &mut encoder,
                    target: &view,
                    size: [SIDE, SIDE],
                    volume_region: [0, 0, SIDE, SIDE],
                    section_region: [0, 0, 1, 1],
                    time,
                    camera: &camera,
                    section: &section,
                    params: &params,
                    markers: &[],
                    meshes: &[],
                    volume_requests: &mut volume_requests,
                    section_requests: &mut section_requests,
                },
            );
            queue.submit([encoder.finish()]);
            assert_eq!(
                info.volume.resident + volume_requests.len(),
                info.volume.visible
            );
            if volume_requests.is_empty() {
                assert_eq!(info.volume.resident, info.volume.visible);
                return;
            }
            assert!(round < 31, "the frame stream never settled");
            for request in &volume_requests {
                let brick = movie.brick(request.time, &grid, request.coord);
                assert!(renderer.upload(&device, &queue, *request, &brick));
            }
        }
    };

    settle(0);
    let first = read_pixels(&device, &queue, &target);
    assert!(
        blue(&first) > 200,
        "the first frame barely rendered: {} blue pixels",
        blue(&first)
    );
    assert!(
        green(&first) < 60,
        "the first frame leaked the second frame: {} green pixels",
        green(&first)
    );

    settle(1);
    let second = read_pixels(&device, &queue, &target);
    assert!(
        green(&second) > 200,
        "the second frame barely rendered: {} green pixels",
        green(&second)
    );
    assert!(
        blue(&second) < 60,
        "the second frame kept the first frame: {} blue pixels",
        blue(&second)
    );
}

fn two_frame_movie() -> Movie {
    let extent = Extent::new(96, 96, 64);
    let spacing = Spacing::new(0.1, 0.1, 0.13);
    let count = extent.voxel_count() as usize;
    let zeros = vec![0u16; count];
    let mut blob = vec![0u16; count];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let radius = ((x as f32 - 48.0).powi(2)
                    + (y as f32 - 48.0).powi(2)
                    + (z as f32 - 32.0).powi(2))
                .sqrt();
                let density = (1.0 - radius / 22.0).max(0.0);
                blob[extent.index(x, y, z) as usize] = (density * density * 60000.0) as u16;
            }
        }
    }
    let frame = |blue: Vec<u16>, green: Vec<u16>| {
        Volume::new(
            extent,
            spacing,
            vec![
                Channel::new("nuclei", [0.2, 0.4, 1.0], VoxelData::U16(blue)),
                Channel::new("membrane", [0.2, 1.0, 0.3], VoxelData::U16(green)),
            ],
        )
    };
    Movie::new(
        vec![frame(blob.clone(), zeros.clone()), frame(zeros, blob)],
        1.0,
    )
}

fn open_device() -> Option<(wgpu::Device, wgpu::Queue)> {
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
        return None;
    };
    if !adapter.features().contains(FEATURES) {
        println!("the adapter lacks packed brick streaming, skipping");
        return None;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("grana.test"),
        required_features: FEATURES,
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .expect("failed to create a test device");
    Some((device, queue))
}

fn read_pixels(device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::Texture) -> Vec<[u8; 4]> {
    let row_bytes = SIDE * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grana.test.frame.readback"),
        size: (row_bytes * SIDE) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
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
    let pixels = slice.get_mapped_range().expect("mapped range").to_vec();
    pixels.as_chunks::<4>().0.to_vec()
}

fn blue(pixels: &[[u8; 4]]) -> usize {
    pixels
        .iter()
        .filter(|pixel| {
            u32::from(pixel[2]) > u32::from(pixel[0]) + 20
                && u32::from(pixel[2]) > u32::from(pixel[1]) + 20
                && pixel[2] > 40
        })
        .count()
}

fn green(pixels: &[[u8; 4]]) -> usize {
    pixels
        .iter()
        .filter(|pixel| {
            u32::from(pixel[1]) > u32::from(pixel[0]) + 20
                && u32::from(pixel[1]) > u32::from(pixel[2]) + 20
                && pixel[1] > 40
        })
        .count()
}
