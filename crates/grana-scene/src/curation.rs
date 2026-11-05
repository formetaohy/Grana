use grana_geometry::Bounds;

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

    fn scale(self) -> f32 {
        let size = self.bounds.size();
        (size[0] + size[1] + size[2]) / 6.0
    }

    fn distance(self, point: [f32; 3]) -> f32 {
        (0..3)
            .map(|axis| (self.position[axis] - point[axis]).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    fn clearance(self, other: &Self) -> f32 {
        self.clearance_of(other.position)
            .min(other.clearance_of(self.position))
    }

    fn clearance_of(self, point: [f32; 3]) -> f32 {
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

const SNAP_VOXELS: f32 = 3.0;
const SNAP_SHARE: f32 = 0.5;

fn tolerance(record: f32, candidate: f32, voxel: f32) -> f32 {
    SNAP_VOXELS * voxel + SNAP_SHARE * record.min(candidate)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackAnchor {
    time: u32,
    object: usize,
    position: [f32; 3],
}

impl TrackAnchor {
    pub fn new(time: u32, object: usize, position: [f32; 3]) -> Self {
        assert!(
            position.iter().all(|value| value.is_finite()),
            "a track anchor names a finite position"
        );
        Self {
            time,
            object,
            position,
        }
    }

    pub fn time(self) -> u32 {
        self.time
    }

    pub fn object(self) -> usize {
        self.object
    }

    pub fn position(self) -> [f32; 3] {
        self.position
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackEdit {
    Cut(TrackAnchor),
    Join {
        source: TrackAnchor,
        target: TrackAnchor,
    },
}

impl TrackEdit {
    fn nodes(self) -> [TrackAnchor; 2] {
        match self {
            Self::Cut(node) => [node, node],
            Self::Join { source, target } => [source, target],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Curation {
    expected: Vec<Option<usize>>,
    removed: Vec<Vec<ObjectAnchor>>,
    hidden: Vec<Vec<ObjectAnchor>>,
    edits: Vec<TrackEdit>,
    pruned: bool,
    produced: Vec<bool>,
    dropped: usize,
    pending: usize,
}

pub(crate) struct Kept {
    pub(crate) keep: Vec<bool>,
    pub(crate) pending: usize,
}

impl PartialEq for Curation {
    fn eq(&self, other: &Self) -> bool {
        self.expected == other.expected
            && self.removed == other.removed
            && self.hidden == other.hidden
            && self.edits == other.edits
    }
}

impl Curation {
    pub fn restored(
        expected: &[Option<usize>],
        removed: &[Vec<ObjectAnchor>],
        hidden: &[Vec<ObjectAnchor>],
        edits: &[TrackEdit],
    ) -> Result<Self, String> {
        if removed.len() != expected.len() {
            return Err("the manual removals cover another set of frames".to_string());
        }
        if !hidden.is_empty() && hidden.len() != expected.len() {
            return Err("the hidden objects cover another set of frames".to_string());
        }
        for (time, objects) in removed.iter().enumerate() {
            if objects.is_empty() {
                continue;
            }
            if expected[time].is_none() {
                return Err(format!(
                    "the manual removals of frame {time} record no object count"
                ));
            }
        }
        for (time, objects) in hidden.iter().enumerate() {
            if objects.is_empty() {
                continue;
            }
            if expected.get(time).is_none_or(Option::is_none) {
                return Err(format!(
                    "the hidden objects of frame {time} record no object count"
                ));
            }
        }
        for edit in edits {
            let [first, second] = edit.nodes();
            if let TrackEdit::Join { .. } = edit
                && (first.time != second.time || first.object == second.object)
            {
                return Err("a track join names two objects of one frame".to_string());
            }
            for node in [first, second] {
                match expected.get(node.time as usize).copied().flatten() {
                    Some(count) if node.object >= count => {
                        return Err(format!(
                            "a track edit names object {} of a frame that holds {count} objects",
                            node.object
                        ));
                    }
                    Some(_) => {}
                    None => {
                        return Err(format!(
                            "a track edit names frame {} without an object count",
                            node.time
                        ));
                    }
                }
            }
        }
        Ok(Self {
            expected: expected.to_vec(),
            removed: removed.to_vec(),
            hidden: hidden.to_vec(),
            edits: edits.to_vec(),
            pruned: true,
            produced: vec![false; expected.len()],
            dropped: 0,
            pending: 0,
        })
    }

    pub fn applied(&self) -> bool {
        self.pruned
    }

    pub(crate) fn set_applied(&mut self) {
        self.pruned = true;
        self.produced.clear();
    }

    pub(crate) fn set_produced(&mut self, time: u32) {
        self.pruned = false;
        if self.produced.len() <= time as usize {
            self.produced.resize(time as usize + 1, false);
        }
        self.produced[time as usize] = true;
    }

    pub(crate) fn produced(&self, time: u32) -> bool {
        self.produced.get(time as usize).copied().unwrap_or(false)
    }

    pub fn expected(&self) -> &[Option<usize>] {
        &self.expected
    }

    pub fn removed(&self) -> &[Vec<ObjectAnchor>] {
        &self.removed
    }

    pub fn hidden(&self) -> &[Vec<ObjectAnchor>] {
        &self.hidden
    }

    pub fn edits(&self) -> &[TrackEdit] {
        &self.edits
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn pending(&self) -> usize {
        self.pending
    }

    pub(crate) fn set_pending(&mut self, pending: usize) {
        self.pending = pending;
    }

    pub fn is_empty(&self) -> bool {
        self.removed.iter().all(Vec::is_empty)
            && self.hidden.iter().all(Vec::is_empty)
            && self.edits.is_empty()
    }

    pub fn removal_count(&self) -> usize {
        self.removed.iter().map(Vec::len).sum()
    }

    pub fn hidden_count(&self) -> usize {
        self.hidden.iter().map(Vec::len).sum()
    }

    pub fn edit_count(&self) -> usize {
        self.edits.len()
    }

    pub(crate) fn seed(&mut self, frames: usize, counts: impl Fn(usize) -> Option<usize>) {
        if !self.expected.is_empty() {
            return;
        }
        self.expected = (0..frames).map(counts).collect();
        self.removed = vec![Vec::new(); frames];
        self.hidden = vec![Vec::new(); frames];
        self.produced = vec![false; frames];
    }

    pub(crate) fn record_hidden(&mut self, time: u32, anchors: Vec<ObjectAnchor>) {
        let time = time as usize;
        assert!(
            time < self.expected.len(),
            "a hidden frame lies inside the seeded frames"
        );
        if self.hidden.len() != self.expected.len() {
            self.hidden = vec![Vec::new(); self.expected.len()];
        }
        self.hidden[time] = anchors;
    }

    pub(crate) fn clear_hidden(&mut self) {
        for frame in &mut self.hidden {
            frame.clear();
        }
    }

    pub(crate) fn hidden_targets(
        &self,
        time: u32,
        anchors: &[ObjectAnchor],
        voxel: f32,
    ) -> Vec<usize> {
        let Some(records) = self.hidden.get(time as usize) else {
            return Vec::new();
        };
        assign(records, anchors, voxel)
            .into_iter()
            .flatten()
            .collect()
    }

    pub(crate) fn record(&mut self, time: u32, keep: &[bool], anchors: &[ObjectAnchor]) {
        let time = time as usize;
        assert!(
            time < self.expected.len(),
            "a curated frame lies inside the seeded frames"
        );
        assert_eq!(
            keep.len(),
            anchors.len(),
            "the curated frame anchors every object"
        );
        for (object, kept) in keep.iter().enumerate() {
            if !kept {
                self.removed[time].push(anchors[object]);
            }
        }
    }

    pub(crate) fn record_edit(&mut self, edit: TrackEdit) {
        if !self.edits.contains(&edit) {
            self.edits.push(edit);
        }
    }

    pub(crate) fn keep(&mut self, time: u32, anchors: &[ObjectAnchor], voxel: f32) -> Option<Kept> {
        let removed = self.removed.get(time as usize)?;
        if removed.is_empty() {
            return None;
        }
        let assigned = assign(removed, anchors, voxel);
        let mut keep = vec![true; anchors.len()];
        let mut pending = 0;
        for object in &assigned {
            match object {
                Some(object) => keep[*object] = false,
                None => pending += 1,
            }
        }
        Some(Kept { keep, pending })
    }

    pub(crate) fn forget_edits(&mut self) -> usize {
        let dropped = self.edit_count();
        self.edits.clear();
        self.dropped += dropped;
        dropped
    }

    pub(crate) fn combined(&self, other: &Self) -> Self {
        let frames = self
            .removed
            .len()
            .max(other.removed.len())
            .max(self.hidden.len())
            .max(other.hidden.len());
        Self {
            expected: (0..frames)
                .map(|time| {
                    let left = self.expected.get(time).copied().flatten();
                    let right = other.expected.get(time).copied().flatten();
                    match (left, right) {
                        (Some(left), Some(right)) => Some(left + right),
                        (left, right) => left.or(right),
                    }
                })
                .collect(),
            removed: (0..frames)
                .map(|time| {
                    let mut anchors = self.removed.get(time).cloned().unwrap_or_default();
                    anchors.extend(other.removed.get(time).cloned().unwrap_or_default());
                    anchors
                })
                .collect(),
            hidden: (0..frames)
                .map(|time| {
                    let mut anchors = self.hidden.get(time).cloned().unwrap_or_default();
                    anchors.extend(other.hidden.get(time).cloned().unwrap_or_default());
                    anchors
                })
                .collect(),
            edits: {
                let mut edits = self.edits.clone();
                for edit in &other.edits {
                    if !edits.contains(edit) {
                        edits.push(*edit);
                    }
                }
                edits
            },
            pruned: self.pruned && other.pruned,
            produced: vec![false; frames],
            dropped: self.dropped + other.dropped,
            pending: 0,
        }
    }
}

pub(crate) fn match_anchor(
    anchors: &[ObjectAnchor],
    position: [f32; 3],
    voxel: f32,
) -> Option<usize> {
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

fn assign(records: &[ObjectAnchor], anchors: &[ObjectAnchor], voxel: f32) -> Vec<Option<usize>> {
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
