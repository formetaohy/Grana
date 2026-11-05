use std::ops::Range;

pub(super) const TILE: [u32; 3] = [128, 128, 16];

#[derive(Clone)]
pub(super) struct Grid {
    ranges: [Range<u32>; 3],
    counts: [usize; 3],
}

impl Grid {
    pub(super) fn new(ranges: [Range<u32>; 3]) -> Self {
        assert!(
            ranges.iter().all(|range| range.start <= range.end),
            "a colocalization grid covers an ordered region"
        );
        let counts = [0, 1, 2]
            .map(|axis| (ranges[axis].end - ranges[axis].start).div_ceil(TILE[axis]) as usize);
        Self { ranges, counts }
    }

    pub(super) fn ranges(&self) -> &[Range<u32>; 3] {
        &self.ranges
    }

    pub(super) fn tile_count(&self) -> usize {
        self.counts[0] * self.counts[1] * self.counts[2]
    }

    pub(super) fn tile(&self, index: usize) -> [Range<u32>; 3] {
        assert!(
            index < self.tile_count(),
            "the tile index lies on the colocalization grid"
        );
        let x = index % self.counts[0];
        let y = (index / self.counts[0]) % self.counts[1];
        let z = index / (self.counts[0] * self.counts[1]);
        [0, 1, 2].map(|axis| {
            let start = self.ranges[axis].start + [x, y, z][axis] as u32 * TILE[axis];
            start..(start + TILE[axis]).min(self.ranges[axis].end)
        })
    }
}
