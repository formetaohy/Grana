use wgpu::{
    CurrentSurfaceTexture, Device, DeviceDescriptor, ExperimentalFeatures, Instance,
    InstanceDescriptor, MemoryHints, PowerPreference, PresentMode, Queue, RequestAdapterOptions,
    Surface, SurfaceColorSpace, SurfaceConfiguration, TextureFormat, TextureUsages, Trace,
};

use crate::REQUIRED_FEATURES;

pub struct Gpu {
    pub surface: Surface<'static>,
    pub device: Device,
    pub queue: Queue,
    pub config: SurfaceConfiguration,
    pub adapter: String,
}

impl Gpu {
    pub fn new(window: impl Into<wgpu::SurfaceTarget<'static>>, size: [u32; 2]) -> Self {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window)
            .expect("failed to create a window surface");
        let options = |fallback: bool| RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            force_fallback_adapter: fallback,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        };
        let adapter = pollster::block_on(instance.request_adapter(&options(false)))
            .or_else(|_| pollster::block_on(instance.request_adapter(&options(true))))
            .expect("no graphics adapter can present to this window");
        assert!(
            adapter.features().contains(REQUIRED_FEATURES),
            "the graphics adapter cannot stream packed volume bricks"
        );
        let info = adapter.get_info();
        let description = format!("{} · {:?}", info.name, info.backend);
        println!("grana: using {description}");
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            label: Some("grana"),
            required_features: REQUIRED_FEATURES,
            required_limits: adapter.limits(),
            experimental_features: ExperimentalFeatures::disabled(),
            memory_hints: MemoryHints::Performance,
            trace: Trace::Off,
        }))
        .expect("failed to create a graphics device");
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .unwrap_or(capabilities.formats[0]);
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: SurfaceColorSpace::Auto,
            width: size[0].max(1),
            height: size[1].max(1),
            present_mode: PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        Self {
            surface,
            device,
            queue,
            config,
            adapter: description,
        }
    }

    pub fn format(&self) -> TextureFormat {
        self.config.format
    }

    pub fn size(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        if size[0] == 0 || size[1] == 0 {
            return;
        }
        self.config.width = size[0];
        self.config.height = size[1];
        self.surface.configure(&self.device, &self.config);
    }

    pub fn acquire(&mut self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) => Some(frame),
            CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
            CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                None
            }
            CurrentSurfaceTexture::Timeout
            | CurrentSurfaceTexture::Occluded
            | CurrentSurfaceTexture::Validation => None,
        }
    }
}
