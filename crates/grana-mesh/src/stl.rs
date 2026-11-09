use std::io::Write;

use grana_geometry::PackedMesh;

use crate::Error;
use crate::check::{ensure_object, ensure_triangles};

const HEADER: usize = 80;

pub fn write_stl(mut sink: impl Write, mesh: &PackedMesh, object: usize) -> Result<(), Error> {
    ensure_object(mesh, object)?;
    let group = &mesh.groups()[object];
    ensure_triangles(object, group.index_count())?;
    let triangles = group.index_count() / 3;
    let mut header = [0u8; HEADER];
    let label = format!("Grana STL object {object:06} of {}", mesh.object_count());
    let bytes = label.as_bytes();
    header[..bytes.len().min(HEADER)].copy_from_slice(&bytes[..bytes.len().min(HEADER)]);
    sink.write_all(&header)?;
    sink.write_all(&triangles.to_le_bytes())?;
    for triangle in 0..triangles as usize {
        let corners = mesh
            .triangle_vertices(object, triangle)
            .map(|corner| mesh.position(object, corner as usize));
        let normal = face_normal(corners);
        for value in normal.into_iter().chain(corners.into_iter().flatten()) {
            sink.write_all(&value.to_le_bytes())?;
        }
        sink.write_all(&0u16.to_le_bytes())?;
    }
    sink.flush()?;
    Ok(())
}

fn face_normal(corners: [[f32; 3]; 3]) -> [f32; 3] {
    let across = [
        corners[1][0] - corners[0][0],
        corners[1][1] - corners[0][1],
        corners[1][2] - corners[0][2],
    ];
    let down = [
        corners[2][0] - corners[0][0],
        corners[2][1] - corners[0][1],
        corners[2][2] - corners[0][2],
    ];
    let normal = [
        across[1] * down[2] - across[2] * down[1],
        across[2] * down[0] - across[0] * down[2],
        across[0] * down[1] - across[1] * down[0],
    ];
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if length > 0.0 {
        [normal[0] / length, normal[1] / length, normal[2] / length]
    } else {
        [0.0; 3]
    }
}
