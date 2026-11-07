mod axes;
mod error;
mod image;
mod label;
mod metadata;
mod write;

pub use error::Error;
pub use image::Image;
pub use label::{ChunkFill, Label, LabelSpec, Labels, Written, read_labels, write as write_labels};
pub use write::{ChunkFill as ImageFill, ImageSpec, ImageWritten, WriteChannel, write_image};
