use std::sync::Arc;

use crate::IntensityTable;
use grana_geometry::{Bounds, Geometry, Mesh, PackedMesh};
use grana_store::SpillStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilamentTopology {
    branches: u32,
    terminals: u32,
    junctions: u32,
    nodes: u32,
}

impl FilamentTopology {
    pub fn new(branches: u32, terminals: u32, junctions: u32, nodes: u32) -> Self {
        assert!(branches > 0, "a filament holds at least one branch");
        assert!(
            terminals <= 2 * branches && junctions < branches,
            "a filament counts its terminals and junctions"
        );
        assert!(nodes > 0, "a filament holds at least one node");
        Self {
            branches,
            terminals,
            junctions,
            nodes,
        }
    }

    pub fn branches(self) -> u32 {
        self.branches
    }

    pub fn terminals(self) -> u32 {
        self.terminals
    }

    pub fn junctions(self) -> u32 {
        self.junctions
    }

    pub fn nodes(self) -> u32 {
        self.nodes
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilamentStatistics {
    length: f64,
    diameter: f64,
    volume: f64,
    topology: FilamentTopology,
    centroid: [f32; 3],
    bounds: Bounds,
}

impl FilamentStatistics {
    pub fn new(
        length: f64,
        diameter: f64,
        volume: f64,
        topology: FilamentTopology,
        centroid: [f32; 3],
        bounds: Bounds,
    ) -> Self {
        assert!(
            length.is_finite() && length > 0.0,
            "a filament length is a positive length"
        );
        assert!(
            diameter.is_finite() && diameter > 0.0,
            "a filament diameter is a positive length"
        );
        assert!(
            volume.is_finite() && volume > 0.0,
            "a filament volume is a positive volume"
        );
        assert!(
            centroid.iter().all(|value| value.is_finite()),
            "a filament centroid must be finite"
        );
        Self {
            length,
            diameter,
            volume,
            topology,
            centroid,
            bounds,
        }
    }

    pub fn length(self) -> f64 {
        self.length
    }

    pub fn diameter(self) -> f64 {
        self.diameter
    }

    pub fn volume(self) -> f64 {
        self.volume
    }

    pub fn topology(self) -> FilamentTopology {
        self.topology
    }

    pub fn branches(self) -> u32 {
        self.topology.branches()
    }

    pub fn terminals(self) -> u32 {
        self.topology.terminals()
    }

    pub fn junctions(self) -> u32 {
        self.topology.junctions()
    }

    pub fn nodes(self) -> u32 {
        self.topology.nodes()
    }

    pub fn centroid(self) -> [f32; 3] {
        self.centroid
    }

    pub fn bounds(self) -> Bounds {
        self.bounds
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilamentSet {
    level: usize,
    geometry: Geometry<PackedMesh>,
    objects: Vec<FilamentStatistics>,
    intensities: IntensityTable,
}

impl FilamentSet {
    pub fn new(
        level: usize,
        mesh: Mesh,
        objects: Vec<FilamentStatistics>,
        intensities: IntensityTable,
    ) -> Self {
        let geometry = PackedMesh::new(mesh);
        assert!(
            agrees(&geometry, &objects, &intensities),
            "every packed filament names an object"
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
        store: &Arc<SpillStore>,
        mesh: Mesh,
        objects: Vec<FilamentStatistics>,
        intensities: IntensityTable,
    ) -> Result<Self, grana_geometry::Error> {
        let geometry = PackedMesh::new(mesh);
        assert!(
            agrees(&geometry, &objects, &intensities),
            "every packed filament names an object"
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
        geometry: Geometry<PackedMesh>,
        objects: Vec<FilamentStatistics>,
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

    pub fn objects(&self) -> &[FilamentStatistics] {
        &self.objects
    }

    pub fn intensities(&self) -> &IntensityTable {
        &self.intensities
    }

    pub fn object(&self, index: usize) -> Option<FilamentStatistics> {
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

    pub fn retain(&self, keep: &[bool]) -> Result<Self, grana_geometry::Error> {
        assert_eq!(
            keep.len(),
            self.objects.len(),
            "the kept objects cover every filament"
        );
        let geometry = self.geometry.retain(keep)?;
        let mut objects = Vec::with_capacity(keep.iter().filter(|kept| **kept).count());
        for (object, kept) in keep.iter().enumerate() {
            if *kept {
                objects.push(self.objects[object]);
            }
        }
        Ok(Self {
            level: self.level,
            geometry,
            objects,
            intensities: self.intensities.retain(keep),
        })
    }

    pub fn merge(&self, other: &Self) -> Result<Self, grana_geometry::Error> {
        assert_eq!(
            self.level, other.level,
            "a merged filament set holds one resolution level"
        );
        let geometry = self.geometry.merge(&other.geometry)?;
        let mut objects = self.objects.clone();
        objects.extend_from_slice(&other.objects);
        let intensities = self.intensities.merge(&other.intensities);
        let packed = geometry.load()?;
        assert!(
            agrees(&packed, &objects, &intensities),
            "a merged filament set names every object it holds"
        );
        Ok(Self {
            level: self.level,
            geometry,
            objects,
            intensities,
        })
    }
}

fn agrees(
    geometry: &PackedMesh,
    objects: &[FilamentStatistics],
    intensities: &IntensityTable,
) -> bool {
    geometry.object_count() == objects.len()
        && geometry.is_empty() == objects.is_empty()
        && intensities.objects() == objects.len()
}
