mod atlas;
mod backend;
mod cache;
mod camera;
mod level;
mod marker;
mod mesh;
mod params;
mod renderer;
mod residency;
mod screen;
mod section;
mod target;
mod transfer;
mod uniforms;

pub const MAX_CHANNEL: usize = 8;

pub use camera::{Camera, Ray, plane_basis};
pub use marker::Marker;
pub use mesh::{SurfaceFrame, SurfaceStyle};
pub use params::{ChannelParams, RenderParams, Shading};
pub use renderer::{BrickRequest, Frame, FrameInfo, Renderer, ViewInfo};
pub use section::SectionCamera;
pub use transfer::{Stop, TransferFunction};
