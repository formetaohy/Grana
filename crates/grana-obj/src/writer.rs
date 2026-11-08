use std::io::Write;

use grana_geometry::PackedMesh;

use crate::Error;

pub fn write(mut sink: impl Write, mesh: &PackedMesh, key: &str) -> Result<(), Error> {
    writeln!(sink, "# grana obj")?;
    writeln!(sink, "# units: micrometer")?;
    writeln!(sink, "# set: {key}")?;
    writeln!(sink, "# objects: {}", mesh.object_count())?;
    for (object, group) in mesh.groups().iter().enumerate() {
        ensure_triangles(object, group.index_count())?;
        writeln!(sink, "o {key}_{object:06}")?;
        for vertex in 0..group.vertex_count() as usize {
            let [x, y, z] = mesh.position(object, vertex);
            writeln!(sink, "v {x:.6} {y:.6} {z:.6}")?;
        }
        for vertex in 0..group.vertex_count() as usize {
            let [x, y, z] = mesh.normal(object, vertex);
            writeln!(sink, "vn {x:.4} {y:.4} {z:.4}")?;
        }
        let base = group.first_vertex();
        for triangle in 0..group.index_count() as usize / 3 {
            let corners = mesh
                .triangle_vertices(object, triangle)
                .map(|corner| base + corner + 1);
            writeln!(
                sink,
                "f {}//{} {}//{} {}//{}",
                corners[0], corners[0], corners[1], corners[1], corners[2], corners[2]
            )?;
        }
    }
    sink.flush()?;
    Ok(())
}

fn ensure_triangles(object: usize, indices: u32) -> Result<(), Error> {
    if indices.is_multiple_of(3) {
        return Ok(());
    }
    Err(Error::Indices {
        object,
        indices: indices as usize,
    })
}
