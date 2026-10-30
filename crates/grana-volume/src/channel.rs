use crate::{Intensity, VoxelFormat};

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelInfo {
    name: String,
    tint: [f32; 3],
    intensity: Intensity,
    window: Intensity,
    format: VoxelFormat,
}

impl ChannelInfo {
    pub fn new(
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
        window: Intensity,
        format: VoxelFormat,
    ) -> Self {
        assert!(
            tint.iter().all(|component| (0.0..=1.0).contains(component)),
            "a channel tint is an srgb triple"
        );
        assert!(
            window.span() > 0.0,
            "a display window needs a positive span"
        );
        Self {
            name: name.into(),
            tint,
            intensity,
            window,
            format,
        }
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

    pub fn format(&self) -> VoxelFormat {
        self.format
    }
}
