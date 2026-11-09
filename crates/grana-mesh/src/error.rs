use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to write the mesh stream: {0}")]
    Write(#[from] io::Error),
    #[error("the packed mesh of `{object}` carries {indices} indices that do not form triangles")]
    Indices { object: usize, indices: usize },
    #[error("the export names the object {object} of a set holding {objects}")]
    Object { object: usize, objects: usize },
    #[error("the export names {materials} materials for {objects} objects")]
    Materials { materials: usize, objects: usize },
}
