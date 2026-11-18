struct ObjectData {
    center_radius: vec4<f32>,
    minimum: vec4<f32>,
    extent: vec4<f32>,
    first_index: u32,
    index_count: u32,
    reserved: vec2<u32>,
}

@group(1) @binding(0) var<storage, read> objects: array<ObjectData>;
