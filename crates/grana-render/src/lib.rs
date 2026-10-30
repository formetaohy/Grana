mod atlas;
mod camera;
mod level;
mod marker;
mod params;
mod renderer;
mod section;
mod transfer;

pub const MAX_CHANNEL: usize = 8;

pub use camera::{Camera, plane_basis};
pub use marker::Marker;
pub use params::{ChannelDisplay, ChannelParams, RenderParams, Shading, Surface};
pub use renderer::{BrickRequest, Frame, FrameInfo, Renderer, ViewInfo};
pub use section::SectionCamera;
pub use transfer::{Stop, TransferFunction};
