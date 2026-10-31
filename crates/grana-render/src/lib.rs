mod atlas;
mod brick;
mod camera;
mod census;
mod frame;
mod layout;
mod marker;
mod mesh;
mod present;
mod renderer;
mod schedule;
mod schema;
mod section;
mod table;
mod target;
mod transfer;
mod volume;

pub const CHANNEL_LIMIT: usize = 16;

pub use camera::{Camera, Ray, SectionCamera, plane_basis};
pub use marker::Marker;
pub use renderer::{BrickRequest, Frame, FrameInfo, Renderer, Requests, ViewInfo};
pub use schema::{ChannelRow, Layer, LayerKey, SceneFrame, Shading, Style};
pub use transfer::{Stop, TransferFunction};
