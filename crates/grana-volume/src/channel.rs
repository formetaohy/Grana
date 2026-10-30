use crate::{Extent, Intensity, Level, VoxelData, VoxelFormat};

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    name: String,
    tint: [f32; 3],
    intensity: Intensity,
    window: Intensity,
    levels: Vec<Level>,
}

impl Channel {
    pub fn new(name: impl Into<String>, tint: [f32; 3], extent: Extent, data: VoxelData) -> Self {
        assert!(
            tint.iter().all(|component| (0.0..=1.0).contains(component)),
            "a channel tint is an srgb triple"
        );
        let intensity = data.range();
        let mut levels = vec![Level::new(extent, data)];
        while levels
            .last()
            .expect("a channel always holds level zero")
            .extent()
            .max_dimension()
            > 1
        {
            let next = levels
                .last()
                .expect("a channel always holds level zero")
                .downsampled();
            levels.push(next);
        }
        Self {
            name: name.into(),
            tint,
            intensity,
            window: intensity,
            levels,
        }
    }

    pub fn with_window(mut self, window: Intensity) -> Self {
        assert!(
            window.span() > 0.0,
            "a display window needs a positive span"
        );
        self.window = window;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }

    pub fn intensity(&self) -> Intensity {
        self.intensity
    }

    pub fn window(&self) -> Intensity {
        self.window
    }

    pub fn extent(&self) -> Extent {
        self.levels[0].extent()
    }

    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    pub fn level_count(&self) -> u32 {
        self.levels.len() as u32
    }

    pub fn format(&self) -> VoxelFormat {
        self.levels[0].data().format()
    }
}
