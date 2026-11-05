mod material;
mod motion;
mod renderer;
mod scene;

pub use glam::Vec2;
pub use material::{Appearance, Color, Material, Shadow, Tone};
pub use motion::{Animated, Spring};
pub use renderer::{Frame, Renderer};
pub use scene::{
    Container, ContainerId, Glass, GlassId, IOS_SMOOTHING, Rect, Scene, Shape, smooth_corner,
};
