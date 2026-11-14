use glam::Vec3;

pub const AZIMUTH_LIMIT: f32 = 180.0;
pub const ELEVATION_LIMIT: f32 = 90.0;
pub const DEFAULT_AZIMUTH: f32 = -12.0;
pub const DEFAULT_ELEVATION: f32 = 48.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shading {
    pub ambient: f32,
    pub diffuse: f32,
    pub azimuth: f32,
    pub elevation: f32,
}

impl Shading {
    pub fn light(&self) -> Vec3 {
        assert!(
            self.azimuth.is_finite() && self.elevation.is_finite(),
            "a key light stands at finite angles"
        );
        assert!(
            self.elevation.abs() <= 90.0,
            "a key light stands between the poles"
        );
        let (sin_azimuth, cos_azimuth) = self.azimuth.to_radians().sin_cos();
        let (sin_elevation, cos_elevation) = self.elevation.to_radians().sin_cos();
        Vec3::new(
            cos_elevation * sin_azimuth,
            sin_elevation,
            cos_elevation * cos_azimuth,
        )
    }
}

impl Default for Shading {
    fn default() -> Self {
        Self {
            ambient: 0.35,
            diffuse: 0.75,
            azimuth: DEFAULT_AZIMUTH,
            elevation: DEFAULT_ELEVATION,
        }
    }
}
