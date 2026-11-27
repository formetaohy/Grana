use grana_display::Camera;
use grana_volume::{Pyramid, Region};

use crate::planner::{TargetView, volume_bricks, volume_count, volume_target};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Visible {
    pub level: usize,
    pub bricks: Vec<u32>,
}

fn target(pyramid: &Pyramid, camera: &Camera, size: [u32; 2]) -> usize {
    assert!(
        pyramid.level_count() > 0,
        "a pyramid holds at least one level"
    );
    assert!(
        size[0] > 0 && size[1] > 0,
        "an export covers a positive surface"
    );
    volume_target(TargetView {
        pyramid,
        camera,
        height: size[1] as f32,
        first: 0,
        kept: None,
    })
}

pub fn residency(pyramid: &Pyramid, camera: &Camera, size: [u32; 2], region: Region) -> Residency {
    let level = target(pyramid, camera, size);
    let aspect = size[0] as f32 / size[1] as f32;
    let bricks = volume_count(pyramid, camera, region, aspect, level, None);
    Residency {
        detail: Detail { level, bricks },
        slots: (bricks + 1).max(1),
    }
}

pub fn visible(pyramid: &Pyramid, camera: &Camera, size: [u32; 2], region: Region) -> Visible {
    let level = target(pyramid, camera, size);
    let aspect = size[0] as f32 / size[1] as f32;
    Visible {
        level,
        bricks: volume_bricks(pyramid, camera, region, aspect, level),
    }
}
