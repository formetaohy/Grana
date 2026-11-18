use crate::schedule;

const ANALYSIS_BYTES: u64 = 2 << 30;

#[derive(Clone, Copy, Debug)]
pub struct Budget {
    workers: usize,
    bytes: u64,
}

impl Budget {
    pub fn new(workers: usize) -> Self {
        Self::with_bytes(workers, ANALYSIS_BYTES)
    }

    pub fn with_bytes(workers: usize, bytes: u64) -> Self {
        assert!(bytes > 0, "an analysis budget holds at least one byte");
        Self {
            workers: schedule::workers(workers),
            bytes,
        }
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn window(&self, unit: u64) -> usize {
        (self.bytes / unit.max(1)).max(1) as usize
    }

    pub fn split(&self, window: usize, total: u32) -> Self {
        let concurrent = self.workers.min(window).min(total.max(1) as usize);
        Self {
            workers: (self.workers / concurrent).max(1),
            bytes: (self.bytes / concurrent as u64).max(1),
        }
    }

    pub fn worker_bytes(&self) -> u64 {
        (self.bytes / self.workers as u64).max(1)
    }

    pub fn voxels(&self) -> u64 {
        self.bytes / 2
    }

    pub fn detection_bytes(&self) -> u64 {
        self.bytes * 3 / 8
    }

    pub fn band_bytes(&self) -> u64 {
        self.bytes / 32
    }

    pub fn tile_bytes(&self) -> u64 {
        self.bytes / 8
    }

    pub fn histogram_bytes(&self) -> u64 {
        self.bytes / 8
    }

    pub fn read_voxels(&self) -> u64 {
        self.bytes / 128
    }

    pub fn filament_voxels(&self) -> u64 {
        self.bytes / 64
    }
}
