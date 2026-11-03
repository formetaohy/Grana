use std::sync::Arc;

use crate::IntensityTable;
use grana_geometry::{Bounds, Geometry, Mesh, MeshStore, PackedMesh};

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
    geometry: Geometry,
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
        assert!(
            agrees(&geometry, &objects, &intensities),
            "every packed surface names an object"
        );
        Self {
            level,
            geometry: Geometry::resident(geometry),
            objects,
            intensities,
        }
    }

    pub fn paged(
        level: usize,
        store: &Arc<MeshStore>,
        mesh: Mesh,
        objects: Vec<SurfaceStatistics>,
        intensities: IntensityTable,
    ) -> Result<Self, grana_geometry::Error> {
        let geometry = PackedMesh::new(mesh);
        assert!(
            agrees(&geometry, &objects, &intensities),
            "every packed surface names an object"
        );
        Ok(Self {
            level,
            geometry: Geometry::paged(store, geometry)?,
            objects,
            intensities,
        })
    }

    pub fn from_geometry(
        level: usize,
        geometry: Geometry,
        objects: Vec<SurfaceStatistics>,
        intensities: IntensityTable,
    ) -> Result<Option<Self>, grana_geometry::Error> {
        let packed = geometry.load()?;
        if !agrees(&packed, &objects, &intensities) {
            return Ok(None);
        }
        Ok(Some(Self {
            level,
            geometry,
            objects,
            intensities,
        }))
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn geometry(&self) -> Result<Arc<PackedMesh>, grana_geometry::Error> {
        self.geometry.load()
    }

    pub fn bytes(&self) -> u64 {
        self.geometry.bytes()
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

    pub fn without(&self, object: usize) -> Result<Option<Self>, grana_geometry::Error> {
        if object >= self.objects.len() {
            return Ok(None);
        }
        let Some(geometry) = self.geometry.without(object)? else {
            return Ok(None);
        };
        let mut objects = self.objects.clone();
        objects.remove(object);
        Self::from_geometry(
            self.level,
            geometry,
            objects,
            self.intensities.without(object),
        )
    }
}

fn agrees(
    geometry: &PackedMesh,
    objects: &[SurfaceStatistics],
    intensities: &IntensityTable,
) -> bool {
    geometry.object_count() == objects.len()
        && geometry.is_empty() == objects.is_empty()
        && intensities.objects() == objects.len()
}
