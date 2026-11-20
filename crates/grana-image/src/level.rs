use std::ops::Range;

use grana_volume::{BRICK_SIDE, Extent, Pyramid};

const RESOLVED_LIMIT: usize = 8;

pub(crate) fn extended(pyramid: &Pyramid) -> Pyramid {
    let mut levels = pyramid.levels().to_vec();
    let stored = levels.len();
    while levels.len() - stored < RESOLVED_LIMIT {
        let last = *levels.last().expect("a source holds at least one level");
        if last.extent().max_dimension() <= BRICK_SIDE {
            break;
        }
        levels.push(last.coarser());
    }
    Pyramid::placed(
        levels,
        stored,
        pyramid.channels().to_vec(),
        pyramid.frame_count(),
        pyramid.frame_interval(),
        pyramid.placement(),
        pyramid.calibration(),
    )
}

pub(crate) struct Scale {
    source: usize,
    shift: u32,
    source_extent: Extent,
}

impl Scale {
    pub(crate) fn of(pyramid: &Pyramid, level: usize) -> Self {
        let stored = pyramid.stored();
        assert!(level >= stored, "only resolved levels carry a scale");
        Self {
            source: stored - 1,
            shift: u32::try_from(level - stored + 1)
                .expect("the pyramid holds fewer than 4G levels"),
            source_extent: pyramid.level(stored - 1).extent(),
        }
    }

    pub(crate) fn source(&self) -> usize {
        self.source
    }

    pub(crate) fn source_ranges(&self, ranges: &[Range<u32>; 3]) -> [Range<u32>; 3] {
        let ratio = 1u32 << self.shift;
        [
            clamped(&ranges[0], ratio, self.source_extent.x()),
            clamped(&ranges[1], ratio, self.source_extent.y()),
            clamped(&ranges[2], ratio, self.source_extent.z()),
        ]
    }
}

fn clamped(range: &Range<u32>, ratio: u32, limit: u32) -> Range<u32> {
    let start = (range.start * ratio).min(limit);
    let end = (range.end * ratio).min(limit);
    start..end.max(start)
}
