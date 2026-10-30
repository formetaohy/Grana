use std::path::PathBuf;
use std::sync::Arc;

use grana_volume::Volume;

#[derive(Clone)]
pub enum Source {
    Memory(Arc<Volume>),
    File(PathBuf),
}
