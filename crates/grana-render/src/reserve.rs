use grana_volume::{BrickGrid, Pyramid};

use crate::brick::{BrickArena, RECORD_BYTES};
use crate::slots::Slots;

const SURFACE_SHARE: u64 = 8;

const RECORD_SHARE: u64 = 16;

const MINIMUM_SLOTS: u64 = 2;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Allocation {
    Display(u64),
    Export { slots: usize, ceiling: u64 },
}

impl Allocation {
    pub(crate) fn shrink(self, least: u64) -> Option<Self> {
        match self {
            Self::Display(budget) if budget > least => Some(Self::Display((budget / 2).max(least))),
            Self::Display(_) | Self::Export { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Reservation {
    slots: Slots,
    record_limit: usize,
    records: u64,
    surface: u64,
    atlas: u64,
    requested: u64,
}

impl Reservation {
    pub(crate) fn of(
        pyramid: &Pyramid,
        allocation: Allocation,
        slot_bytes: u64,
        max_dimension: u32,
    ) -> Result<Self, String> {
        match allocation {
            Allocation::Display(budget) => {
                Ok(Self::display(pyramid, budget, slot_bytes, max_dimension))
            }
            Allocation::Export { slots, ceiling } => {
                Self::export(pyramid, slots, slot_bytes, max_dimension, ceiling)
            }
        }
    }

    fn display(pyramid: &Pyramid, budget: u64, slot_bytes: u64, max_dimension: u32) -> Self {
        let record_limit = (budget / RECORD_SHARE / RECORD_BYTES).max(1) as usize;
        let (start, plans) = BrickArena::plan(pyramid, record_limit)
            .expect("the residency holds the coarsest brick level");
        assert!(
            start < pyramid.level_count(),
            "a planned arena names its first level"
        );
        let records = plans as u64 * RECORD_BYTES;
        let surface = budget / SURFACE_SHARE;
        let held = budget.saturating_sub(records.saturating_add(surface));
        let slots = Slots::covering(brick_count(pyramid), max_dimension)
            .ok()
            .filter(|slots| slots.capacity() <= (held / slot_bytes) as usize)
            .unwrap_or_else(|| Slots::fitting((held / slot_bytes) as usize, max_dimension));
        Self {
            slots,
            record_limit,
            records,
            surface,
            atlas: slots.capacity() as u64 * slot_bytes,
            requested: budget,
        }
    }

    fn export(
        pyramid: &Pyramid,
        wanted: usize,
        slot_bytes: u64,
        max_dimension: u32,
        ceiling: u64,
    ) -> Result<Self, String> {
        let slots = Slots::covering(wanted, max_dimension)?;
        let atlas = slots.capacity() as u64 * slot_bytes;
        let record_limit = brick_count(pyramid) + 1;
        let (_, records) = BrickArena::plan(pyramid, record_limit)
            .expect("an export arena covers every brick level");
        let records = records as u64 * RECORD_BYTES;
        let surface = (atlas + records) / (SURFACE_SHARE - 1);
        let bytes = atlas + records + surface;
        if bytes > ceiling {
            return Err(format!(
                "an export at this size needs {:.1} GiB of brick residency at full detail, above the {:.1} GiB export ceiling",
                bytes as f64 / (1u64 << 30) as f64,
                ceiling as f64 / (1u64 << 30) as f64
            ));
        }
        Ok(Self {
            slots,
            record_limit,
            records,
            surface,
            atlas,
            requested: bytes,
        })
    }

    pub(crate) fn bytes(self) -> u64 {
        self.records + self.surface + self.atlas
    }

    pub(crate) fn requested(self) -> u64 {
        self.requested
    }

    pub(crate) fn slots(self) -> Slots {
        self.slots
    }

    pub(crate) fn record_limit(self) -> usize {
        self.record_limit
    }

    pub(crate) fn records(self) -> u64 {
        self.records
    }

    pub(crate) fn surface(self) -> u64 {
        self.surface
    }

    pub(crate) fn atlas(self) -> u64 {
        self.atlas
    }
}

pub(crate) fn least(pyramid: &Pyramid, slot_bytes: u64) -> u64 {
    let coarsest = BrickGrid::new(pyramid.level(pyramid.level_count() - 1).extent()).count() as u64;
    slot_bytes * MINIMUM_SLOTS + coarsest * RECORD_BYTES
}

fn brick_count(pyramid: &Pyramid) -> usize {
    pyramid
        .levels()
        .iter()
        .map(|level| BrickGrid::new(level.extent()).count())
        .sum()
}
