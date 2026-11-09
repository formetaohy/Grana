use std::io::Write;

use grana_geometry::PackedMesh;
use serde_json::{Value, json};

use crate::check::{ensure_materials, ensure_triangles};
use crate::{Error, Material};

const MAGIC: u32 = 0x4654_6C67;
const VERSION: u32 = 2;
const JSON_CHUNK: u32 = 0x4E4F_534A;
const BIN_CHUNK: u32 = 0x004E_4942;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;

pub fn write_glb(
    mut sink: impl Write,
    mesh: &PackedMesh,
    key: &str,
    materials: &[Material],
) -> Result<(), Error> {
    ensure_materials(mesh.object_count(), materials.len())?;
    let mut binary = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut primitives = Vec::new();
    let mut nodes = Vec::new();
    for object in 0..mesh.object_count() {
        let group = &mesh.groups()[object];
        ensure_triangles(object, group.index_count())?;
        let vertices = group.vertex_count() as usize;
        let triangles = group.index_count() as usize / 3;
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        let position_offset = binary.len();
        for vertex in 0..vertices {
            let point = mesh.position(object, vertex);
            for axis in 0..3 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
            for value in point {
                binary.extend_from_slice(&value.to_le_bytes());
            }
        }
        let position_view = view(&mut views, position_offset, vertices * 12, ARRAY_BUFFER);
        let position_accessor = accessors.len();
        accessors.push(json!({
            "bufferView": position_view,
            "componentType": FLOAT,
            "count": vertices,
            "type": "VEC3",
            "min": low,
            "max": high,
        }));
        let normal_offset = binary.len();
        for vertex in 0..vertices {
            for value in mesh.normal(object, vertex) {
                binary.extend_from_slice(&value.to_le_bytes());
            }
        }
        let normal_view = view(&mut views, normal_offset, vertices * 12, ARRAY_BUFFER);
        let normal_accessor = accessors.len();
        accessors.push(json!({
            "bufferView": normal_view,
            "componentType": FLOAT,
            "count": vertices,
            "type": "VEC3",
        }));
        let index_offset = binary.len();
        for triangle in 0..triangles {
            for corner in mesh.triangle_vertices(object, triangle) {
                binary.extend_from_slice(&corner.to_le_bytes());
            }
        }
        let index_view = view(
            &mut views,
            index_offset,
            triangles * 12,
            ELEMENT_ARRAY_BUFFER,
        );
        let index_accessor = accessors.len();
        accessors.push(json!({
            "bufferView": index_view,
            "componentType": UNSIGNED_INT,
            "count": triangles * 3,
            "type": "SCALAR",
        }));
        let name = format!("{key}_{object:06}");
        primitives.push(json!({
            "name": name,
            "primitives": [{
                "attributes": {
                    "POSITION": position_accessor,
                    "NORMAL": normal_accessor,
                },
                "indices": index_accessor,
                "material": object,
            }],
        }));
        nodes.push(json!({ "name": name, "mesh": object }));
    }
    let materials = materials
        .iter()
        .enumerate()
        .map(|(index, material)| material_json(key, index, *material))
        .collect::<Vec<_>>();
    let document = json!({
        "asset": {
            "version": "2.0",
            "generator": "Grana",
            "extras": { "units": "micrometer", "upAxis": "Z" },
        },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": primitives,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{ "byteLength": binary.len() }],
    });
    write_chunks(&mut sink, &document, &binary)?;
    sink.flush()?;
    Ok(())
}

fn view(views: &mut Vec<Value>, offset: usize, length: usize, target: u32) -> usize {
    assert!(offset.is_multiple_of(4), "a glb view starts aligned");
    views.push(json!({
        "buffer": 0,
        "byteOffset": offset,
        "byteLength": length,
        "target": target,
    }));
    views.len() - 1
}

fn material_json(key: &str, index: usize, material: Material) -> Value {
    let [red, green, blue] = material.color();
    let opacity = material.opacity();
    json!({
        "name": format!("{key}_{index:06}"),
        "pbrMetallicRoughness": {
            "baseColorFactor": [red, green, blue, opacity],
            "metallicFactor": material.surface().metallic(),
            "roughnessFactor": material.surface().roughness(),
        },
        "alphaMode": if opacity < 1.0 { "BLEND" } else { "OPAQUE" },
        "doubleSided": true,
    })
}

fn write_chunks(sink: &mut impl Write, document: &Value, binary: &[u8]) -> Result<(), Error> {
    let mut json = document.to_string().into_bytes();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut data = binary.to_vec();
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
    let length = 12 + 8 + json.len() + 8 + data.len();
    sink.write_all(&MAGIC.to_le_bytes())?;
    sink.write_all(&VERSION.to_le_bytes())?;
    sink.write_all(&(length as u32).to_le_bytes())?;
    sink.write_all(&(json.len() as u32).to_le_bytes())?;
    sink.write_all(&JSON_CHUNK.to_le_bytes())?;
    sink.write_all(&json)?;
    sink.write_all(&(data.len() as u32).to_le_bytes())?;
    sink.write_all(&BIN_CHUNK.to_le_bytes())?;
    sink.write_all(&data)?;
    Ok(())
}
