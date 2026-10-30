use crate::{Extent, Intensity, Level, Spacing, VoxelData, VoxelFormat};

#[derive(Clone, Debug)]
pub struct Volume {
    extent: Extent,
    spacing: Spacing,
    intensity: Intensity,
    levels: Vec<Level>,
}

impl Volume {
    pub fn new(extent: Extent, spacing: Spacing, data: VoxelData) -> Self {
        let intensity = data.range();
        let mut levels = vec![Level::new(extent, data)];
        while levels
            .last()
            .expect("a volume always holds level zero")
            .extent()
            .max_dimension()
            > 1
        {
            let next = levels
                .last()
                .expect("a volume always holds level zero")
                .downsampled();
            levels.push(next);
        }
        Self {
            extent,
            spacing,
            intensity,
            levels,
        }
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn spacing(&self) -> Spacing {
        self.spacing
    }

    pub fn intensity(&self) -> Intensity {
        self.intensity
    }

    pub fn format(&self) -> VoxelFormat {
        self.levels[0].data().format()
    }

    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    pub fn level_count(&self) -> u32 {
        self.levels.len() as u32
    }

    pub fn world_extent(&self) -> [f32; 3] {
        self.spacing.world_extent(self.extent)
    }
}
