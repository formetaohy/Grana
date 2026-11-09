mod cache;
mod device;
mod dynamics;
mod error;
mod model;
mod slice;
mod tile;
mod weights;

pub use dynamics::{Flow, Prompt};
pub use error::Error;
pub use model::{Model, ModelInfo};
pub use slice::{Segmenter, Settings, Volume, assemble};

pub type Result<T> = std::result::Result<T, Error>;
