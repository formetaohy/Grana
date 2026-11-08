mod cpu;
mod gpu;

use std::sync::Mutex;
use std::sync::OnceLock;

use grana_optics::KernelBank;

pub struct Job<'a> {
    pub dims: [u32; 3],
    pub source: &'a [f32],
    pub first_plane: u32,
    pub iterations: u32,
}

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
        bank: &KernelBank,
        first_plane: u32,
        iterations: u32,
    ) -> Vec<f32> {
        let jobs = [Job {
            dims,
            source,
            first_plane,
            iterations,
        }];
        let mut restored = self.restore_batch(bank, &jobs);
        restored.pop().expect("one job restores one volume")
    }

    pub fn restore_batch(&self, bank: &KernelBank, jobs: &[Job<'_>]) -> Vec<Vec<f32>> {
        for job in jobs {
            assert!(
                job.iterations > 0 && job.iterations <= 1000,
                "richardson-lucy runs a bounded number of iterations"
            );
            assert!(
                job.dims.iter().all(|extent| *extent > 0),
                "a restored volume carries voxels"
            );
            assert_eq!(
                job.source.len(),
                (job.dims[0] * job.dims[1] * job.dims[2]) as usize,
                "a restored volume holds every requested voxel"
            );
        }
        if jobs.is_empty() {
            return Vec::new();
        }
        let mut held = self
            .graphics
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match held.as_mut() {
            Some(gpu) => gpu.restore_batch(bank, jobs),
            None => jobs
                .iter()
                .map(|job| {
                    cpu::restore(job.dims, job.source, bank, job.first_plane, job.iterations)
                })
                .collect(),
        }
    }
}

impl Default for Restorer {
    fn default() -> Self {
        Self::new()
    }
}
