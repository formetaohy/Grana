use glam::Vec3;

use crate::{Basis, Calibration, ChannelInfo, Level, Region, Spacing};

#[derive(Clone, Debug, PartialEq)]
pub struct Pyramid {
    levels: Vec<Level>,
    stored: usize,
    channels: Vec<ChannelInfo>,
    frames: u32,
    frame_interval: f32,
    placement: [f32; 3],
    calibration: Calibration,
}

impl Pyramid {
    pub fn new(
        levels: Vec<Level>,
        stored: usize,
        channels: Vec<ChannelInfo>,
        frames: u32,
        frame_interval: f32,
        calibration: Calibration,
    ) -> Self {
        Self::placed(
            levels,
            stored,
            channels,
            frames,
            frame_interval,
            [0.0; 3],
            calibration,
        )
    }

    pub fn placed(
        levels: Vec<Level>,
        stored: usize,
        channels: Vec<ChannelInfo>,
        frames: u32,
        frame_interval: f32,
        placement: [f32; 3],
        calibration: Calibration,
    ) -> Self {
        assert!(!levels.is_empty(), "a pyramid needs at least one level");
        assert!(
            stored > 0 && stored <= levels.len(),
            "the stored levels form a prefix of the pyramid"
        );
        assert!(!channels.is_empty(), "a pyramid needs at least one channel");
        assert!(frames > 0, "a pyramid holds at least one frame");
        assert!(
            frame_interval.is_finite() && frame_interval > 0.0,
            "the frame interval is a positive duration"
        );
        assert!(
            placement.iter().all(|value| value.is_finite()),
            "a pyramid placement is finite"
        );
        for pair in levels.windows(2) {
            let (fine, coarse) = (pair[0], pair[1]);
            let fine_extent = fine.extent();
            let coarse_extent = coarse.extent();
            assert!(
                fine_extent.x() >= coarse_extent.x()
                    && fine_extent.y() >= coarse_extent.y()
                    && fine_extent.z() >= coarse_extent.z(),
                "pyramid levels must shrink with every step"
            );
            let fine_spacing = fine.spacing();
            let coarse_spacing = coarse.spacing();
            assert!(
                fine_spacing.x() <= coarse_spacing.x()
                    && fine_spacing.y() <= coarse_spacing.y()
                    && fine_spacing.z() <= coarse_spacing.z(),
                "pyramid voxels must grow with every step"
            );
        }
        Self {
            levels,
            stored,
            channels,
            frames,
            frame_interval,
            placement,
            calibration,
        }
    }

    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    pub fn level(&self, index: usize) -> Level {
        self.levels[index]
    }

    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    pub fn stored(&self) -> usize {
        self.stored
    }

    pub fn placement(&self) -> [f32; 3] {
        self.placement
    }

    pub fn calibration(&self) -> Calibration {
        self.calibration
    }

    pub fn with_calibration(mut self, calibration: Calibration) -> Pyramid {
        self.calibration = calibration;
        self
    }

    pub fn rescaled(&self, spacing: Spacing, calibration: Calibration) -> Pyramid {
        let base = self.levels[0].spacing();
        if base == spacing {
            return self.clone().with_calibration(calibration);
        }
        let factor = [
            spacing.x() / base.x(),
            spacing.y() / base.y(),
            spacing.z() / base.z(),
        ];
        Pyramid {
            levels: self
                .levels
                .iter()
                .map(|level| rescale_level(*level, factor))
                .collect(),
            stored: self.stored,
            channels: self.channels.clone(),
            frames: self.frames,
            frame_interval: self.frame_interval,
            placement: self.placement,
            calibration,
        }
    }

    pub fn channels(&self) -> &[ChannelInfo] {
        &self.channels
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn with_channels(&self, channels: Vec<ChannelInfo>) -> Pyramid {
        assert!(!channels.is_empty(), "a pyramid needs at least one channel");
        Pyramid {
            levels: self.levels.clone(),
            stored: self.stored,
            channels,
            frames: self.frames,
            frame_interval: self.frame_interval,
            placement: self.placement,
            calibration: self.calibration,
        }
    }

    pub fn frame_count(&self) -> u32 {
        self.frames
    }

    pub fn brick_bytes(&self) -> u64 {
        let voxels = u64::from(crate::PADDED_SIDE).pow(3);
        let channel: u64 = self
            .channels
            .iter()
            .map(|info| info.format().bytes() as u64)
            .sum();
        voxels * channel
    }

    pub fn frame_interval(&self) -> f32 {
        self.frame_interval
    }

    pub fn fit_level(&self, region: Region, voxels: u64) -> Option<usize> {
        self.fit_level_by(voxels, |index| region.voxels(self.levels[index]))
    }

    pub fn fit_level_by(&self, voxels: u64, cost: impl Fn(usize) -> Option<u64>) -> Option<usize> {
        assert!(voxels > 0, "a level budget covers at least one voxel");
        (0..self.levels.len()).find(|index| cost(*index).is_none_or(|cost| cost <= voxels))
    }
}

fn rescale_level(level: Level, factor: [f32; 3]) -> Level {
    let basis = level.basis();
    let columns = std::array::from_fn(|axis| {
        let column = basis.column(axis);
        [
            column[0] * factor[axis],
            column[1] * factor[axis],
            column[2] * factor[axis],
        ]
    });
    let next = Basis::new(columns);
    let extent = level.extent();
    let reach = Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
    let center = Vec3::from(level.center());
    let origin = center - next.matrix() * (0.5 * reach);
    Level::placed(extent, next, origin.to_array())
}
