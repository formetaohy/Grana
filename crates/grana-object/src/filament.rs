use std::sync::Arc;

use crate::IntensityTable;
use grana_geometry::{Bounds, Mesh, PackedMesh};

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
    geometry: Arc<PackedMesh>,
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
        let geometry = Arc::new(PackedMesh::new(mesh));
        assert_eq!(
            geometry.object_count(),
            objects.len(),
            "every packed filament names an object"
        );
        assert_eq!(
            geometry.is_empty(),
            objects.is_empty(),
            "the mesh and the object list agree on emptiness"
        );
        assert_eq!(
            intensities.objects(),
            objects.len(),
            "every filament is measured in every channel"
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
        objects: Vec<FilamentStatistics>,
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
            geometry: Arc::new(geometry),
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

    pub fn shared(&self) -> Arc<PackedMesh> {
        Arc::clone(&self.geometry)
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

    pub fn without(&self, object: usize) -> Option<Self> {
        if object >= self.objects.len() {
            return None;
        }
        let mut objects = self.objects.clone();
        objects.remove(object);
        Self::from_parts(
            self.level,
            self.geometry.without(object),
            objects,
            self.intensities.without(object),
        )
    }
}
