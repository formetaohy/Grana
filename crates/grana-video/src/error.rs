use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the movie needs an even frame size, not {size:?}")]
    Size { size: [u32; 2] },
    #[error("the movie encoder failed: {0}")]
    Encode(String),
    #[error("failed to write {}: {detail}", path.display())]
    Write { path: PathBuf, detail: String },
    #[error("the movie encoder stopped before the movie was complete")]
    Stopped,
}
