use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use wgpu::{
    Adapter, Backends, CurrentSurfaceTexture, Device, DeviceDescriptor, ExperimentalFeatures,
    Instance, InstanceDescriptor, MemoryHints, PowerPreference, PresentMode, Queue,
    RequestAdapterOptions, Surface, SurfaceColorSpace, SurfaceConfiguration, SurfaceTarget,
    TextureFormat, TextureUsages, Trace,
};

use crate::Pipelines;
use crate::REQUIRED_FEATURES;
use crate::ledger::Ledger;
use crate::memory::Memory;

pub const HEADLESS_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

const BACKEND_ORDER: &[Backends] = &[Backends::VULKAN];

pub struct Graphics {
    instance: Instance,
    adapter: Adapter,
    device: Device,
    queue: Queue,
    memory: Option<Memory>,
    ledger: Arc<Ledger>,
    lost: Lost,
    pipelines: Pipelines,
}

pub struct Gpu {
    pub surface: Surface<'static>,
    pub device: Device,
    pub queue: Queue,
    pub config: SurfaceConfiguration,
    pub adapter: String,
    graphics: Arc<Graphics>,
}

pub struct Headless {
    pub device: Device,
    pub queue: Queue,
    graphics: Arc<Graphics>,
}

#[derive(Clone, Default)]
pub struct Lost(Arc<Mutex<LostState>>);

#[derive(Default)]
struct LostState {
    reason: Option<String>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Lost {
    fn watch(device: &Device) -> Self {
        let lost = Self::default();
        let held = lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            held.record(format!(
                "the graphics device was lost ({reason:?}) and cannot be recreated: {message}"
            ));
        });
        let held = lost.clone();
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            if held.reason().is_some() {
                return;
            }
            panic!("the graphics device reported an uncaptured error: {error}");
        }));
        lost
    }

    pub fn wake_with(&self, wake: impl Fn() + Send + Sync + 'static) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.wake = Some(Arc::new(wake));
    }

    fn record(&self, detail: String) {
        let wake = {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            if state.reason.is_some() {
                return;
            }
            grana_log::error!(target: "grana::gpu", %detail, "the graphics device was lost");
            state.reason = Some(detail);
            state.wake.clone()
        };
        if let Some(wake) = wake {
            wake();
        }
    }

    pub fn reason(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .reason
            .clone()
    }
}

impl Graphics {
    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn adapter(&self) -> &Adapter {
        &self.adapter
    }

    pub fn backend(&self) -> wgpu::Backend {
        self.adapter.get_info().backend
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    pub fn memory(&self) -> Option<&Memory> {
        self.memory.as_ref()
    }

    pub fn ledger(&self) -> &Arc<Ledger> {
        &self.ledger
    }

    pub fn lost(&self) -> Option<String> {
        self.lost.reason()
    }

    pub fn lost_handle(&self) -> &Lost {
        &self.lost
    }

    pub fn pipelines(&self) -> &Pipelines {
        &self.pipelines
    }

    pub fn description(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} · {:?}", info.name, info.backend)
    }
}

impl Headless {
    pub fn shared() -> Result<Self, String> {
        let graphics = open(None)?;
        Ok(Self {
            device: graphics.device.clone(),
            queue: graphics.queue.clone(),
            graphics,
        })
    }

    pub fn graphics(&self) -> &Graphics {
        &self.graphics
    }

    pub fn format(&self) -> TextureFormat {
        HEADLESS_FORMAT
    }

    pub fn memory(&self) -> Option<&Memory> {
        self.graphics.memory()
    }

    pub fn ledger(&self) -> &Arc<Ledger> {
        self.graphics.ledger()
    }

    pub fn lost(&self) -> Option<String> {
        self.graphics.lost()
    }

    pub fn lost_handle(&self) -> &Lost {
        self.graphics.lost_handle()
    }

    pub fn pipelines(&self) -> &Pipelines {
        self.graphics.pipelines()
    }
}

impl Gpu {
    pub fn new<W>(window: W, size: [u32; 2]) -> Self
    where
        W: Into<SurfaceTarget<'static>> + Clone,
    {
        let surface = instance()
            .create_surface(window)
            .expect("failed to create a window surface");
        let graphics = open(Some(&surface)).unwrap_or_else(|detail| {
            panic!("no graphics adapter can render grana ({detail})");
        });
        Self::configure(graphics, surface, size)
    }

    fn configure(graphics: Arc<Graphics>, surface: Surface<'static>, size: [u32; 2]) -> Self {
        let description = graphics.description();
        println!("grana: using {description}");
        let capabilities = surface.get_capabilities(graphics.adapter());
        let Some(format) = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
        else {
            panic!(
                "the window surface offers no linear colour format for display space bytes ({:?})",
                capabilities.formats
            );
        };
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
        surface.configure(&graphics.device, &config);
        Self {
            surface,
            device: graphics.device.clone(),
            queue: graphics.queue.clone(),
            config,
            adapter: description,
            graphics,
        }
    }

    pub fn graphics(&self) -> &Graphics {
        &self.graphics
    }

    pub fn format(&self) -> TextureFormat {
        self.config.format
    }

    pub fn memory(&self) -> Option<&Memory> {
        self.graphics.memory()
    }

    pub fn ledger(&self) -> &Arc<Ledger> {
        self.graphics.ledger()
    }

    pub fn lost(&self) -> Option<String> {
        self.graphics.lost()
    }

    pub fn lost_handle(&self) -> &Lost {
        self.graphics.lost_handle()
    }

    pub fn pipelines(&self) -> &Pipelines {
        self.graphics.pipelines()
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

fn held() -> MutexGuard<'static, Option<Arc<Graphics>>> {
    static HELD: OnceLock<Mutex<Option<Arc<Graphics>>>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn open(surface: Option<&Surface<'static>>) -> Result<Arc<Graphics>, String> {
    let mut held = held();
    if let Some(graphics) = held.as_ref() {
        if let Some(surface) = surface
            && !graphics.adapter.is_surface_supported(surface)
        {
            return Err(
                "the shared graphics adapter cannot present to this window; the process already holds a device without surface support"
                    .to_string(),
            );
        }
        return Ok(Arc::clone(graphics));
    }
    let graphics = acquire(surface)?;
    let opened = Arc::new(graphics);
    *held = Some(Arc::clone(&opened));
    Ok(opened)
}

fn acquire(surface: Option<&Surface<'static>>) -> Result<Graphics, String> {
    let mut failures = Vec::new();
    for fallback in [false, true] {
        match request(instance(), surface, fallback) {
            Ok(graphics) => return Ok(graphics),
            Err(detail) => failures.push(detail),
        }
    }
    Err(format!(
        "no graphics adapter is available ({})",
        failures.join("; ")
    ))
}

fn instance() -> &'static Instance {
    static INSTANCE: OnceLock<Instance> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        Instance::new(InstanceDescriptor {
            backends: BACKEND_ORDER[0],
            ..InstanceDescriptor::new_without_display_handle()
        })
    })
}

fn request(
    instance: &Instance,
    surface: Option<&Surface<'static>>,
    fallback: bool,
) -> Result<Graphics, String> {
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
    let features = adapter.features() - wgpu::Features::all_experimental_mask();
    let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
        label: Some("grana"),
        required_features: features,
        required_limits: adapter.limits(),
        experimental_features: ExperimentalFeatures::disabled(),
        memory_hints: MemoryHints::MemoryUsage,
        trace: Trace::Off,
    }))
    .map_err(|error| format!("failed to create a graphics device: {error}"))?;
    let info = adapter.get_info();
    let memory = Memory::of(&info);
    grana_log::info!(
        target: "grana::gpu",
        adapter = %info.name,
        backend = ?info.backend,
        "graphics device ready"
    );
    let lost = Lost::watch(&device);
    let ledger = Ledger::of(memory.as_ref());
    let pipelines = Pipelines::new(&device);
    Ok(Graphics {
        instance: instance.clone(),
        adapter,
        device,
        queue,
        memory,
        ledger,
        lost,
        pipelines,
    })
}
