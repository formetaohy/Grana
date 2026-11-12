use std::sync::Arc;

use grana_geometry::PackedMesh;

use crate::intensity::IntensityTable;
use crate::surface::{SurfaceSet, SurfaceStatistics};

#[derive(Clone, Debug)]
pub struct CellSet {
    bodies: SurfaceSet,
    nuclei: Vec<SurfaceStatistics>,
}

impl PartialEq for CellSet {
    fn eq(&self, other: &Self) -> bool {
        self.bodies == other.bodies && self.nuclei == other.nuclei
    }
}

impl CellSet {
    pub fn new(bodies: SurfaceSet, nuclei: Vec<SurfaceStatistics>) -> Self {
        assert_eq!(
            nuclei.len(),
            bodies.object_count(),
            "every cell body holds exactly one nucleus"
        );
        Self { bodies, nuclei }
    }

    pub fn level(&self) -> usize {
        self.bodies.level()
    }

    pub fn bodies(&self) -> &SurfaceSet {
        &self.bodies
    }

    pub fn nuclei(&self) -> &[SurfaceStatistics] {
        &self.nuclei
    }

    pub fn object_count(&self) -> usize {
        self.bodies.object_count()
    }

    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    pub fn body(&self, index: usize) -> SurfaceStatistics {
        self.bodies.objects()[index]
    }

    pub fn nucleus(&self, index: usize) -> SurfaceStatistics {
        self.nuclei[index]
    }

    pub fn geometry(&self) -> Result<Arc<PackedMesh>, grana_geometry::Error> {
        self.bodies.geometry()
    }

    pub fn detail(&self, level: usize) -> Result<(Arc<PackedMesh>, usize), grana_geometry::Error> {
        self.bodies.detail(level)
    }

    pub fn select_detail(&self, target: usize) -> usize {
        self.bodies.select_detail(target)
    }

    pub fn intensities(&self) -> &IntensityTable {
        self.bodies.intensities()
    }

    pub fn bytes(&self) -> u64 {
        self.bodies.bytes()
    }

    pub fn without(&self, object: usize) -> Result<Option<Self>, grana_geometry::Error> {
        let Some(bodies) = self.bodies.without(object)? else {
            return Ok(None);
        };
        let mut nuclei = self.nuclei.clone();
        nuclei.remove(object);
        Ok(Some(Self::new(bodies, nuclei)))
    }

    pub fn retain(&self, keep: &[bool]) -> Result<Self, grana_geometry::Error> {
        assert_eq!(
            keep.len(),
            self.object_count(),
            "the kept cells cover every cell of the frame"
        );
        let bodies = self.bodies.retain(keep)?;
        let nuclei = self
            .nuclei
            .iter()
            .zip(keep)
            .filter(|(_, kept)| **kept)
            .map(|(nucleus, _)| *nucleus)
            .collect::<Vec<_>>();
        Ok(Self::new(bodies, nuclei))
    }

    pub fn merge(&self, other: &Self) -> Result<Self, grana_geometry::Error> {
        assert_eq!(
            self.level(),
            other.level(),
            "merged cell sets share one resolution level"
        );
        let bodies = self.bodies.merge(&other.bodies)?;
        let mut nuclei = self.nuclei.clone();
        nuclei.extend_from_slice(&other.nuclei);
        Ok(Self::new(bodies, nuclei))
    }
}
