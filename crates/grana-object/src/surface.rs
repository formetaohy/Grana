use crate::IntensityTable;
use grana_geometry::{Bounds, Mesh, PackedMesh};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceStatistics {
    voxel_count: u64,
    volume: f64,
    area: f64,
    sphericity: f64,
    centroid: [f32; 3],
    bounds: Bounds,
}

impl SurfaceStatistics {
    pub fn new(
        voxel_count: u64,
        volume: f64,
        area: f64,
        centroid: [f32; 3],
        bounds: Bounds,
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceSet {
    level: usize,
    geometry: PackedMesh,
    objects: Vec<SurfaceStatistics>,
    intensities: IntensityTable,
}

impl SurfaceSet {
    pub fn new(
        level: usize,
        mesh: Mesh,
        objects: Vec<SurfaceStatistics>,
        intensities: IntensityTable,
    ) -> Self {
        let geometry = PackedMesh::new(mesh);
        assert_eq!(
            geometry.object_count(),
            objects.len(),
            "every packed surface names an object"
        );
        assert_eq!(
            geometry.is_empty(),
            objects.is_empty(),
            "the mesh and the object list agree on emptiness"
        );
        assert_eq!(
            intensities.objects(),
            objects.len(),
            "every surface is measured in every channel"
        );
        Self {
            level,
            geometry,
            objects,
            intensities,
        }
    }

    pub fn from_parts(
        level: usize,
        geometry: PackedMesh,
        objects: Vec<SurfaceStatistics>,
        intensities: IntensityTable,
    ) -> Option<Self> {
        if geometry.object_count() != objects.len()
            || geometry.is_empty() != objects.is_empty()
            || intensities.objects() != objects.len()
        {
            return None;
        }
        Some(Self {
            level,
            geometry,
            objects,
            intensities,
        })
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn geometry(&self) -> &PackedMesh {
        &self.geometry
    }

    pub fn objects(&self) -> &[SurfaceStatistics] {
        &self.objects
    }

    pub fn intensities(&self) -> &IntensityTable {
        &self.intensities
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
