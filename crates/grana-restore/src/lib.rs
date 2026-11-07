mod cpu;
mod gpu;

use std::sync::Mutex;
use std::sync::OnceLock;

use grana_optics::Kernel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Graphics,
    Processor,
}

impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Graphics => "graphics",
            Self::Processor => "processor",
        }
    }
}

pub struct Restorer {
    graphics: Mutex<Option<gpu::Gpu>>,
}

impl Restorer {
    pub fn shared() -> &'static Self {
        static SHARED: OnceLock<Restorer> = OnceLock::new();
        SHARED.get_or_init(Self::new)
    }

    pub fn new() -> Self {
        Self {
            graphics: Mutex::new(gpu::Gpu::new().ok()),
        }
    }

    pub fn processor() -> Self {
        Self {
            graphics: Mutex::new(None),
        }
    }

    pub fn backend(&self) -> Backend {
        let held = self
            .graphics
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match *held {
            Some(_) => Backend::Graphics,
            None => Backend::Processor,
        }
    }

    pub fn restore(
        &self,
        dims: [u32; 3],
        source: &[f32],
        kernel: &Kernel,
        iterations: u32,
    ) -> Vec<f32> {
        assert!(
            iterations > 0 && iterations <= 1000,
            "richardson-lucy runs a bounded number of iterations"
        );
        assert!(
            dims.iter().all(|extent| *extent > 0),
            "a restored volume carries voxels"
        );
        let mut held = self
            .graphics
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match held.as_mut() {
            Some(gpu) => gpu.restore(dims, source, kernel, iterations),
            None => cpu::restore(dims, source, kernel, iterations),
        }
    }
}

impl Default for Restorer {
    fn default() -> Self {
        Self::new()
    }
}
