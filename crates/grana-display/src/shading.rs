#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shading {
    pub ambient: f32,
    pub diffuse: f32,
    pub specular: f32,
    pub shininess: f32,
    pub shadows: f32,
}

impl Default for Shading {
    fn default() -> Self {
        Self {
            ambient: 0.35,
            diffuse: 0.75,
            specular: 0.30,
            shininess: 28.0,
            shadows: 0.7,
        }
    }
}
