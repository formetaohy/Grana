use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to write the obj stream: {0}")]
    Write(#[from] io::Error),
    #[error("the packed mesh of `{object}` carries {indices} indices that do not form triangles")]
    Indices { object: usize, indices: usize },
}
