use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use grana_volume::{BrickGrid, Level, Pyramid};
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, Queue};

use crate::atlas::Atlas;
use grana_volume::CHANNEL_LIMIT;

const MAXIMA_PLANE: usize = CHANNEL_LIMIT / 4;

pub(crate) const HOLLOW_FLAG: u32 = 2;

pub(crate) const RECORD_BYTES: u64 = size_of::<BrickRecord>() as u64;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct BrickRecord {
    pub slot: u32,
    pub flags: u32,
    pub time: u32,
    pub reserved: u32,
    pub maxima: [[f32; 4]; MAXIMA_PLANE],
}

const _: () = {
    assert!(size_of::<BrickRecord>() == 80);
    assert!(std::mem::offset_of!(BrickRecord, maxima) == 16);
};

impl BrickRecord {
    pub const MISSING: Self = Self {
        slot: 0,
        flags: 0,
        time: u32::MAX,
        reserved: 0,
        maxima: [[0.0; 4]; MAXIMA_PLANE],
    };

    pub const fn hollow(time: u32) -> Self {
        Self {
            slot: 0,
            flags: HOLLOW_FLAG,
            time,
            reserved: 0,
            maxima: [[0.0; 4]; MAXIMA_PLANE],
        }
    }

    pub fn new(slot: u32, time: u32, maxima: [[f32; 4]; MAXIMA_PLANE]) -> Self {
        Self {
            slot,
            flags: 0,
            time,
            reserved: 0,
            maxima,
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
        }
    }

    pub fn rate(&self, direction: Vec3) -> Vec3 {
        Vec3::new(
            self.inverses[0].dot(direction),
            self.inverses[1].dot(direction),
            self.inverses[2].dot(direction),
        )
    }

    pub fn record_base(&self) -> u32 {
        self.base
    }

    pub fn rebase(&mut self, queue: &Queue, time: u32, atlas: &Atlas, arena: &BrickArena) {
        for resident in atlas.residents(time, self.level) {
            let record = match resident.slot {
                Some(slot) => BrickRecord::new(slot + 1, time, resident.maxima),
                None => BrickRecord::hollow(time),
            };
            if self.records[resident.index] != record {
                self.set_record(queue, arena, resident.index, record);
            }
        }
        self.time = Some(time);
    }

    pub fn set_record(
        &mut self,
        queue: &Queue,
        arena: &BrickArena,
        index: usize,
        record: BrickRecord,
    ) {
        assert!(
            self.base as usize + index < arena.records(),
            "a brick record lies inside the arena"
        );
        self.records[index] = record;
        queue.write_buffer(
            arena.buffer(),
            (self.base as u64 + index as u64) * RECORD_BYTES,
            bytemuck::bytes_of(&record),
        );
    }

    pub fn drop_record(&mut self, queue: &Queue, arena: &BrickArena, index: usize, time: u32) {
        if self.records[index].time() == time {
            self.set_record(queue, arena, index, BrickRecord::MISSING);
        }
    }

    pub fn time(&self) -> Option<u32> {
        self.time
    }
}
