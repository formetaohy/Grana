use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use grana_volume::{BrickGrid, Level, Pyramid};
use std::ops::Range;
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device};

use crate::atlas::Atlas;
use crate::upload::Upload;

pub(crate) const HOLLOW_FLAG: u32 = 2;

pub(crate) const RECORD_BYTES: u64 = size_of::<BrickRecord>() as u64;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct BrickRecord {
    pub slot: u32,
    pub flags: u32,
    pub time: u32,
    pub content: u32,
}

const _: () = {
    assert!(size_of::<BrickRecord>() == 16);
};

impl BrickRecord {
    pub const MISSING: Self = Self {
        slot: 0,
        flags: 0,
        time: u32::MAX,
        content: 0,
    };

    pub const fn hollow(time: u32, content: u32) -> Self {
        Self {
            slot: 0,
            flags: HOLLOW_FLAG,
            time,
            content,
        }
    }

    pub const fn new(slot: u32, time: u32, content: u32) -> Self {
        Self {
            slot,
            flags: 0,
            time,
            content,
        }
    }

    pub fn time(&self) -> u32 {
        self.time
    }
}

pub(crate) struct BrickArena {
    buffer: Buffer,
    bases: Vec<u32>,
    first: usize,
    records: usize,
}

impl BrickArena {
    pub fn new(device: &Device, pyramid: &Pyramid, limit: usize) -> Option<Self> {
        let counts = (0..pyramid.level_count())
            .map(|level| BrickGrid::new(pyramid.level(level).extent()).count())
            .collect::<Vec<_>>();
        let mut first = counts.len();
        let mut records = 0usize;
        for level in (0..counts.len()).rev() {
            if records + counts[level] > limit {
                break;
            }
            records += counts[level];
            first = level;
        }
        if first == counts.len() {
            return None;
        }
        assert!(records > 0, "a brick arena holds at least one record");
        let mut bases = vec![0u32; counts.len()];
        let mut base = 0usize;
        for level in first..counts.len() {
            bases[level] = base as u32;
            base += counts[level];
        }
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("grana.brick.records"),
            size: records as u64 * RECORD_BYTES,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Self {
            buffer,
            bases,
            first,
            records,
        })
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn first(&self) -> usize {
        self.first
    }

    pub fn records(&self) -> usize {
        self.records
    }

    pub fn base(&self, level: usize) -> u32 {
        assert!(
            level >= self.first && level < self.bases.len(),
            "the arena holds the records of a reachable level"
        );
        self.bases[level]
    }
}

pub(crate) struct BrickTable {
    pub grid: BrickGrid,
    pub origin: Vec3,
    pub bounds: (Vec3, Vec3),
    pub world: Vec3,
    pub dims: Vec3,
    pub voxel: Vec3,
    pub axes: [Vec3; 3],
    pub inverses: [Vec3; 3],
    pub atlas: [u32; 3],
    level: usize,
    base: u32,
    time: Option<u32>,
    records: Vec<BrickRecord>,
    dirty: Vec<Range<usize>>,
    residents: Vec<crate::atlas::Resident>,
    revision: u64,
}

impl BrickTable {
    pub fn new(level: usize, source: Level, atlas: [u32; 3], base: u32) -> Self {
        let grid = BrickGrid::new(source.extent());
        let (low, high) = source.bounds();
        let bounds = (Vec3::from(low), Vec3::from(high));
        let world = bounds.1 - bounds.0;
        let origin = Vec3::from(source.origin());
        let extent = source.extent();
        let dims = Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
        let basis = source.basis();
        let matrix = basis.matrix();
        let inverse = basis.inverse();
        let axes = [matrix.col(0), matrix.col(1), matrix.col(2)];
        let inverses = [inverse.row(0), inverse.row(1), inverse.row(2)];
        let voxel = Vec3::new(basis.step(0), basis.step(1), basis.step(2));
        Self {
            grid,
            origin,
            bounds,
            world,
            dims,
            voxel,
            axes,
            inverses,
            atlas,
            level,
            base,
            time: None,
            records: vec![BrickRecord::MISSING; grid.count()],
            dirty: Vec::new(),
            residents: Vec::new(),
            revision: 0,
        }
    }

    pub fn record_base(&self) -> u32 {
        self.base
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn rebase(
        &mut self,
        upload: &mut Upload,
        time: u32,
        content: u32,
        atlas: &Atlas,
        arena: &BrickArena,
    ) {
        let Self {
            level,
            records,
            dirty,
            residents,
            ..
        } = self;
        atlas.residents(time, *level, residents);
        for resident in residents.iter() {
            let record = match resident.slot {
                Some(slot) => BrickRecord::new(slot + 1, time, content),
                None => BrickRecord::hollow(time, content),
            };
            if records[resident.index] != record {
                records[resident.index] = record;
                dirty.push(resident.index..resident.index + 1);
            }
        }
        self.flush(upload, arena);
        self.time = Some(time);
    }

    fn mark(&mut self, index: usize) {
        self.dirty.push(index..index + 1);
    }

    pub fn flush(&mut self, upload: &mut Upload, arena: &BrickArena) {
        if self.dirty.is_empty() {
            return;
        }
        self.dirty.sort_unstable_by_key(|range| range.start);
        let mut runs: Vec<Range<usize>> = Vec::new();
        for range in self.dirty.drain(..) {
            match runs.last_mut() {
                Some(held) if range.start <= held.end => held.end = held.end.max(range.end),
                _ => runs.push(range),
            }
        }
        assert!(
            runs.iter()
                .all(|run| !run.is_empty() && self.base as usize + run.end <= arena.records()),
            "a brick record run lies inside the arena"
        );
        self.revision += 1;
        for run in runs {
            upload.buffer(
                arena.buffer(),
                (self.base as u64 + run.start as u64) * RECORD_BYTES,
                bytemuck::cast_slice(&self.records[run]),
            );
        }
    }

    pub fn set_record(&mut self, arena: &BrickArena, index: usize, record: BrickRecord) {
        assert!(
            self.base as usize + index < arena.records(),
            "a brick record lies inside the arena"
        );
        if self.records[index] == record {
            return;
        }
        self.records[index] = record;
        self.mark(index);
    }

    pub fn drop_record(&mut self, arena: &BrickArena, index: usize, time: u32) {
        if self.records[index].time() == time {
            self.set_record(arena, index, BrickRecord::MISSING);
        }
    }

    pub fn time(&self) -> Option<u32> {
        self.time
    }
}
