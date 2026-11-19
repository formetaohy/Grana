#[derive(Default)]
pub struct Image {
    pub name: Option<String>,
    pub dimension_order: Option<String>,
    pub size_x: Option<u32>,
    pub size_y: Option<u32>,
    pub size_z: Option<u32>,
    pub size_c: Option<u32>,
    pub size_t: Option<u32>,
    pub physical: [Option<f32>; 3],
    pub interval: Option<f32>,
    pub channels: Vec<Channel>,
    pub tiff_data: Vec<TiffData>,
    pub planes: Vec<Plane>,
}

pub struct Channel {
    pub name: Option<String>,
    pub tint: Option<[f32; 3]>,
}

#[derive(Default, Clone)]
pub struct TiffData {
    pub ifd: u32,
    pub z: u32,
    pub c: u32,
    pub t: u32,
    pub planes: u32,
    pub file: Option<String>,
}

#[derive(Default, Clone)]
pub struct Plane {
    pub z: u32,
    pub c: u32,
    pub t: u32,
    pub position: [Option<f32>; 3],
}
