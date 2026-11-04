#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectAnchor {
    position: [f32; 3],
    scale: f32,
}

impl ObjectAnchor {
    pub fn new(position: [f32; 3], scale: f32) -> Self {
        assert!(
            position.iter().all(|value| value.is_finite()) && scale.is_finite() && scale > 0.0,
            "an object anchor is a finite position and a positive size"
        );
        Self { position, scale }
    }

    pub fn position(self) -> [f32; 3] {
        self.position
    }

    pub fn scale(self) -> f32 {
        self.scale
    }

    fn distance(self, other: Self) -> f32 {
        (0..3)
            .map(|axis| (self.position[axis] - other.position[axis]).powi(2))
            .sum::<f32>()
            .sqrt()
    }
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
    edits: Vec<TrackEdit>,
    pruned: bool,
    produced: Vec<bool>,
    dropped: usize,
}

impl PartialEq for Curation {
    fn eq(&self, other: &Self) -> bool {
        self.expected == other.expected
            && self.removed == other.removed
            && self.edits == other.edits
    }
}

impl Curation {
    pub fn restored(
        expected: &[Option<usize>],
        removed: &[Vec<ObjectAnchor>],
        edits: &[TrackEdit],
    ) -> Result<Self, String> {
        if removed.len() != expected.len() {
            return Err("the manual removals cover another set of frames".to_string());
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
            edits: edits.to_vec(),
            pruned: true,
            produced: vec![false; expected.len()],
            dropped: 0,
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

    pub fn edits(&self) -> &[TrackEdit] {
        &self.edits
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn is_empty(&self) -> bool {
        self.removed.iter().all(Vec::is_empty) && self.edits.is_empty()
    }

    pub fn removal_count(&self) -> usize {
        self.removed.iter().map(Vec::len).sum()
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
        self.produced = vec![false; frames];
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

    pub(crate) fn keep(
        &mut self,
        time: u32,
        anchors: &[ObjectAnchor],
        voxel: f32,
    ) -> Option<Vec<bool>> {
        let removed = self.removed.get_mut(time as usize)?;
        if removed.is_empty() {
            return None;
        }
        let tolerance = 3.0 * voxel;
        let mut keep = vec![true; anchors.len()];
        let mut taken = vec![false; anchors.len()];
        let mut retained = Vec::with_capacity(removed.len());
        for record in removed.drain(..) {
            let mut nearest: Option<(f32, usize)> = None;
            for (object, anchor) in anchors.iter().enumerate() {
                if taken[object] {
                    continue;
                }
                let distance = record.distance(*anchor);
                let limit = tolerance.min(0.5 * record.scale().min(anchor.scale()));
                if distance <= limit && nearest.is_none_or(|(best, _)| distance < best) {
                    nearest = Some((distance, object));
                }
            }
            match nearest {
                Some((_, object)) => {
                    taken[object] = true;
                    keep[object] = false;
                    retained.push(record);
                }
                None => self.dropped += 1,
            }
        }
        *removed = retained;
        Some(keep)
    }

    pub(crate) fn forget_edits(&mut self) -> usize {
        let dropped = self.edit_count();
        self.edits.clear();
        self.dropped += dropped;
        dropped
    }

    pub(crate) fn combined(&self, other: &Self) -> Self {
        let frames = self.removed.len().max(other.removed.len());
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
        }
    }
}

pub(crate) fn match_anchor(
    anchors: &[ObjectAnchor],
    position: [f32; 3],
    voxel: f32,
) -> Option<usize> {
    let tolerance = 3.0 * voxel;
    let mut nearest: Option<(f32, usize)> = None;
    for (object, anchor) in anchors.iter().enumerate() {
        let distance = (0..3)
            .map(|axis| (position[axis] - anchor.position[axis]).powi(2))
            .sum::<f32>()
            .sqrt();
        let limit = tolerance.min(0.5 * anchor.scale());
        if distance <= limit && nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, object));
        }
    }
    nearest.map(|(_, object)| object)
}
