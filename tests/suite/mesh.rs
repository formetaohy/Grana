use grana_geometry::{Mesh, MeshVertex, PackedMesh};
use grana_mesh::{Material, write_glb, write_obj, write_stl};

fn quad() -> PackedMesh {
    let vertices = vec![
        MeshVertex::new([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0),
        MeshVertex::new([2.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0),
        MeshVertex::new([2.0, 3.0, 0.0], [0.0, 0.0, 1.0], 0),
        MeshVertex::new([0.0, 3.0, 0.0], [0.0, 0.0, 1.0], 0),
    ];
    PackedMesh::new(Mesh::new(vertices, vec![0, 1, 2, 0, 2, 3]))
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("the word is whole"))
}

fn real(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().expect("the real is whole"))
}

#[test]
fn writes_obj_stl_and_glb_in_world_coordinates() {
    let mesh = quad();
    let mut obj = Vec::new();
    write_obj(&mut obj, &mesh, "cells").expect("the obj stream writes");
    let text = String::from_utf8(obj).expect("the obj stream is text");
    assert_eq!(
        text.lines().filter(|line| line.starts_with("v ")).count(),
        4,
        "the obj names every vertex"
    );
    assert_eq!(
        text.lines().filter(|line| line.starts_with("f ")).count(),
        2,
        "the obj names every triangle"
    );
    assert!(text.contains("v 2.000000 0.000000 0.000000"));

    let mut stl = Vec::new();
    write_stl(&mut stl, &mesh, 0).expect("the stl stream writes");
    assert_eq!(stl.len(), 84 + 2 * 50, "the stl holds two facets");
    assert_eq!(word(&stl, 80), 2, "the stl counts its facets");
    assert!((real(&stl, 92) - 1.0).abs() < 1.0e-6, "the facet faces up");
    assert_eq!(real(&stl, 96), 0.0, "the stl keeps world coordinates");
    assert_eq!(real(&stl, 108), 2.0, "the stl keeps world coordinates");

    let mut glb = Vec::new();
    let material = Material::new([0.8, 0.2, 0.4], 0.5);
    write_glb(&mut glb, &mesh, "cells", &[material]).expect("the glb stream writes");
    assert_eq!(&glb[0..4], b"glTF", "the glb names its magic");
    assert_eq!(word(&glb, 4), 2, "the glb states its version");
    assert_eq!(
        word(&glb, 8) as usize,
        glb.len(),
        "the glb states its length"
    );
    let json_length = word(&glb, 12) as usize;
    let json: serde_json::Value =
        serde_json::from_slice(&glb[20..20 + json_length]).expect("the glb carries json");
    assert_eq!(json["meshes"].as_array().expect("meshes").len(), 1);
    assert_eq!(json["nodes"][0]["name"], "cells_000000");
    assert_eq!(json["accessors"][0]["count"], 4);
    assert_eq!(json["accessors"][0]["min"][1], 0.0);
    assert_eq!(json["accessors"][0]["max"][1], 3.0);
    let base = json["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"]
        .as_array()
        .expect("a material color");
    assert!((base[0].as_f64().expect("a channel") - 0.8).abs() < 1.0e-6);
    assert!((base[3].as_f64().expect("a channel") - 0.5).abs() < 1.0e-6);
    assert_eq!(json["materials"][0]["alphaMode"], "BLEND");
    let binary = 20 + json_length + 8;
    assert_eq!(
        json["buffers"][0]["byteLength"].as_u64().expect("a length") as usize,
        glb.len() - binary,
        "the glb buffer fills its chunk"
    );
    assert!((real(&glb, binary + 12) - 2.0).abs() < 1.0e-6);
}
