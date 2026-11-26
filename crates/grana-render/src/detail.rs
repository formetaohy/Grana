use grana_display::Camera;
use grana_volume::{Pyramid, Region};

use crate::planner::{Counts, TargetView, ladder_count, volume_demand, volume_target};

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

pub fn residency(pyramid: &Pyramid, camera: &Camera, size: [u32; 2], region: Region) -> Residency {
    assert!(
        pyramid.level_count() > 0,
        "a pyramid holds at least one level"
    );
    assert!(
        size[0] > 0 && size[1] > 0,
        "an export covers a positive surface"
    );
    let height = size[1];
    let aspect = size[0] as f32 / height as f32;
    let target = volume_target(TargetView {
        pyramid,
        camera,
        height: height as f32,
        first: 0,
        current: None,
    });
    let count = ladder_count(
        camera.projection(),
        pyramid.level_count(),
        target.min(pyramid.level_count() - 1),
    );
    let bands = volume_demand(pyramid, camera, region, aspect, height, target, count);
    let bricks = bands[..count].iter().sum();
    Residency {
        detail: Detail {
            level: target,
            bricks,
        },
        slots: (bricks + 1).max(1),
    }
}

pub fn visible_bricks(
    pyramid: &Pyramid,
    camera: &Camera,
    size: [u32; 2],
    region: Region,
) -> Vec<Visible> {
    assert!(
        pyramid.level_count() > 0,
        "a pyramid holds at least one level"
    );
    assert!(
        size[0] > 0 && size[1] > 0,
        "an export covers a positive surface"
    );
    let height = size[1];
    let aspect = size[0] as f32 / height as f32;
    let target = volume_target(TargetView {
        pyramid,
        camera,
        height: height as f32,
        first: 0,
        current: None,
    })
    .min(pyramid.level_count() - 1);
    let count = ladder_count(camera.projection(), pyramid.level_count(), target);
    let counts = Counts::new(pyramid, camera, region, aspect, height, 0, None);
    (0..count)
        .map(|band| {
            let level = target + band;
            Visible {
                level,
                bricks: counts.bricks(level),
            }
        })
        .collect()
}
