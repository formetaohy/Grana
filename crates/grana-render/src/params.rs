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

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelParams {
    pub visible: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub transfer: TransferFunction,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderParams {
    pub shading: Shading,
    pub step_scale: f32,
    pub channels: Vec<ChannelParams>,
}
