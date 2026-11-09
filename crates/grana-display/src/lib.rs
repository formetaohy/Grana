mod camera;
mod color;
mod palette;
mod shading;
mod transfer;
mod viewport;

pub use camera::{
    Camera, CameraState, PROJECTION_THICKNESS, Projection, Ray, SectionCamera, SectionState,
    THICKNESS_LIMIT,
};
pub use color::{ColorState, Coloring, Extent, merged};
pub use palette::Palette;
pub use shading::{AZIMUTH_LIMIT, DEFAULT_AZIMUTH, DEFAULT_ELEVATION, ELEVATION_LIMIT, Shading};
pub use transfer::{Stop, TransferFunction, linear_to_srgb, srgb_to_linear};
pub use viewport::{SPLIT_MAX, SPLIT_MIN, Viewport};
