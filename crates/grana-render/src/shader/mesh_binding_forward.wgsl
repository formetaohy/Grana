struct ObjectData {
    center_radius: vec4<f32>,
    minimum: vec4<f32>,
    extent: vec4<f32>,
    narrow_first: u32,
    narrow_count: u32,
    wide_first: u32,
    wide_count: u32,
    vertex_base: u32,
}

@group(1) @binding(0) var<storage, read> objects: array<ObjectData>;
@group(5) @binding(0) var<storage, read> pick_ids: array<u32>;
