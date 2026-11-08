use grana_geometry::Bounds;

const SNAP_VOXELS: f32 = 3.0;
const SNAP_SHARE: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectAnchor {
    position: [f32; 3],
    bounds: Bounds,
}

impl ObjectAnchor {
    pub fn new(position: [f32; 3], bounds: Bounds) -> Self {
        assert!(
            position.iter().all(|value| value.is_finite()),
            "an object anchor names a finite position"
        );
        Self { position, bounds }
    }

    pub fn position(self) -> [f32; 3] {
        self.position
    }

    pub fn bounds(self) -> Bounds {
        self.bounds
    }

    pub fn scale(self) -> f32 {
        let size = self.bounds.size();
        (size[0] + size[1] + size[2]) / 6.0
    }

    pub fn distance(self, point: [f32; 3]) -> f32 {
        (0..3)
            .map(|axis| (self.position[axis] - point[axis]).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    pub fn clearance(self, other: &Self) -> f32 {
        self.clearance_of(other.position)
            .min(other.clearance_of(self.position))
    }

    pub fn clearance_of(self, point: [f32; 3]) -> f32 {
        let low = self.bounds.min();
        let high = self.bounds.max();
        (0..3)
            .map(|axis| {
                let below = low[axis] - point[axis];
                let above = point[axis] - high[axis];
                below.max(above).max(0.0).powi(2)
            })
            .sum::<f32>()
            .sqrt()
    }
}

fn tolerance(record: f32, candidate: f32, voxel: f32) -> f32 {
    SNAP_VOXELS * voxel + SNAP_SHARE * record.min(candidate)
}

pub fn match_anchor(anchors: &[ObjectAnchor], position: [f32; 3], voxel: f32) -> Option<usize> {
    let mut best: Option<(f32, f32, usize)> = None;
    for (index, anchor) in anchors.iter().enumerate() {
        let limit = tolerance(anchor.scale(), anchor.scale(), voxel);
        let clearance = anchor.clearance_of(position);
        if clearance > limit {
            continue;
        }
        let distance = anchor.distance(position);
        if best.is_none_or(|(best_clearance, best_distance, _)| {
            (clearance, distance) < (best_clearance, best_distance)
        }) {
            best = Some((clearance, distance, index));
        }
    }
    best.map(|(_, _, index)| index)
}

pub fn assign(
    records: &[ObjectAnchor],
    anchors: &[ObjectAnchor],
    voxel: f32,
) -> Vec<Option<usize>> {
    let mut candidates = Vec::new();
    for (record_index, record) in records.iter().enumerate() {
        let scale = record.scale();
        for (anchor_index, anchor) in anchors.iter().enumerate() {
            let limit = tolerance(scale, anchor.scale(), voxel);
            let clearance = record.clearance(anchor);
            if clearance > limit {
                continue;
            }
            candidates.push((
                clearance / limit,
                record.distance(anchor.position),
                record_index,
                anchor_index,
            ));
        }
    }
    candidates.sort_unstable_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then(left.1.total_cmp(&right.1))
            .then(left.2.cmp(&right.2))
            .then(left.3.cmp(&right.3))
    });
    let mut assigned = vec![None; records.len()];
    let mut taken = vec![false; anchors.len()];
    for (_, _, record, anchor) in candidates {
        if assigned[record].is_some() || taken[anchor] {
            continue;
        }
        assigned[record] = Some(anchor);
        taken[anchor] = true;
    }
    assigned
}
