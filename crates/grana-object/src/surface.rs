use crate::{Bounds, Mesh};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntensityStatistics {
    mean: f32,
    min: f32,
    max: f32,
    deviation: f32,
}

impl IntensityStatistics {
    pub fn new(mean: f32, min: f32, max: f32, deviation: f32) -> Self {
        assert!(
            [mean, min, max, deviation]
                .iter()
                .all(|value| value.is_finite()),
            "intensity statistics are finite"
        );
        assert!(
            min <= mean && mean <= max,
            "intensity statistics are ordered"
        );
        assert!(deviation >= 0.0, "an intensity deviation is non negative");
        Self {
            mean,
            min,
            max,
            deviation,
        }
    }

    pub fn mean(self) -> f32 {
        self.mean
    }

    pub fn min(self) -> f32 {
        self.min
    }

    pub fn max(self) -> f32 {
        self.max
    }

    pub fn deviation(self) -> f32 {
        self.deviation
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceStatistics {
    voxel_count: u64,
    volume: f64,
    area: f64,
    sphericity: f64,
    centroid: [f32; 3],
    bounds: Bounds,
    intensity: IntensityStatistics,
}

impl SurfaceStatistics {
    pub fn new(
        voxel_count: u64,
        volume: f64,
        area: f64,
        centroid: [f32; 3],
        bounds: Bounds,
        intensity: IntensityStatistics,
    ) -> Self {
        assert!(voxel_count > 0, "a surface encloses at least one voxel");
        assert!(
            volume.is_finite() && volume > 0.0,
            "a surface volume is a positive volume"
        );
        assert!(
            area.is_finite() && area > 0.0,
            "a surface area is a positive area"
        );
        assert!(
            centroid.iter().all(|value| value.is_finite()),
            "a surface centroid must be finite"
        );
        let sphericity =
            (std::f64::consts::PI.cbrt() * (6.0 * volume).powf(2.0 / 3.0) / area).clamp(0.0, 1.0);
        Self {
            voxel_count,
            volume,
            area,
            sphericity,
            centroid,
            bounds,
            intensity,
        }
    }

    pub fn voxel_count(self) -> u64 {
        self.voxel_count
    }

    pub fn volume(self) -> f64 {
        self.volume
    }

    pub fn area(self) -> f64 {
        self.area
    }

    pub fn sphericity(self) -> f64 {
        self.sphericity
    }

    pub fn centroid(self) -> [f32; 3] {
        self.centroid
    }

    pub fn bounds(self) -> Bounds {
        self.bounds
    }

    pub fn intensity(self) -> IntensityStatistics {
        self.intensity
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceSet {
    level: usize,
    mesh: Mesh,
    objects: Vec<SurfaceStatistics>,
}

impl SurfaceSet {
    pub fn new(level: usize, mesh: Mesh, objects: Vec<SurfaceStatistics>) -> Self {
        assert!(
            mesh.vertices()
                .iter()
                .all(|vertex| (vertex.object() as usize) < objects.len()),
            "every mesh vertex names an object"
        );
        assert_eq!(
            mesh.is_empty(),
            objects.is_empty(),
            "the mesh and the object list agree on emptiness"
        );
        Self {
            level,
            mesh,
            objects,
        }
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    pub fn objects(&self) -> &[SurfaceStatistics] {
        &self.objects
    }

    pub fn object(&self, index: usize) -> Option<SurfaceStatistics> {
        self.objects.get(index).copied()
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}
