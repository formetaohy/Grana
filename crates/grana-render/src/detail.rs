use grana_volume::{PADDED_SIDE, Pyramid, Region};

use crate::PANE_COUNT;
use crate::PaneFrame;
use crate::planner::{
    LadderView, Limits, section_brick_count, select_section_level, volume_counts, volume_ladder,
};

pub const EXPORT_CEILING: u64 = 4 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detail {
    pub level: usize,
    pub bricks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Residency {
    pub panes: [Option<Detail>; PANE_COUNT],
    pub slots: usize,
}

#[derive(Clone, Copy, Default)]
struct Spread {
    count: usize,
    total: usize,
    widest: usize,
}

impl Spread {
    fn push(&mut self, bricks: usize) {
        self.count += 1;
        self.total += bricks;
        self.widest = self.widest.max(bricks);
    }
}

pub fn residency(
    pyramid: &Pyramid,
    panes: &[Option<PaneFrame<'_>>; PANE_COUNT],
    region: Region,
) -> Residency {
    assert!(
        pyramid.level_count() > 0,
        "a pyramid holds at least one level"
    );
    let limits = Limits {
        first: 0,
        ceiling: pyramid.level_count() - 1,
    };
    let mut details = [None; PANE_COUNT];
    let mut sections = Spread::default();
    let mut volumes = Spread::default();
    for (index, pane) in panes.iter().enumerate() {
        let Some(pane) = pane else {
            continue;
        };
        let [_, _, width, height] = pane.region();
        let height = height.max(1);
        let aspect = width.max(1) as f32 / height as f32;
        let detail = match pane {
            PaneFrame::Volume { camera, .. } => {
                let ladder = volume_ladder(LadderView {
                    pyramid,
                    camera,
                    height: height as f32,
                    current: None,
                    limits,
                    penalty: 1.0,
                });
                let counts = volume_counts(
                    pyramid,
                    ladder.base,
                    ladder.count,
                    camera,
                    aspect,
                    height as f32,
                    region,
                );
                let bricks = counts[..ladder.count].iter().sum();
                volumes.push(bricks);
                Detail {
                    level: ladder.base,
                    bricks,
                }
            }
            PaneFrame::Section { section, .. } => {
                let level =
                    select_section_level(pyramid, section, height as f32, None, limits, 1.0);
                let bricks = section_brick_count(pyramid, level, section, aspect, region);
                sections.push(bricks);
                Detail { level, bricks }
            }
        };
        details[index] = Some(detail);
    }
    let slots = (sections.count * sections.widest)
        .max(sections.total + volumes.count * volumes.widest + 1)
        .max(1);
    Residency {
        panes: details,
        slots,
    }
}

pub fn residency_budget(
    slots: usize,
    slot_bytes: u64,
    max_dimension: u32,
    ceiling: u64,
) -> Result<u64, String> {
    assert!(slots > 0, "an export reserves at least one brick slot");
    assert!(slot_bytes > 0, "a brick slot holds at least one byte");
    let mut axis = 1u32;
    while (axis as u64).pow(3) < slots as u64 {
        axis += 1;
    }
    let limit = max_dimension / PADDED_SIDE;
    let held = (limit as u64).pow(3);
    if axis as u64 > limit as u64 {
        return Err(format!(
            "an export at this size needs {slots} brick slots at full detail, but this device holds at most {held}"
        ));
    }
    let bytes = (axis as u64).pow(3) * slot_bytes;
    if bytes > ceiling {
        return Err(format!(
            "an export at this size needs {:.1} GiB of brick residency at full detail, above the {:.1} GiB export ceiling",
            bytes as f64 / (1u64 << 30) as f64,
            ceiling as f64 / (1u64 << 30) as f64
        ));
    }
    Ok(bytes)
}
