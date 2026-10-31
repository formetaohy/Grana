use grana_render::{
    BrickRequest, Camera, ChannelParams, Frame, RenderParams, Renderer, SectionCamera,
    TransferFunction, ViewInfo,
};
use grana_volume::{Axis, BrickGrid, Channel, Extent, Level, Pyramid, Spacing, Volume, VoxelData};

const SIDE: u32 = 256;

#[test]
fn switches_levels() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .expect("adapter");
    let features = wgpu::Features::TEXTURE_BINDING_ARRAY
        .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
        .union(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("lod"),
        required_features: features,
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .expect("device");
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lod.target"),
        size: wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());

    let levels = [
        (Extent::new(256, 256, 64), Spacing::new(0.1, 0.1, 0.2)),
        (Extent::new(128, 128, 32), Spacing::new(0.2, 0.2, 0.4)),
        (Extent::new(64, 64, 16), Spacing::new(0.4, 0.4, 0.8)),
    ];
    let volumes = levels
        .iter()
        .map(|(extent, spacing)| sphere(*extent, *spacing))
        .collect::<Vec<_>>();
    let pyramid = Pyramid::new(
        levels
            .iter()
            .map(|(extent, spacing)| Level::new(*extent, *spacing))
            .collect(),
        volumes[0].pyramid().channels().to_vec(),
        1,
        1.0,
    );
    let grids = levels
        .iter()
        .map(|(extent, _)| BrickGrid::new(*extent))
        .collect::<Vec<_>>();
    let mut renderer = Renderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        100 * 78_608,
    );
    renderer.set_pyramid(&device, &pyramid);
    let mut camera = Camera::new(35.0);
    camera.fit(glam::Vec3::from(volumes[0].world_extent()), 1.0);
    let params = RenderParams {
        shading: Default::default(),
        step_scale: 1.0,
        channels: vec![ChannelParams {
            visible: true,
            opacity: 1.0,
            window: pyramid.channels()[0].window(),
            transfer: TransferFunction::tinted([0.3, 0.6, 1.0]),
        }],
    };
    let mut stream = Stream {
        renderer,
        device: &device,
        queue: &queue,
        view: &view,
        section: SectionCamera::new(Axis::Z),
        volumes: &volumes,
        grids: &grids,
        params: &params,
        volume_requests: Vec::new(),
        section_requests: Vec::new(),
    };

    let fitted = stream.settle(&camera);
    println!(
        "fitted: level {} resident {}/{}",
        fitted.level, fitted.resident, fitted.visible
    );
    assert!(
        fitted.level > 0,
        "the fitted view must coarsen to fit the cache"
    );

    for _ in 0..10 {
        camera.zoom(1.0);
    }
    let close = stream.settle(&camera);
    println!(
        "close: level {} resident {}/{}",
        close.level, close.resident, close.visible
    );
    assert_eq!(close.level, 0, "zooming in must resolve the finest level");

    for _ in 0..10 {
        camera.zoom(-1.0);
    }
    let far = stream.settle(&camera);
    println!(
        "far: level {} resident {}/{}",
        far.level, far.resident, far.visible
    );
    assert!(far.level > 0, "zooming out must coarsen again");
}

struct Stream<'a> {
    renderer: Renderer,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    view: &'a wgpu::TextureView,
    section: SectionCamera,
    volumes: &'a [Volume],
    grids: &'a [BrickGrid],
    params: &'a RenderParams,
    volume_requests: Vec<BrickRequest>,
    section_requests: Vec<BrickRequest>,
}

impl Stream<'_> {
    fn settle(&mut self, camera: &Camera) -> ViewInfo {
        for _ in 0..50 {
            self.volume_requests.clear();
            self.section_requests.clear();
            let mut encoder = self.device.create_command_encoder(&Default::default());
            let info = self.renderer.draw(
                self.device,
                self.queue,
                Frame {
                    encoder: &mut encoder,
                    target: self.view,
                    size: [SIDE, SIDE],
                    volume_region: [0, 0, SIDE, SIDE],
                    section_region: [0, 0, 1, 1],
                    time: 0,
                    camera,
                    section: &self.section,
                    params: self.params,
                    markers: &[],
                    meshes: &[],
                    volume_requests: &mut self.volume_requests,
                    section_requests: &mut self.section_requests,
                },
            );
            self.queue.submit([encoder.finish()]);
            if self.volume_requests.is_empty() {
                assert_eq!(
                    info.volume.resident, info.volume.visible,
                    "the view must be complete"
                );
                return info.volume;
            }
            for request in self.volume_requests.iter() {
                let brick =
                    self.volumes[request.level].brick(&self.grids[request.level], request.coord);
                assert!(
                    self.renderer
                        .upload(self.device, self.queue, *request, &brick)
                );
            }
        }
        panic!("the stream never settled");
    }
}

fn sphere(extent: Extent, spacing: Spacing) -> Volume {
    let mut data = vec![0u16; extent.voxel_count() as usize];
    let centre = [
        extent.x() as f32 * 0.5,
        extent.y() as f32 * 0.5,
        extent.z() as f32 * 0.5,
    ];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let dx = (x as f32 - centre[0]) / (extent.x() as f32 * 0.3);
                let dy = (y as f32 - centre[1]) / (extent.y() as f32 * 0.3);
                let dz = (z as f32 - centre[2]) / (extent.z() as f32 * 0.3);
                let radius = (dx * dx + dy * dy + dz * dz).sqrt();
                let value = (1.0 - radius).max(0.0);
                data[extent.index(x, y, z) as usize] = (value * 60000.0) as u16;
            }
        }
    }
    Volume::new(
        extent,
        spacing,
        vec![Channel::new(
            "sphere",
            [0.3, 0.6, 1.0],
            VoxelData::U16(data),
        )],
    )
}
