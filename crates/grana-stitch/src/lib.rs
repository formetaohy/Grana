mod mosaic;
mod run;

pub use mosaic::{Edge, Offset, Plane, Side, estimate, solve};
pub use run::{Request, Summary, stitch};

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
