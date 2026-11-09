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
        .frame(time)
        .ok_or_else(|| format!("{name} holds no frame {time}"))?;
    let geometry = match frame.geometry().map_err(|error| error.to_string())? {
        Some(geometry) if geometry.object_count() > 0 => geometry,
        _ => {
            clear(format, target)?;
            return Ok(Export::Empty);
        }
    };
    let key = object.key().as_str().to_string();
    match format {
        Format::Obj => {
            let file = create(target)?;
            grana_mesh::write_obj(BufWriter::new(file), &geometry, &key)
                .map_err(|error| failed(target, error))?;
        }
        Format::Glb => {
            let file = create(target)?;
            let materials = materials(scene, object, geometry.object_count());
            grana_mesh::write_glb(BufWriter::new(file), &geometry, &key, &materials)
                .map_err(|error| failed(target, error))?;
        }
        Format::Stl => {
            clear(format, target)?;
            std::fs::create_dir_all(target)
                .map_err(|error| format!("failed to create {}: {error}", target.display()))?;
            for index in 0..geometry.object_count() {
                let path = target.join(format!("{index:06}.stl"));
                let file = create(&path)?;
                grana_mesh::write_stl(BufWriter::new(file), &geometry, index)
                    .map_err(|error| failed(&path, error))?;
            }
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

fn create(path: &Path) -> Result<File, String> {
    File::create(path).map_err(|error| format!("failed to create {}: {error}", path.display()))
}

fn clear(format: Format, target: &Path) -> Result<(), String> {
    if !target.exists() {
        return Ok(());
    }
    let removed = match format {
        Format::Stl if target.is_dir() => std::fs::remove_dir_all(target),
        Format::Stl => return Ok(()),
        _ if target.is_file() => std::fs::remove_file(target),
        _ => return Ok(()),
    };
    removed.map_err(|error| format!("failed to clear {}: {error}", target.display()))
}

fn failed(path: &Path, error: grana_mesh::Error) -> String {
    format!("failed to write {}: {error}", path.display())
}
