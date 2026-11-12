use std::sync::Arc;

use grana_optics::Psf;
use grana_pipeline::Channels;
use grana_volume::Intensity;

use crate::task::Task;

pub(super) enum PendingChannel {
    AddRestore {
        channels: Arc<Channels>,
        source: usize,
        name: String,
        tint: [f32; 3],
        psf: Psf,
        iterations: u32,
        task: Task<Result<Intensity, String>>,
    },
    AddRatio {
        channels: Arc<Channels>,
        numerator: usize,
        denominator: usize,
        name: String,
        tint: [f32; 3],
        task: Task<Result<Intensity, String>>,
    },
    SetRatio {
        channels: Arc<Channels>,
        channel: usize,
        numerator: usize,
        denominator: usize,
        task: Task<Result<Intensity, String>>,
    },
    AddBleach {
        channels: Arc<Channels>,
        source: usize,
        name: String,
        tint: [f32; 3],
        task: Task<Result<Intensity, String>>,
    },
}

impl PendingChannel {
    pub(super) fn task(&self) -> &Task<Result<Intensity, String>> {
        match self {
            Self::AddRestore { task, .. } => task,
            Self::AddRatio { task, .. } => task,
            Self::SetRatio { task, .. } => task,
            Self::AddBleach { task, .. } => task,
        }
    }

    pub(super) fn channels(&self) -> &Arc<Channels> {
        match self {
            Self::AddRestore { channels, .. } => channels,
            Self::AddRatio { channels, .. } => channels,
            Self::SetRatio { channels, .. } => channels,
            Self::AddBleach { channels, .. } => channels,
        }
    }
}

pub(super) fn calibrate(
    channels: Arc<Channels>,
    work: impl FnOnce(&Channels) -> Result<Intensity, String> + Send + 'static,
) -> Task<Result<Intensity, String>> {
    Task::spawn(grana_exec::Role::Compute, move |cancel, sink| {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        sink(work(&channels));
    })
}
