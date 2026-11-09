use std::sync::Arc;

use grana_geometry::GeometryStore;
use grana_object::{
    Basin, ObjectAnchor, SampleAccumulator, SurfaceEdit, SurfaceEditKind, SurfaceSet,
    SurfaceTopology,
};

use super::cut::{self, Piece};

const EMPTY: u32 = u32::MAX;

pub fn apply(
    set: &SurfaceSet,
    edit: &SurfaceEdit,
    voxel: f32,
    store: Option<&Arc<GeometryStore>>,
) -> Result<SurfaceSet, String> {
    let anchors = set
        .objects()
        .iter()
        .map(|statistics| ObjectAnchor::new(statistics.centroid(), statistics.bounds()))
        .collect::<Vec<_>>();
    let resolved = grana_object::assign(edit.anchors(), &anchors, voxel);
    let mut objects = Vec::with_capacity(resolved.len());
    for (index, anchor) in resolved.iter().enumerate() {
        let Some(object) = anchor else {
            return Err(format!(
                "the {} of frame {} finds {} of the {} recorded objects",
                edit.kind().verb(),
                edit.frame(),
                index,
                edit.anchors().len()
            ));
        };
        objects.push(*object);
    }
    objects.sort_unstable();
    let recorded = objects.len();
    objects.dedup();
    if objects.len() != recorded {
        return Err(format!(
            "the {} of frame {} matches two recorded objects to one surface",
            edit.kind().verb(),
            edit.frame()
        ));
    }
    match edit.kind() {
        SurfaceEditKind::Merge => merge(set, &objects, store),
        SurfaceEditKind::Split => split(set, objects[0], store),
    }
}

pub fn merge(
    set: &SurfaceSet,
    objects: &[usize],
    store: Option<&Arc<GeometryStore>>,
) -> Result<SurfaceSet, String> {
    let topology = loaded(set)?;
    let count = set.object_count();
    let leaves = set.leaves();
    if leaves.len() != count {
        return Err("the surface set names no watershed basins to edit".to_string());
    }
    let mut selected = objects.to_vec();
    selected.sort_unstable();
    selected.dedup();
    if selected.len() < 2 {
        return Err("merging needs at least two objects".to_string());
    }
    if let Some(object) = selected.iter().find(|object| **object >= count) {
        return Err(format!("the surface set holds no object {object}"));
    }
    let mapping = mapping(count, &selected);
    let mut targets = identity(&topology, leaves);
    for (object, basins) in leaves.iter().enumerate() {
        for basin in basins {
            targets[*basin as usize] = mapping[object];
        }
    }
    let mut merged = Vec::with_capacity(leaves.len() - selected.len() + 1);
    for (object, basins) in leaves.iter().enumerate() {
        let target = mapping[object] as usize;
        if merged.len() <= target {
            merged.resize_with(target + 1, Vec::new);
        }
        merged[target].extend_from_slice(basins);
    }
    rebuild(&topology, set, targets, merged, store)
}

pub fn split(
    set: &SurfaceSet,
    object: usize,
    store: Option<&Arc<GeometryStore>>,
) -> Result<SurfaceSet, String> {
    let topology = loaded(set)?;
    let count = set.object_count();
    let leaves = set.leaves();
    if object >= count || leaves.len() != count {
        return Err(format!("the surface set holds no object {object}"));
    }
    let basins = &leaves[object];
    if basins.is_empty() {
        return Err("the object holds no watershed basins to split".to_string());
    }
    let parts = partition(&topology, basins)?;
    let mut targets = identity(&topology, leaves);
    let mut split = Vec::with_capacity(leaves.len() + parts.len() - 1);
    for (index, part) in parts.iter().enumerate() {
        let target = if index == 0 {
            object
        } else {
            count + index - 1
        };
        for basin in part {
            targets[*basin as usize] = target as u32;
        }
        split.push(part.clone());
    }
    let mut edited = leaves.to_vec();
    edited[object] = split[0].clone();
    edited.extend(split.into_iter().skip(1));
    rebuild(&topology, set, targets, edited, store)
}

fn loaded(set: &SurfaceSet) -> Result<Arc<SurfaceTopology>, String> {
    let topology = set
        .topology()
        .ok_or_else(|| "the surface set carries no watershed topology to edit".to_string())?;
    let loaded = topology.load().map_err(|error| error.to_string())?;
    if loaded.info().seeds() > 0 {
        return Err("a seeded surface cannot be merged or split".to_string());
    }
    Ok(loaded)
}

fn identity(topology: &SurfaceTopology, leaves: &[Vec<u32>]) -> Vec<u32> {
    let mut targets = vec![EMPTY; topology.basins().len()];
    for (object, basins) in leaves.iter().enumerate() {
        for basin in basins {
            targets[*basin as usize] = object as u32;
        }
    }
    targets
}

fn mapping(count: usize, selected: &[usize]) -> Vec<u32> {
    let first = selected[0];
    let mut mapping = Vec::with_capacity(count);
    for object in 0..count {
        if object <= first {
            mapping.push(object as u32);
        } else if selected.contains(&object) {
            mapping.push(first as u32);
        } else {
            let below = selected.iter().filter(|merged| **merged < object).count();
            mapping.push((object - below + 1) as u32);
        }
    }
    mapping
}

fn partition(topology: &SurfaceTopology, basins: &[u32]) -> Result<Vec<Vec<u32>>, String> {
    let mut member = vec![false; topology.basins().len()];
    for basin in basins {
        member[*basin as usize] = true;
    }
    let mut seams = topology
        .seams()
        .iter()
        .filter(|seam| member[seam.left() as usize] && member[seam.right() as usize])
        .map(|seam| (seam.left(), seam.right(), seam.saddle()))
        .collect::<Vec<_>>();
    seams.sort_unstable_by(|left, right| {
        right
            .2
            .cmp(&left.2)
            .then(left.0.cmp(&right.0))
            .then(left.1.cmp(&right.1))
    });
    if seams.is_empty() {
        return Err("the object is one watershed basin and cannot be split".to_string());
    }
    let mut joined = Union::new(topology.basins().len());
    let mut components = basins.len();
    let mut weakest = None;
    for (left, right, _) in &seams {
        if joined.union(*left, *right) {
            components -= 1;
            weakest = Some((*left, *right));
        }
    }
    if components == 1 {
        let (left, right) = weakest.expect("a connected surface merged at least once");
        seams.retain(|(a, b, _)| !((*a == left && *b == right) || (*a == right && *b == left)));
        joined = Union::new(topology.basins().len());
        for (a, b, _) in &seams {
            joined.union(*a, *b);
        }
    }
    let mut groups: Vec<Vec<u32>> = Vec::new();
    let mut slots: Vec<Option<usize>> = vec![None; topology.basins().len()];
    let mut ordered = basins.to_vec();
    ordered.sort_unstable();
    for basin in ordered {
        let root = joined.find(basin) as usize;
        let slot = match slots[root] {
            Some(slot) => slot,
            None => {
                let slot = groups.len();
                groups.push(Vec::new());
                slots[root] = Some(slot);
                slot
            }
        };
        groups[slot].push(basin);
    }
    if groups.len() < 2 {
        return Err("the object is one watershed basin and cannot be split".to_string());
    }
    Ok(groups)
}

fn rebuild(
    topology: &SurfaceTopology,
    set: &SurfaceSet,
    targets: Vec<u32>,
    leaves: Vec<Vec<u32>>,
    store: Option<&Arc<GeometryStore>>,
) -> Result<SurfaceSet, String> {
    let objects = leaves.len();
    let channels = topology.info().channels();
    let (order, offsets) = cut::grouped(topology, &targets);
    let aggregates = aggregates(topology, &targets, objects, channels);
    let mut pieces = Vec::with_capacity(objects);
    for (object, aggregate) in aggregates.iter().enumerate() {
        let range = offsets[object] as usize..offsets[object + 1] as usize;
        let piece = Piece {
            node: aggregate.node,
            accumulators: &aggregate.accumulators,
            triangles: &order[range],
        };
        let Some(chunk) = cut::mesh_piece(topology, piece, false, None) else {
            return Err(format!(
                "the edited object {object} carries no surface triangles"
            ));
        };
        pieces.push(chunk);
    }
    let assembled = cut::assemble(
        topology.info().resolution(),
        channels,
        pieces,
        leaves,
        set.removed(),
        store,
    )?;
    match set.topology() {
        Some(topology) => assembled
            .with_topology(topology.clone())
            .map_err(|error| error.to_string()),
        None => Ok(assembled),
    }
}

struct Aggregate {
    node: Basin,
    accumulators: Vec<SampleAccumulator>,
}

fn aggregates(
    topology: &SurfaceTopology,
    targets: &[u32],
    objects: usize,
    channels: usize,
) -> Vec<Aggregate> {
    let mut nodes: Vec<Option<Basin>> = vec![None; objects];
    let mut accumulators = vec![vec![SampleAccumulator::new(); channels]; objects];
    for (basin, target) in targets.iter().enumerate() {
        if *target == EMPTY {
            continue;
        }
        let target = *target as usize;
        let node = topology.basins()[basin];
        match &mut nodes[target] {
            Some(held) => held.absorb(node),
            None => nodes[target] = Some(node),
        }
        for (channel, accumulator) in accumulators[target].iter_mut().enumerate() {
            accumulator.merge(&topology.accumulators()[basin * channels + channel]);
        }
    }
    nodes
        .into_iter()
        .zip(accumulators)
        .map(|(node, accumulators)| Aggregate {
            node: node.expect("every edited object holds a watershed basin"),
            accumulators,
        })
        .collect()
}

struct Union {
    parent: Vec<u32>,
}

impl Union {
    fn new(count: usize) -> Self {
        Self {
            parent: (0..count as u32).collect(),
        }
    }

    fn find(&mut self, node: u32) -> u32 {
        let mut root = node;
        while self.parent[root as usize] != root {
            root = self.parent[root as usize];
        }
        let mut current = node;
        while self.parent[current as usize] != root {
            let next = self.parent[current as usize];
            self.parent[current as usize] = root;
            current = next;
        }
        root
    }

    fn union(&mut self, left: u32, right: u32) -> bool {
        let left = self.find(left);
        let right = self.find(right);
        if left == right {
            return false;
        }
        let (low, high) = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        self.parent[high as usize] = low;
        true
    }
}
