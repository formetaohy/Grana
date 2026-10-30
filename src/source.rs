use std::path::PathBuf;
use std::sync::Arc;

use grana_volume::Movie;

#[derive(Clone)]
pub enum Source {
    Memory(Arc<Movie>),
    File(PathBuf),
}
