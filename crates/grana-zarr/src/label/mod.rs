mod read;
mod write;

pub use read::{Label, Labels, read as read_labels};
pub use write::{ChunkFill, LabelSpec, Written, write};
