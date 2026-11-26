use grana_volume::{BrickGrid, Pyramid};

use crate::atlas;
use crate::bake;
use crate::brick::{self, BrickArena};
use crate::occupancy;
use crate::planner;
use crate::shadow;
use crate::slots::Slots;
use crate::table;
use crate::upload::Upload;

const RECORD_SHARE: u64 = 16;

const SURFACE_SHARE: u64 = 8;

const MINIMUM_SLOTS: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum Charge {
    Atlas = 0,
    Records,
    Occupancy,
    Bake,
    Plan,
    Feedback,
    Shadow,
    Scene,
    Upload,
    Support,
    Surface,
    Frames,
    Splats,
}

impl Charge {
    pub(crate) const COUNT: usize = 13;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Charges([u64; Charge::COUNT]);

impl Charges {
    pub(crate) fn of(self, charge: Charge) -> u64 {
        self.0[charge as usize]
    }

    pub(crate) fn set(&mut self, charge: Charge, bytes: u64) {
        self.0[charge as usize] = bytes;
    }

    pub(crate) fn total(self) -> u64 {
        self.0.iter().sum()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Footprint {
    pub(crate) frames: u64,
    pub(crate) splats: u64,
    pub(crate) support: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Allocation {
    Display(u64),
    Export { slots: usize, ceiling: u64 },
}

impl Allocation {
    pub(crate) fn shrink(self, least: u64) -> Option<Self> {
        match self {
            Self::Display(budget) if budget > least => Some(Self::Display((budget / 2).max(least))),
            Self::Export { slots, ceiling } if slots > MINIMUM_SLOTS as usize => {
                Some(Self::Export {
                    slots: (slots / 2).max(MINIMUM_SLOTS as usize),
                    ceiling,
                })
            }
            Self::Display(_) | Self::Export { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Plan {
    target: u64,
    slots: Slots,
    record_limit: usize,
    slot_bytes: u64,
    surface: u64,
    charges: Charges,
}

impl Plan {
    pub(crate) fn of(
        pyramid: &Pyramid,
        allocation: Allocation,
        footprint: Footprint,
        max_dimension: u32,
    ) -> Result<Self, String> {
        let slot_bytes = slot_bytes(pyramid);
        match allocation {
            Allocation::Display(budget) => {
                let (slots, record_limit, surface) =
                    display(pyramid, budget, slot_bytes, max_dimension)?;
                let charges = dataset(pyramid, slots, record_limit);
                Ok(Self::assemble(
                    budget,
                    slots,
                    record_limit,
                    slot_bytes,
                    surface,
                    charges,
                    footprint,
                ))
            }
            Allocation::Export { slots, ceiling } => {
                let record_limit = brick_count(pyramid) + 1;
                let mut slots = slots
                    .max(MINIMUM_SLOTS as usize)
                    .min(Slots::limit(max_dimension).pow(3))
                    .min(brick_count(pyramid));
                loop {
                    let grid = Slots::covering(slots, max_dimension)?;
                    let charges = dataset(pyramid, grid, record_limit);
                    let surface = (charges.of(Charge::Atlas) + charges.of(Charge::Records))
                        / (SURFACE_SHARE - 1);
                    let total = charges.total() + surface;
                    if total <= ceiling {
                        return Ok(Self::assemble(
                            total,
                            grid,
                            record_limit,
                            slot_bytes,
                            surface,
                            charges,
                            footprint,
                        ));
                    }
                    if slots <= MINIMUM_SLOTS as usize {
                        return Err(format!(
                            "an export at this size needs {:.1} MiB of textures for a single brick and the export ceiling holds {:.1} MiB",
                            total as f64 / (1u64 << 20) as f64,
                            ceiling as f64 / (1u64 << 20) as f64
                        ));
                    }
                    slots = ((slots as u64).saturating_mul(ceiling) / total)
                        .clamp(MINIMUM_SLOTS, slots as u64 - 1)
                        as usize;
                }
            }
        }
    }

    fn assemble(
        target: u64,
        slots: Slots,
        record_limit: usize,
        slot_bytes: u64,
        surface: u64,
        mut charges: Charges,
        footprint: Footprint,
    ) -> Self {
        charges.set(Charge::Surface, surface);
        charges.set(Charge::Frames, footprint.frames);
        charges.set(Charge::Splats, footprint.splats);
        charges.set(Charge::Support, footprint.support);
        Self {
            target,
            slots,
            record_limit,
            slot_bytes,
            surface,
            charges,
        }
    }

    pub(crate) fn target(&self) -> u64 {
        self.target
    }

    pub(crate) fn slots(&self) -> Slots {
        self.slots
    }

    pub(crate) fn slot_bytes(&self) -> u64 {
        self.slot_bytes
    }

    pub(crate) fn record_limit(&self) -> usize {
        self.record_limit
    }

    pub(crate) fn surface(&self) -> u64 {
        self.surface
    }

    pub(crate) fn dataset(&self) -> u64 {
        self.charges.total()
            - self.charges.of(Charge::Surface)
            - self.charges.of(Charge::Frames)
            - self.charges.of(Charge::Splats)
            - self.charges.of(Charge::Support)
    }

    pub(crate) fn residency(&self) -> u64 {
        self.dataset() + self.surface
    }

    pub(crate) fn total(&self) -> u64 {
        self.charges.total()
    }
}

fn display(
    pyramid: &Pyramid,
    budget: u64,
    slot_bytes: u64,
    max_dimension: u32,
) -> Result<(Slots, usize, u64), String> {
    let limit = (budget / RECORD_SHARE / brick::RECORD_BYTES) as usize;
    let record_limit = limit.max(coarsest_count(pyramid));
    let (_, records) = BrickArena::plan(pyramid, record_limit).ok_or_else(|| {
        format!(
            "the brick residency cannot hold the {} records of the coarsest level",
            coarsest_count(pyramid)
        )
    })?;
    let surface = budget / SURFACE_SHARE;
    let held = budget
        .saturating_sub(brick::size(records))
        .saturating_sub(surface)
        .saturating_sub(floor(pyramid));
    let affordable = (held / per_slot(slot_bytes)).max(1);
    let affordable = usize::try_from(affordable).unwrap_or(usize::MAX);
    let slots = Slots::covering(brick_count(pyramid), max_dimension)
        .ok()
        .filter(|slots| slots.capacity() <= affordable)
        .unwrap_or_else(|| Slots::fitting(affordable, max_dimension));
    Ok((slots, record_limit, surface))
}

fn dataset(pyramid: &Pyramid, slots: Slots, record_limit: usize) -> Charges {
    let capacity = slots.capacity();
    let (_, records) = BrickArena::plan(pyramid, record_limit)
        .expect("a planned arena holds the records of its levels");
    let mut charges = Charges::default();
    charges.set(Charge::Atlas, atlas::bytes(slots, pyramid.channel_count()));
    charges.set(Charge::Records, brick::size(records));
    charges.set(Charge::Occupancy, occupancy::bytes(capacity));
    charges.set(Charge::Bake, bake::bytes(capacity));
    charges.set(Charge::Plan, planner::bytes(capacity));
    charges.set(Charge::Feedback, planner::readback_bytes(capacity));
    charges.set(Charge::Shadow, shadow::bytes(pyramid));
    charges.set(Charge::Scene, table::bytes(pyramid.channel_count()));
    charges.set(Charge::Upload, Upload::bytes());
    charges
}

fn per_slot(slot_bytes: u64) -> u64 {
    slot_bytes + occupancy::bytes(1) + bake::bytes(1) + planner::bytes(1) - planner::bytes(0)
        + planner::readback_bytes(1)
        - planner::readback_bytes(0)
}

fn floor(pyramid: &Pyramid) -> u64 {
    planner::bytes(0)
        + planner::readback_bytes(0)
        + shadow::bytes(pyramid)
        + table::bytes(pyramid.channel_count())
        + Upload::bytes()
}

pub(crate) fn slot_bytes(pyramid: &Pyramid) -> u64 {
    let channels = pyramid.channel_count();
    let source = (0..grana_volume::pack_count(channels))
        .map(|pack| grana_volume::texels(grana_volume::components(channels, pack)) as u64)
        .sum::<u64>()
        * (grana_volume::PADDED_SIDE as u64).pow(3)
        * size_of::<u16>() as u64;
    source + atlas::STRUCTURE_BYTES
}

pub(crate) fn least(pyramid: &Pyramid, slot_bytes: u64) -> u64 {
    slot_bytes * MINIMUM_SLOTS + coarsest_count(pyramid) as u64 * brick::RECORD_BYTES
}

fn coarsest_count(pyramid: &Pyramid) -> usize {
    BrickGrid::new(pyramid.level(pyramid.level_count() - 1).extent()).count()
}

fn brick_count(pyramid: &Pyramid) -> usize {
    pyramid
        .levels()
        .iter()
        .map(|level| BrickGrid::new(level.extent()).count())
        .sum()
}
