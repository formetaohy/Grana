mod detail;
mod edit;
mod topology;

use std::sync::Arc;

use grana_geometry::{Bounds, Geometry, Mesh, PackedMesh, SurfaceShape};
use grana_store::SpillStore;

use crate::IntensityTable;

pub use edit::{SurfaceEdit, SurfaceEditKind};
pub use topology::{Basin, Seam, SurfacePatches, SurfaceTopology, TopologyInfo};

use detail::Details;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceStatistics {
    voxel_count: u64,
    volume: f64,
    area: f64,
    sphericity: f64,
    centroid: [f32; 3],
    bounds: Bounds,
    shape: Option<SurfaceShape>,
}

impl SurfaceStatistics {
    pub fn new(
        voxel_count: u64,
        volume: f64,
        area: f64,
        centroid: [f32; 3],
        bounds: Bounds,
        shape: Option<SurfaceShape>,
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
            shape,
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

    pub fn shape(self) -> Option<SurfaceShape> {
        self.shape
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
    triangles: usize,
    details: Arc<Details>,
}

impl SurfaceSet {
    pub fn new(
        level: usize,
        mesh: Mesh,
        objects: Vec<SurfaceStatistics>,
        intensities: IntensityTable,
    ) -> Self {
        let triangles = mesh.triangle_count();
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
            triangles,
            details: Arc::new(Details::new()),
        }
    }

    pub fn paged(
        level: usize,
        store: &Arc<SpillStore>,
        mesh: Mesh,
        objects: Vec<SurfaceStatistics>,
        intensities: IntensityTable,
    ) -> Result<Self, grana_geometry::Error> {
        let triangles = mesh.triangle_count();
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
            triangles,
            details: Arc::new(Details::new()),
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
        let triangles = packed.triangle_count();
        Ok(Some(Self {
            level,
            geometry,
            objects,
            intensities,
            topology: None,
            leaves: Vec::new(),
            removed: Vec::new(),
            triangles,
            details: Arc::new(Details::new()),
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

    pub fn triangles(&self) -> usize {
        self.triangles
    }

    pub fn select_detail(&self, target: usize) -> usize {
        self.details.select(self.triangles, target)
    }

    pub fn detail(&self, level: usize) -> Result<(Arc<PackedMesh>, usize), grana_geometry::Error> {
        let (mesh, available, start) = self.details.level(level, &self.geometry)?;
        if start {
            let details = Arc::clone(&self.details);
            let base = self.geometry.clone();
            grana_exec::Executor::lane(grana_exec::Role::Compute).submit(move |cancel| {
                if cancel.cancelled() {
                    details.cancel();
                    return;
                }
                detail::build(details, base);
            });
        }
        Ok((mesh, available))
    }

    pub fn bytes(&self) -> u64 {
        self.geometry.bytes()
            + self.details.bytes()
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
        let packed = self.geometry.load()?;
        let dropped = packed
            .groups()
            .get(object)
            .map_or(0, |group| group.index_count() as usize / 3);
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
            triangles: self.triangles.saturating_sub(dropped),
            details: Arc::new(Details::new()),
        }))
    }

    pub fn retain(&self, keep: &[bool]) -> Result<Self, grana_geometry::Error> {
        assert_eq!(
            keep.len(),
            self.objects.len(),
            "the kept objects cover every surface"
        );
        let packed = self.geometry.load()?;
        let triangles = keep
            .iter()
            .enumerate()
            .filter(|(_, kept)| **kept)
            .filter_map(|(object, _)| packed.groups().get(object))
            .map(|group| group.index_count() as usize / 3)
            .sum();
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
            triangles,
            details: Arc::new(Details::new()),
        })
    }

    pub fn edited(
        &self,
        edits: &[(usize, Option<&Self>)],
    ) -> Result<Option<Self>, grana_geometry::Error> {
        if edits.is_empty() {
            return Ok(Some(self.clone()));
        }
        assert!(
            edits.iter().all(|(object, _)| *object < self.objects.len()),
            "an edited surface names an object of its set"
        );
        for (_, replacement) in edits {
            if let Some(replacement) = replacement {
                assert!(
                    replacement.object_count() == 1,
                    "an edited surface replaces one object with one surface"
                );
                assert_eq!(
                    replacement.level, self.level,
                    "an edited surface keeps the level of its object set"
                );
            }
        }
        let replacements = edits
            .iter()
            .map(|(object, replacement)| {
                Ok((
                    *object,
                    replacement
                        .map(|replacement| replacement.geometry.load())
                        .transpose()?,
                ))
            })
            .collect::<Result<Vec<_>, grana_geometry::Error>>()?;
        let base = self.geometry.load()?;
        let packed = replacements
            .iter()
            .map(|(object, mesh)| (*object, mesh.as_deref()))
            .collect::<Vec<_>>();
        let edited = base.edited(&packed);
        let triangles = edited.triangle_count();
        let geometry = self.geometry.replace(edited)?;
        let dropped = edits
            .iter()
            .filter(|(_, replacement)| replacement.is_none())
            .map(|(object, _)| *object)
            .collect::<std::collections::BTreeSet<_>>();
        let replaced = edits.iter().any(|(_, replacement)| replacement.is_some());
        let channels = self.intensities.channels();
        let mut objects = Vec::with_capacity(self.objects.len() - dropped.len());
        let mut values = Vec::with_capacity(objects.capacity() * channels);
        for object in 0..self.objects.len() {
            if dropped.contains(&object) {
                continue;
            }
            let held = edits
                .iter()
                .find(|(target, _)| *target == object)
                .and_then(|(_, replacement)| replacement.as_ref());
            match held {
                Some(replacement) => {
                    objects.push(replacement.objects[0]);
                    values.extend(
                        (0..channels).map(|channel| replacement.intensities.object(0, channel)),
                    );
                }
                None => {
                    objects.push(self.objects[object]);
                    values.extend(
                        (0..channels).map(|channel| self.intensities.object(object, channel)),
                    );
                }
            }
        }
        let intensities = IntensityTable::new(channels, values);
        let cuttable = !replaced && self.leaves.len() == self.objects.len();
        let mut leaves = Vec::new();
        let mut removed = self.removed.clone();
        if cuttable {
            for (object, basins) in self.leaves.iter().enumerate() {
                if dropped.contains(&object) {
                    removed.extend_from_slice(basins);
                } else {
                    leaves.push(basins.clone());
                }
            }
            removed.sort_unstable();
            removed.dedup();
        }
        let topology = if cuttable {
            self.topology.clone()
        } else {
            None
        };
        Ok(Some(Self {
            level: self.level,
            geometry,
            objects,
            intensities,
            topology,
            leaves,
            removed,
            triangles,
            details: Arc::new(Details::new()),
        }))
    }

    pub fn merge(&self, other: &Self) -> Result<Self, grana_geometry::Error> {
        assert_eq!(
            self.level, other.level,
            "a merged surface set holds one resolution level"
        );
        let geometry = self.geometry.merge(&other.geometry)?;
        let mut objects = self.objects.clone();
        objects.extend_from_slice(&other.objects);
        let intensities = self.intensities.merge(&other.intensities);
        let grown = match (self.topology.as_ref(), other.topology.as_ref()) {
            (Some(left), Some(right)) => {
                assert_eq!(
                    self.leaves.len(),
                    self.objects.len(),
                    "a cut surface set names the basins of every object"
                );
                assert_eq!(
                    other.leaves.len(),
                    other.objects.len(),
                    "a cut surface set names the basins of every object"
                );
                let carried = left.load()?;
                let absorbed = right.load()?;
                match carried.merge(&absorbed) {
                    Some(merged) => Some((left.replace(merged)?, carried.basins().len() as u32)),
                    None => None,
                }
            }
            _ => None,
        };
        let (topology, leaves, removed) = match grown {
            Some((topology, base)) => {
                let mut leaves = self.leaves.clone();
                leaves.extend(unions(&other.leaves, base));
                let mut removed = self.removed.clone();
                removed.extend(other.removed.iter().map(|basin| basin + base));
                removed.sort_unstable();
                removed.dedup();
                (Some(topology), leaves, removed)
            }
            None => (None, Vec::new(), Vec::new()),
        };
        Ok(Self {
            level: self.level,
            geometry,
            objects,
            intensities,
            topology,
            leaves,
            removed,
            triangles: self.triangles + other.triangles,
            details: Arc::new(Details::new()),
        })
    }
}

fn unions(leaves: &[Vec<u32>], base: u32) -> Vec<Vec<u32>> {
    leaves
        .iter()
        .map(|leaves| leaves.iter().map(|basin| basin + base).collect())
        .collect()
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
