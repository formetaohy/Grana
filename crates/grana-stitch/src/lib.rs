mod mosaic;
mod run;
mod tile;

pub use mosaic::{Edge, Offset, Plane, Side, estimate, solve};
pub use run::{Request, Summary, stitch};
pub use tile::{Folder, default_grid, folder, unfold};

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
