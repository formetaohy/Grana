mod dynamics;
mod error;
mod model;
mod slice;
mod tile;

pub use dynamics::Flow;
pub use error::Error;
pub use model::{Model, ModelInfo, Precision};
pub use slice::{Segmenter, Settings, Volume, assemble};

pub type Result<T> = std::result::Result<T, Error>;
