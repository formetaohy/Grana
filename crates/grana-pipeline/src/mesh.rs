use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use grana_mesh::{Format, Material};
use grana_scene::{ObjectSet, ObjectSetId, Scene};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Export {
    Empty,
    Written,
}

pub fn write(
    scene: &Scene,
    set: ObjectSetId,
    time: u32,
    format: Format,
    target: &Path,
) -> Result<Export, String> {
    let object = scene
        .objects
        .get(set)
        .ok_or_else(|| "the mesh export names no object set".to_string())?;
    let name = object.name().to_string();
    let frame = object
        .frame_shared(time)
        .ok_or_else(|| format!("{name} holds no frame {time}"))?;
    let geometry = match frame.geometry().map_err(|error| error.to_string())? {
        Some(geometry) if geometry.object_count() > 0 => geometry,
        _ => return Ok(Export::Empty),
    };
    let key = object.key().as_str().to_string();
    match format {
        Format::Obj => atomic(target, |file| {
            grana_mesh::write_obj(BufWriter::new(file), &geometry, &key)
        })?,
        Format::Glb => {
            let materials = materials(scene, object, geometry.object_count());
            atomic(target, |file| {
                grana_mesh::write_glb(BufWriter::new(file), &geometry, &key, &materials)
            })?
        }
        Format::Stl => {
            let atomic =
                grana_file::Atomic::directory(target).map_err(|error| failed(target, error))?;
            for index in 0..geometry.object_count() {
                let path = atomic.staging().join(format!("{index:06}.stl"));
                let file = File::create(&path).map_err(|error| failed(&path, error))?;
                grana_mesh::write_stl(BufWriter::new(file), &geometry, index)
                    .map_err(|error| failed(&path, mesh_error(error)))?;
            }
            atomic.commit().map_err(|error| failed(target, error))?;
        }
    }
    Ok(Export::Written)
}

pub fn materials(scene: &Scene, object: &ObjectSet, objects: usize) -> Vec<Material> {
    let colors = scene.object_colors(object);
    (0..objects)
        .map(|index| {
            let color = colors
                .as_ref()
                .and_then(|colors| colors.get(index).copied())
                .unwrap_or_else(|| object.color());
            Material::new(
                grana_display::srgb_to_linear(color),
                object.opacity(),
                object.material(),
            )
        })
        .collect()
}

fn atomic(
    target: &Path,
    write: impl FnOnce(File) -> Result<(), grana_mesh::Error>,
) -> Result<(), String> {
    let atomic = grana_file::Atomic::file(target).map_err(|error| failed(target, error))?;
    let file = File::create(atomic.staging()).map_err(|error| failed(target, error))?;
    write(file).map_err(|error| failed(target, mesh_error(error)))?;
    atomic.commit().map_err(|error| failed(target, error))
}

fn mesh_error(error: grana_mesh::Error) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

fn failed(path: &Path, error: std::io::Error) -> String {
    format!("failed to write {}: {error}", path.display())
}
