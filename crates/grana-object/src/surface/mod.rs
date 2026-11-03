mod topology;

use std::sync::Arc;

use grana_geometry::{Bounds, Geometry, GeometryStore, Mesh, PackedMesh};

use crate::IntensityTable;

pub use topology::{Basin, Seam, SurfacePatches, SurfaceTopology, TopologyInfo};

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

#[derive(Clone, Debug)]
pub struct SurfaceSet {
    level: usize,
    geometry: Geometry<PackedMesh>,
    objects: Vec<SurfaceStatistics>,
    intensities: IntensityTable,
    topology: Option<Geometry<SurfaceTopology>>,
    leaves: Vec<Vec<u32>>,
    removed: Vec<u32>,
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
            topology: None,
            leaves: Vec::new(),
            removed: Vec::new(),
        }
    }

    pub fn paged(
        level: usize,
        store: &Arc<GeometryStore>,
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
            topology: None,
            leaves: Vec::new(),
            removed: Vec::new(),
        })
    }

    pub fn from_geometry(
        level: usize,
        geometry: Geometry<PackedMesh>,
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
            topology: None,
            leaves: Vec::new(),
            removed: Vec::new(),
        }))
    }

    pub fn with_cut(mut self, leaves: Vec<Vec<u32>>, removed: Vec<u32>) -> Self {
        assert_eq!(
            leaves.len(),
            self.objects.len(),
            "every cut surface names the watershed basins it holds"
        );
        assert!(
            removed.windows(2).all(|pair| pair[0] < pair[1]),
            "removed watershed basins are ordered and unique"
        );
        self.leaves = leaves;
        self.removed = removed;
        self
    }

    pub fn with_topology(
        mut self,
        topology: Geometry<SurfaceTopology>,
    ) -> Result<Self, grana_geometry::Error> {
        let loaded = topology.load()?;
        assert!(
            loaded.level() == self.level,
            "a surface topology describes the level of its set"
        );
        let basins = loaded.basins().len() as u32;
        assert!(
            self.leaves.iter().flatten().all(|basin| *basin < basins),
            "a cut surface names the watershed basins of its topology"
        );
        assert!(
            self.removed.iter().all(|basin| *basin < basins),
            "a removed surface names the watershed basins of its topology"
        );
        self.topology = Some(topology);
        Ok(self)
    }

    pub fn topology(&self) -> Option<&Geometry<SurfaceTopology>> {
        self.topology.as_ref()
    }

    pub fn leaves(&self) -> &[Vec<u32>] {
        &self.leaves
    }

    pub fn removed(&self) -> &[u32] {
        &self.removed
    }

    pub fn cuttable(&self) -> bool {
        self.topology.is_some()
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn geometry(&self) -> Result<Arc<PackedMesh>, grana_geometry::Error> {
        self.geometry.load()
    }

    pub fn bytes(&self) -> u64 {
        self.geometry.bytes()
            + self
                .topology
                .as_ref()
                .map_or(0, |topology| topology.bytes())
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
        let intensities = self.intensities.without(object);
        let mut removed = self.removed.clone();
        let mut leaves = self.leaves.clone();
        if let Some(mut pruned) = (object < leaves.len()).then(|| leaves.remove(object)) {
            removed.append(&mut pruned);
            removed.sort_unstable();
            removed.dedup();
        }
        Ok(Some(Self {
            level: self.level,
            geometry,
            objects,
            intensities,
            topology: self.topology.clone(),
            leaves,
            removed,
        }))
    }

    pub fn retain(&self, keep: &[bool]) -> Result<Self, grana_geometry::Error> {
        assert_eq!(
            keep.len(),
            self.objects.len(),
            "the kept objects cover every surface"
        );
        let geometry = self.geometry.retain(keep)?;
        let carries = self.leaves.len() == self.objects.len();
        let kept_count = keep.iter().filter(|kept| **kept).count();
        let mut objects = Vec::with_capacity(kept_count);
        let mut leaves = Vec::with_capacity(kept_count);
        let mut removed = self.removed.clone();
        for (object, kept) in keep.iter().enumerate() {
            if *kept {
                objects.push(self.objects[object]);
                if carries {
                    leaves.push(self.leaves[object].clone());
                }
            } else if carries {
                removed.extend_from_slice(&self.leaves[object]);
            }
        }
        if carries {
            removed.sort_unstable();
            removed.dedup();
        }
        Ok(Self {
            level: self.level,
            geometry,
            objects,
            intensities: self.intensities.retain(keep),
            topology: self.topology.clone(),
            leaves,
            removed,
        })
    }
}

impl PartialEq for SurfaceSet {
    fn eq(&self, other: &Self) -> bool {
        self.level == other.level
            && self.geometry == other.geometry
            && self.objects == other.objects
            && self.intensities == other.intensities
            && self.leaves == other.leaves
            && self.removed == other.removed
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
