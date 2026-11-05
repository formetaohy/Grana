use grana_object::{Basin, SampleAccumulator, Seam, SurfacePatches, SurfaceTopology, TopologyInfo};

use crate::hash::Map;

use super::tile::TileOutput;
use super::{EMPTY, Face, Node, Pass, SurfaceBuilder, find};

pub(super) struct Assembler<'a> {
    pub(super) plan: &'a SurfaceBuilder,
    pub(super) nodes: Vec<Node>,
    pub(super) accumulators: Vec<SampleAccumulator>,
    pub(super) necks: Vec<(u32, u32, u32)>,
    pub(super) seed_count: usize,
    pub(super) vertices: Vec<grana_geometry::MeshVertex>,
    pub(super) keys: Vec<u64>,
    pub(super) indices: Vec<u32>,
    pub(super) triangle_objects: Vec<u32>,
    next: usize,
    x_face: Option<Face>,
    y_faces: Vec<Option<Face>>,
    z_faces: Vec<Option<Face>>,
}

impl<'a> Assembler<'a> {
    pub(super) fn new(pass: &Pass<'a>) -> Self {
        let plan = pass.plan;
        let tiles = plan.tiles;
        Self {
            plan,
            nodes: Vec::new(),
            accumulators: Vec::new(),
            necks: Vec::new(),
            seed_count: pass.seeds.len(),
            vertices: Vec::new(),
            keys: Vec::new(),
            indices: Vec::new(),
            triangle_objects: Vec::new(),
            next: 0,
            x_face: None,
            y_faces: (0..tiles[0]).map(|_| None).collect(),
            z_faces: (0..tiles[0] * tiles[1]).map(|_| None).collect(),
        }
    }

    pub(super) fn push(&mut self, index: usize, mut output: TileOutput) -> Result<(), String> {
        assert_eq!(
            index, self.next,
            "surface tiles are assembled in raster order"
        );
        self.next += 1;
        let plan = self.plan;
        let nodes = std::mem::take(&mut output.nodes);
        let local_count = nodes.len();
        let base = self.nodes.len() as u32;
        assert!(
            u64::from(base) + local_count as u64 <= u64::from(u32::MAX),
            "a surface frame holds fewer nodes than the empty sentinel"
        );
        for (local, node) in nodes.into_iter().enumerate() {
            self.nodes.push(Node {
                parent: base + local as u32,
                ..node
            });
        }
        self.accumulators
            .extend(std::mem::take(&mut output.accumulators));
        for (left, right, neck) in std::mem::take(&mut output.necks) {
            self.necks.push((left + base, right + base, neck));
        }
        for face in output.join.iter_mut().flatten() {
            face.rebase(base);
        }
        for face in output.cache.iter_mut().flatten() {
            face.rebase(base);
        }
        self.join(&output);
        for (seed, local) in std::mem::take(&mut output.seeds) {
            let root = find(&mut self.nodes, base + local);
            let node = &mut self.nodes[root as usize];
            if node.seed == seed {
                continue;
            }
            if node.seed != EMPTY {
                return Err(format!(
                    "the seed points {} and {} share one watershed basin and cannot cut it apart",
                    node.seed + 1,
                    seed + 1
                ));
            }
            node.seed = seed;
        }
        let position = output.position;
        if let Some(face) = output.cache[0].take() {
            self.x_face = Some(face);
        }
        if let Some(face) = output.cache[1].take() {
            self.y_faces[position[0] as usize] = Some(face);
        }
        if let Some(face) = output.cache[2].take() {
            let slot = position[1] as usize * plan.tiles[0] as usize + position[0] as usize;
            self.z_faces[slot] = Some(face);
        }
        let mut resolved = vec![EMPTY; local_count];
        for object in std::mem::take(&mut output.triangles) {
            let root = self.resolve(base, object, &mut resolved);
            self.triangle_objects.push(root);
        }
        for contact in std::mem::take(&mut output.contacts) {
            let object = self.resolve(base, contact.object, &mut resolved);
            let node = self.nodes[object as usize];
            if node.contact != EMPTY || node.voxels > 0 {
                continue;
            }
            let mut found = EMPTY;
            let mut best = 0u64;
            for corner in 0..contact.count as usize {
                let root = find(&mut self.nodes, base + contact.labels[corner]);
                if root == object {
                    continue;
                }
                let voxels = self.nodes[root as usize].voxels;
                if voxels > 0 && voxels > best {
                    found = root;
                    best = voxels;
                }
            }
            if found != EMPTY {
                self.nodes[object as usize].contact = found;
            }
        }
        let vertices = std::mem::take(&mut output.vertices);
        let offset = self.vertices.len() as u64;
        assert!(
            offset + vertices.len() as u64 <= u64::from(u32::MAX),
            "a surface frame holds fewer mesh vertices than u32"
        );
        let offset = offset as u32;
        self.vertices.extend(vertices);
        self.keys.extend(std::mem::take(&mut output.keys));
        self.indices.extend(
            std::mem::take(&mut output.indices)
                .into_iter()
                .map(|vertex| offset + vertex),
        );
        Ok(())
    }

    pub(super) fn topology(self) -> SurfaceTopology {
        let plan = self.plan;
        let mut nodes = self.nodes;
        let accumulators = self.accumulators;
        let vertices = self.vertices;
        let keys = self.keys;
        let indices = self.indices;
        let mut weld: Map<u64, u32> = Map::default();
        let mut positions = Vec::with_capacity(vertices.len());
        let mut normals: Vec<[f32; 3]> = Vec::with_capacity(vertices.len());
        let mut welded = Vec::with_capacity(vertices.len());
        for (index, vertex) in vertices.iter().enumerate() {
            let slot = match weld.get(&keys[index]) {
                Some(slot) => *slot,
                None => {
                    let slot = positions.len() as u32;
                    weld.insert(keys[index], slot);
                    positions.push(vertex.position());
                    normals.push([0.0; 3]);
                    slot
                }
            };
            let normal = &mut normals[slot as usize];
            for (axis, value) in vertex.normal().iter().enumerate() {
                normal[axis] += value;
            }
            welded.push(slot);
        }
        let mut triangles = Vec::with_capacity(indices.len() / 3);
        for triangle in indices.as_chunks::<3>().0 {
            triangles.push(triangle.map(|index| welded[index as usize]));
        }
        assert!(
            self.triangle_objects.iter().all(|object| *object != EMPTY),
            "every surface triangle names a watershed basin"
        );
        let mut basin_of_triangle = self.triangle_objects;
        for object in &mut basin_of_triangle {
            *object = find(&mut nodes, *object);
        }
        let mut seams = Vec::with_capacity(self.necks.len());
        for (left, right, saddle) in self.necks {
            let left = find(&mut nodes, left);
            let right = find(&mut nodes, right);
            if left != right {
                seams.push(Seam::new(left, right, saddle));
            }
        }
        for index in 0..nodes.len() {
            let contact = nodes[index].contact;
            if contact != EMPTY {
                nodes[index].contact = find(&mut nodes, contact);
            }
        }
        let basins = nodes
            .iter()
            .map(|node| {
                Basin::new(
                    node.reach,
                    node.voxels,
                    node.sum,
                    node.low,
                    node.high,
                    (node.contact != EMPTY).then_some(node.contact),
                    (node.seed != EMPTY).then_some(node.seed),
                )
            })
            .collect();
        let patches = SurfacePatches::new(positions, normals, triangles, basin_of_triangle);
        let info = TopologyInfo::new(
            plan.level_index,
            plan.channels,
            plan.detection,
            plan.spacing,
            plan.world,
        )
        .with_sampling(plan.voxel_volume, plan.span, self.seed_count);
        SurfaceTopology::new(info, basins, accumulators, seams, patches)
    }

    fn resolve(&mut self, base: u32, local: u32, resolved: &mut [u32]) -> u32 {
        let slot = local as usize;
        assert!(slot < resolved.len(), "a tile feature names a tile node");
        if resolved[slot] == EMPTY {
            resolved[slot] = find(&mut self.nodes, base + local);
        }
        resolved[slot]
    }

    fn join(&mut self, output: &TileOutput) {
        let position = output.position;
        let tiles = self.plan.tiles;
        if position[0] > 0 {
            let face = self.x_face.take().expect("the x neighbour caches its face");
            self.join_face(&face, output.join[0].as_ref().expect("the tile joins x"));
        }
        if position[1] > 0 {
            let face = self.y_faces[position[0] as usize]
                .take()
                .expect("the y neighbour caches its face");
            self.join_face(&face, output.join[1].as_ref().expect("the tile joins y"));
        }
        if position[2] > 0 {
            let slot = position[1] as usize * tiles[0] as usize + position[0] as usize;
            let face = self.z_faces[slot]
                .take()
                .expect("the z neighbour caches its face");
            self.join_face(&face, output.join[2].as_ref().expect("the tile joins z"));
        }
    }

    fn join_face(&mut self, cached: &Face, local: &Face) {
        let start = [
            cached.origin[0].max(local.origin[0]),
            cached.origin[1].max(local.origin[1]),
        ];
        let end = [
            (cached.origin[0] + cached.dims[0]).min(local.origin[0] + local.dims[0]),
            (cached.origin[1] + cached.dims[1]).min(local.origin[1] + local.dims[1]),
        ];
        if start[0] >= end[0] || start[1] >= end[1] {
            return;
        }
        let mut pairs = Vec::new();
        for second in start[1]..end[1] {
            for first in start[0]..end[0] {
                let cached_slot = (first - cached.origin[0]) as usize
                    + cached.dims[0] as usize * (second - cached.origin[1]) as usize;
                let cached_label = cached.labels[cached_slot];
                if cached_label == EMPTY {
                    continue;
                }
                let local_slot = (first - local.origin[0]) as usize
                    + local.dims[0] as usize * (second - local.origin[1]) as usize;
                let local_label = local.labels[local_slot];
                if local_label == EMPTY {
                    continue;
                }
                let neck = cached.reach[cached_slot].min(local.reach[local_slot]);
                pairs.push((local_label, cached_label, neck));
            }
        }
        if pairs.is_empty() {
            return;
        }
        pairs.sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        let mut index = 0;
        while index < pairs.len() {
            let (first, second, _) = pairs[index];
            let mut saddle = 0;
            while index < pairs.len() && pairs[index].0 == first && pairs[index].1 == second {
                saddle = saddle.max(pairs[index].2);
                index += 1;
            }
            self.necks.push((first, second, saddle));
        }
    }
}
