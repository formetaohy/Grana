use grana_geometry::PackedMesh;

use crate::Error;

pub(crate) fn ensure_object(mesh: &PackedMesh, object: usize) -> Result<(), Error> {
    if object < mesh.object_count() {
        return Ok(());
    }
    Err(Error::Object {
        object,
        objects: mesh.object_count(),
    })
}

pub(crate) fn ensure_triangles(object: usize, indices: u32) -> Result<(), Error> {
    if indices.is_multiple_of(3) {
        return Ok(());
    }
    Err(Error::Indices {
        object,
        indices: indices as usize,
    })
}

pub(crate) fn ensure_materials(objects: usize, materials: usize) -> Result<(), Error> {
    if objects == materials {
        return Ok(());
    }
    Err(Error::Materials { materials, objects })
}
