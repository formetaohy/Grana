#[derive(Clone, Debug)]
pub struct GlyphRaster {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}
