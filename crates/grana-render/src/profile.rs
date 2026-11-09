use std::collections::VecDeque;

use grana_gpu::Readback;
use wgpu::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoder, ComputePassDescriptor,
    ComputePassTimestampWrites, Device, Features, QuerySet, QuerySetDescriptor, QueryType, Queue,
};

const MARK_LIMIT: usize = 16;
const STAMP_BYTES: u64 = size_of::<u64>() as u64;
const SLOTS: usize = 4;
const SMOOTHING: f32 = 0.3;
const OVERRUN: f32 = 1.1;
const HEADROOM: f32 = 0.7;
const STEP: f32 = 1.15;
const PENALTY_FLOOR: f32 = 1.0;
const PENALTY_CEILING: f32 = 4.0;
const NANOSECONDS: f32 = 1.0e6;

const FEATURES: Features = Features::TIMESTAMP_QUERY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Plan,
    Cull,
    Shadow,
    Geometry,
    Oit,
    Volume,
    Section,
    Cut,
    Present,
}

pub const STAGE_COUNT: usize = 9;

impl Stage {
    pub const ALL: [Stage; STAGE_COUNT] = [
        Stage::Plan,
        Stage::Cull,
        Stage::Shadow,
        Stage::Geometry,
        Stage::Oit,
        Stage::Volume,
        Stage::Section,
        Stage::Cut,
        Stage::Present,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "Plan",
            Self::Cull => "Cull",
            Self::Shadow => "Shadow",
            Self::Geometry => "Geometry",
            Self::Oit => "Transparency",
            Self::Volume => "Volume",
            Self::Section => "Section",
            Self::Cut => "Cut",
            Self::Present => "Present",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameProfile {
    pub stages: [f32; STAGE_COUNT],
    pub total: f32,
    pub target: f32,
    pub penalty: f32,
}

impl FrameProfile {
    pub fn stage(self, stage: Stage) -> f32 {
        self.stages[stage.index()]
    }

    pub fn measured(self) -> bool {
        self.total > 0.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Budget {
    target: f32,
    measured: f32,
    penalty: f32,
}

impl Budget {
    pub fn new(target: f32) -> Self {
        assert!(
            target.is_finite() && target > 0.0,
            "a frame budget is a positive duration"
        );
        Self {
            target,
            measured: target,
            penalty: 1.0,
        }
    }

    pub fn target(self) -> f32 {
        self.target
    }

    pub fn measured(self) -> f32 {
        self.measured
    }

    pub fn penalty(self) -> f32 {
        self.penalty
    }

    pub fn observe(&mut self, elapsed: f32) {
        assert!(
            elapsed.is_finite() && elapsed >= 0.0,
            "a measured frame spans a finite duration"
        );
        self.measured += SMOOTHING * (elapsed - self.measured);
        if self.measured > self.target * OVERRUN {
            self.penalty = (self.penalty * STEP).min(PENALTY_CEILING);
        } else if self.measured < self.target * HEADROOM {
            self.penalty = (self.penalty / STEP).max(PENALTY_FLOOR);
        }
    }
}

#[derive(Default)]
pub struct Marks {
    phases: Vec<Stage>,
}

impl Marks {
    pub fn new() -> Self {
        Self::default()
    }
}

pub struct Profiler {
    queries: QuerySet,
    resolve: Buffer,
    readback: Readback,
    period: f32,
    frames: VecDeque<Vec<Stage>>,
}

impl Profiler {
    pub fn new(device: &Device, queue: &Queue) -> Option<Self> {
        if !device.features().contains(FEATURES) {
            return None;
        }
        let queries = device.create_query_set(&QuerySetDescriptor {
            label: Some("grana.profile"),
            ty: QueryType::Timestamp,
            count: MARK_LIMIT as u32 + 1,
        });
        let resolve = device.create_buffer(&BufferDescriptor {
            label: Some("grana.profile.resolve"),
            size: (MARK_LIMIT as u64 + 1) * STAMP_BYTES,
            usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = Readback::new(
            device,
            "grana.profile.readback",
            (MARK_LIMIT as u64 + 1) * STAMP_BYTES,
            SLOTS,
        );
        Some(Self {
            queries,
            resolve,
            readback,
            period: queue.get_timestamp_period(),
            frames: VecDeque::new(),
        })
    }

    pub fn mark(&self, encoder: &mut CommandEncoder, stage: Stage, marks: &mut Marks) {
        if marks.phases.len() >= MARK_LIMIT {
            return;
        }
        self.stamp(encoder, marks.phases.len() as u32);
        marks.phases.push(stage);
    }

    pub fn finish(&mut self, encoder: &mut CommandEncoder, marks: Marks) {
        let count = marks.phases.len();
        if count < 2 {
            return;
        }
        self.stamp(encoder, count as u32);
        let Some(slot) = self.readback.reserve() else {
            return;
        };
        encoder.resolve_query_set(&self.queries, 0..count as u32 + 1, &self.resolve, 0);
        self.readback.stage(
            encoder,
            slot,
            0,
            &self.resolve,
            (count as u64 + 1) * STAMP_BYTES,
        );
        self.frames.push_back(marks.phases);
    }

    fn stamp(&self, encoder: &mut CommandEncoder, index: u32) {
        let _ = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("grana.profile.mark"),
            timestamp_writes: Some(ComputePassTimestampWrites {
                query_set: &self.queries,
                beginning_of_pass_write_index: None,
                end_of_pass_write_index: Some(index),
            }),
        });
    }

    pub fn register(&mut self) {
        self.readback.register();
    }

    pub fn poll(&mut self, device: &Device) -> Option<FrameProfile> {
        let (_, bytes) = self.readback.take(device)?;
        let phases = self.frames.pop_front()?;
        let (stamps, _) = bytes.as_chunks::<8>();
        let stamps = stamps
            .iter()
            .map(|chunk| u64::from_le_bytes(*chunk))
            .collect::<Vec<_>>();
        let mut stages = [0.0f32; STAGE_COUNT];
        for (index, stage) in phases.iter().enumerate() {
            let Some(end) = stamps.get(index + 1) else {
                break;
            };
            let span = end.saturating_sub(stamps[index]);
            stages[stage.index()] += span as f32 * self.period / NANOSECONDS;
        }
        let total = stages.iter().sum();
        Some(FrameProfile {
            stages,
            total,
            target: 0.0,
            penalty: 0.0,
        })
    }
}
