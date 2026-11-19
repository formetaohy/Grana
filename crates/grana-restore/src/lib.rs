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

pub struct Restorer {
    core: Core,
}

enum Core {
    Graphics(Mutex<gpu::Gpu>),
    Processor,
}

impl Restorer {
    pub fn shared() -> Result<&'static Self, String> {
        static SHARED: OnceLock<Result<Restorer, String>> = OnceLock::new();
        SHARED
            .get_or_init(Self::graphics)
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn graphics() -> Result<Self, String> {
        Ok(Self {
            core: Core::Graphics(Mutex::new(gpu::Gpu::new()?)),
        })
    }

    pub fn processor() -> Self {
        Self {
            core: Core::Processor,
        }
    }

    pub fn restore(
        &self,
        dims: [u32; 3],
        source: &[f32],
        bank: &KernelBank,
        first_plane: u32,
        iterations: u32,
    ) -> Result<Vec<f32>, String> {
        let jobs = [Job {
            dims,
            source,
            first_plane,
            iterations,
        }];
        let mut restored = self.restore_batch(bank, &jobs)?;
        restored
            .pop()
            .ok_or_else(|| "the deconvolution restored no volume for the single job".to_string())
    }

    pub fn restore_batch(
        &self,
        bank: &KernelBank,
        jobs: &[Job<'_>],
    ) -> Result<Vec<Vec<f32>>, String> {
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
            return Ok(Vec::new());
        }
        match &self.core {
            Core::Graphics(graphics) => graphics
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .restore_batch(bank, jobs),
            Core::Processor => Ok(jobs
                .iter()
                .map(|job| {
                    cpu::restore(job.dims, job.source, bank, job.first_plane, job.iterations)
                })
                .collect()),
        }
    }
}
