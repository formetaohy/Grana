#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    roughness: f32,
    metallic: f32,
}

impl Material {
    pub const DEFAULT: Self = Self {
        roughness: 0.45,
        metallic: 0.0,
    };

    pub fn new(roughness: f32, metallic: f32) -> Self {
        assert!(
            roughness.is_finite() && (0.0..=1.0).contains(&roughness),
            "a material roughness lies in the unit interval"
        );
        assert!(
            metallic.is_finite() && (0.0..=1.0).contains(&metallic),
            "a material metallic weight lies in the unit interval"
        );
        Self {
            roughness,
            metallic,
        }
    }

    pub fn roughness(self) -> f32 {
        self.roughness
    }

    pub fn metallic(self) -> f32 {
        self.metallic
    }

    pub fn with_roughness(self, roughness: f32) -> Self {
        Self::new(roughness, self.metallic)
    }

    pub fn with_metallic(self, metallic: f32) -> Self {
        Self::new(self.roughness, metallic)
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::DEFAULT
    }
}
