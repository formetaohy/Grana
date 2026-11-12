mod mosaic;
mod run;
mod tile;

pub use mosaic::{Edge, Offset, Plane, Side, estimate, solve};
pub use run::{Request, Summary, series_count, stitch};
pub use tile::{channel_of, default_grid, tiles};

#[derive(Debug, thiserror::Error)]
#[error("{detail}")]
pub struct Error {
    detail: String,
}

impl Error {
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}
