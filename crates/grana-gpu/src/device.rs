use wgpu::{
    Adapter, Backends, CurrentSurfaceTexture, Device, DeviceDescriptor, ExperimentalFeatures,
    Instance, InstanceDescriptor, MemoryHints, PowerPreference, PresentMode, Queue,
    RequestAdapterOptions, Surface, SurfaceColorSpace, SurfaceConfiguration, SurfaceTarget,
    TextureFormat, TextureUsages, Trace,
};

use crate::REQUIRED_FEATURES;

pub const HEADLESS_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

const TIMING_FEATURES: wgpu::Features = wgpu::Features::TIMESTAMP_QUERY;

const BACKEND_ORDER: &[Backends] = backend_order();

const fn backend_order() -> &'static [Backends] {
    #[cfg(target_os = "windows")]
    {
        &[Backends::DX12, Backends::VULKAN]
    }
    #[cfg(target_os = "macos")]
    {
        &[Backends::METAL]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        &[Backends::VULKAN]
    }
}

pub struct Gpu {
    pub surface: Surface<'static>,
    pub device: Device,
    pub queue: Queue,
    pub config: SurfaceConfiguration,
    pub adapter: String,
}

pub struct Headless {
    pub device: Device,
    pub queue: Queue,
}

impl Headless {
    pub fn new() -> Result<Self, String> {
        let mut failures = Vec::new();
        for fallback in [false, true] {
            for backends in BACKEND_ORDER {
                let instance = instance(*backends);
                match acquire(&instance, None, fallback) {
                    Ok((_, device, queue)) => return Ok(Self { device, queue }),
                    Err(detail) => failures.push(format!("{backends:?}: {detail}")),
                }
            }
        }
        Err(format!(
            "no graphics adapter can render grana ({})",
            failures.join("; ")
        ))
    }

    pub fn format(&self) -> TextureFormat {
        HEADLESS_FORMAT
    }
}

impl Gpu {
    pub fn new<W>(window: W, size: [u32; 2]) -> Self
    where
        W: Into<SurfaceTarget<'static>> + Clone,
    {
        let mut failures = Vec::new();
        for fallback in [false, true] {
            for backends in BACKEND_ORDER {
                let instance = instance(*backends);
                let surface = instance
                    .create_surface(window.clone())
                    .expect("failed to create a window surface");
                match acquire(&instance, Some(&surface), fallback) {
                    Ok((adapter, device, queue)) => {
                        return Self::configure(adapter, device, queue, surface, size);
                    }
                    Err(detail) => failures.push(format!("{backends:?}: {detail}")),
                }
            }
        }
        panic!(
            "no graphics adapter can render grana ({})",
            failures.join("; ")
        );
    }

    fn configure(
        adapter: Adapter,
        device: Device,
        queue: Queue,
        surface: Surface<'static>,
        size: [u32; 2],
    ) -> Self {
        let info = adapter.get_info();
        let description = format!("{} · {:?}", info.name, info.backend);
        println!("grana: using {description}");
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

fn instance(backends: Backends) -> Instance {
    Instance::new(InstanceDescriptor {
        backends,
        ..InstanceDescriptor::new_without_display_handle()
    })
}

fn acquire(
    instance: &Instance,
    surface: Option<&Surface<'static>>,
    fallback: bool,
) -> Result<(Adapter, Device, Queue), String> {
    let options = RequestAdapterOptions {
        power_preference: PowerPreference::HighPerformance,
        force_fallback_adapter: fallback,
        compatible_surface: surface,
        apply_limit_buckets: false,
    };
    let adapter = pollster::block_on(instance.request_adapter(&options))
        .map_err(|_| "no graphics adapter is available".to_string())?;
    if !adapter.features().contains(REQUIRED_FEATURES) {
        return Err("the graphics adapter cannot stream packed volume bricks".to_string());
    }
    let mut features = REQUIRED_FEATURES;
    if adapter.features().contains(TIMING_FEATURES) {
        features |= TIMING_FEATURES;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
        label: Some("grana"),
        required_features: features,
        required_limits: adapter.limits(),
        experimental_features: ExperimentalFeatures::disabled(),
        memory_hints: MemoryHints::Performance,
        trace: Trace::Off,
    }))
    .map_err(|error| format!("failed to create a graphics device: {error}"))?;
    Ok((adapter, device, queue))
}
