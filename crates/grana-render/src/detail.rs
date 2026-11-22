use grana_display::Camera;
use grana_volume::{PADDED_SIDE, Pyramid, Region};

use crate::planner::{Counts, FitView, LadderView, Limits, volume_ladder};

pub const EXPORT_CEILING: u64 = 4 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detail {
    pub level: usize,
    pub bricks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Residency {
    pub detail: Detail,
    pub slots: usize,
}

pub fn residency(pyramid: &Pyramid, camera: &Camera, size: [u32; 2], region: Region) -> Residency {
    assert!(
        pyramid.level_count() > 0,
        "a pyramid holds at least one level"
    );
    assert!(
        size[0] > 0 && size[1] > 0,
        "an export covers a positive surface"
    );
    let limits = Limits {
        first: 0,
        ceiling: pyramid.level_count() - 1,
    };
    let height = size[1];
    let aspect = size[0] as f32 / height as f32;
    let ladder = volume_ladder(LadderView {
        pyramid,
        camera,
        height: height as f32,
        current: None,
        limits,
    });
    let mut counts = Counts::new(
        FitView {
            pyramid,
            camera,
            region,
            aspect,
            height,
            limits,
            capacity: 1,
        },
        ladder.base,
    );
    let bands = counts.volume(ladder.base, ladder.count);
    let bricks = bands[..ladder.count].iter().sum();
    Residency {
        detail: Detail {
            level: ladder.base,
            bricks,
        },
        slots: (bricks + 1).max(1),
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
