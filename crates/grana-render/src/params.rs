use crate::TransferFunction;
use grana_volume::Intensity;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shading {
    pub ambient: f32,
    pub diffuse: f32,
    pub specular: f32,
    pub shininess: f32,
}

impl Default for Shading {
    fn default() -> Self {
        Self {
            ambient: 0.35,
            diffuse: 0.75,
            specular: 0.30,
            shininess: 28.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    threshold: f32,
    color: [f32; 3],
}

impl Surface {
    pub fn new(threshold: f32, color: [f32; 3]) -> Self {
        assert!(
            threshold.is_finite(),
            "a surface threshold must be a finite intensity"
        );
        assert!(
            color.iter().all(|channel| (0.0..=1.0).contains(channel)),
            "a surface color is an srgb triple"
        );
        Self { threshold, color }
    }

    pub fn threshold(self) -> f32 {
        self.threshold
    }

    pub fn color(self) -> [f32; 3] {
        self.color
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        assert!(
            threshold.is_finite(),
            "a surface threshold must be a finite intensity"
        );
        self.threshold = threshold;
    }

    pub fn set_color(&mut self, color: [f32; 3]) {
        assert!(
            color.iter().all(|channel| (0.0..=1.0).contains(channel)),
            "a surface color is an srgb triple"
        );
        self.color = color;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChannelDisplay {
    Volume,
    Surface(Surface),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelParams {
    pub visible: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub transfer: TransferFunction,
    pub display: ChannelDisplay,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderParams {
    pub shading: Shading,
    pub step_scale: f32,
    pub channels: Vec<ChannelParams>,
}
