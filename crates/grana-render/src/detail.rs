use grana_display::Camera;
use grana_volume::{Pyramid, Region};

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
