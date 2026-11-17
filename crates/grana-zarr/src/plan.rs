use crate::chunk::Chunk;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Located {
    pub(crate) frame: u32,
    pub(crate) coords: [u32; 3],
    pub(crate) origin: [u32; 3],
    pub(crate) length: [u32; 3],
}

pub(crate) struct Plan {
    level: usize,
    extent: [u32; 3],
    block: Chunk,
    counts: [u32; 3],
    frames: u32,
    channels: usize,
}

impl Plan {
    pub(crate) fn new(
        level: usize,
        extent: [u32; 3],
        block: Chunk,
        frames: u32,
        channels: usize,
    ) -> Self {
        assert!(
            extent.iter().all(|axis| *axis > 0),
            "a written level covers at least one voxel on every axis"
        );
        assert!(frames > 0, "a written level holds at least one frame");
        assert!(channels > 0, "a written level holds at least one channel");
        let shape = block.shape();
        let counts = std::array::from_fn(|axis| extent[axis].div_ceil(shape[axis]));
        Self {
            level,
            extent,
            block,
            counts,
            frames,
            channels,
        }
    }

    pub(crate) fn level(&self) -> usize {
        self.level
    }

    pub(crate) fn extent(&self) -> [u32; 3] {
        self.extent
    }

    pub(crate) fn block(&self) -> Chunk {
        self.block
    }

    pub(crate) fn channels(&self) -> usize {
        self.channels
    }

    pub(crate) fn count(&self) -> usize {
        let spatial = self.counts[0] as usize * self.counts[1] as usize * self.counts[2] as usize;
        self.frames as usize * spatial
    }

    pub(crate) fn locate(&self, index: usize) -> Located {
        let columns = self.counts[0] as usize;
        let planes = columns * self.counts[1] as usize;
        let per_frame = planes * self.counts[2] as usize;
        let frame = index / per_frame;
        let held = index % per_frame;
        let coords = [
            (held % columns) as u32,
            ((held % planes) / columns) as u32,
            (held / planes) as u32,
        ];
        let shape = self.block.shape();
        let origin = [
            coords[0] * shape[0],
            coords[1] * shape[1],
            coords[2] * shape[2],
        ];
        let length = std::array::from_fn(|axis| shape[axis].min(self.extent[axis] - origin[axis]));
        Located {
            frame: frame as u32,
            coords,
            origin,
            length,
        }
    }

    pub(crate) fn indices(&self, located: &Located) -> [u64; 5] {
        [
            u64::from(located.frame),
            0,
            u64::from(located.coords[2]),
            u64::from(located.coords[1]),
            u64::from(located.coords[0]),
        ]
    }
}
