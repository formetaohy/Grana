mod cache;
mod device;
mod dynamics;
mod error;
mod model;
mod normalize;
mod slice;
mod tile;
mod weights;

pub use error::Error;
pub use slice::{Component, Segmenter, Settings, Volume, assemble};

pub type Result<T> = std::result::Result<T, Error>;
