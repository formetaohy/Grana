mod atlas;
mod camera;
mod level;
mod params;
mod renderer;
mod transfer;

pub const MAX_CHANNEL: usize = 8;

pub use camera::Camera;
pub use params::{ChannelParams, RenderParams, Shading};
pub use renderer::{BrickRequest, Frame, FrameInfo, Renderer};
pub use transfer::{Stop, TransferFunction};
