#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackAnchor {
    time: u32,
    object: usize,
}

impl TrackAnchor {
    pub fn new(time: u32, object: usize) -> Self {
        Self { time, object }
    }

    pub fn time(self) -> u32 {
        self.time
    }

    pub fn object(self) -> usize {
        self.object
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    removed: Vec<Vec<usize>>,
    edits: Vec<TrackEdit>,
    pruned: bool,
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
        removed: &[Vec<usize>],
        edits: &[TrackEdit],
    ) -> Result<Self, String> {
        if removed.len() != expected.len() {
            return Err("the manual removals cover another set of frames".to_string());
        }
        for (time, objects) in removed.iter().enumerate() {
            if objects.is_empty() {
                continue;
            }
            let Some(count) = expected[time] else {
                return Err(format!(
                    "the manual removals of frame {time} record no object count"
                ));
            };
            if !objects.windows(2).all(|pair| pair[0] < pair[1]) {
                return Err(format!(
                    "the manual removals of frame {time} are not ordered"
                ));
            }
            if objects.last().copied().unwrap_or(0) >= count {
                return Err(format!(
                    "the manual removals of frame {time} leave the object count {count}"
                ));
            }
        }
        for edit in edits {
            let [first, second] = edit.nodes();
            if let TrackEdit::Join { .. } = edit
                && (first == second || first.time != second.time)
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
            dropped: 0,
        })
    }

    pub fn applied(&self) -> bool {
        self.pruned
    }

    pub(crate) fn set_applied(&mut self) {
        self.pruned = true;
    }

    pub(crate) fn set_produced(&mut self) {
        self.pruned = false;
    }

    pub fn expected(&self) -> &[Option<usize>] {
        &self.expected
    }

    pub fn removed(&self) -> &[Vec<usize>] {
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
    }

    pub(crate) fn record(&mut self, time: u32, keep: &[bool]) -> Vec<usize> {
        let time = time as usize;
        assert!(
            time < self.expected.len(),
            "a curated frame lies inside the seeded frames"
        );
        assert_eq!(
            keep.len() + self.removed[time].len(),
            self.expected[time].unwrap_or_else(|| keep.len() + self.removed[time].len()),
            "the curated frame holds the recorded object count"
        );
        let existing = &mut self.removed[time];
        let mut cursor = 0usize;
        let mut produced = Vec::new();
        let mut deleted = Vec::new();
        for (current, kept) in keep.iter().enumerate() {
            while existing.binary_search(&cursor).is_ok() {
                cursor += 1;
            }
            if !kept {
                deleted.push(current);
                produced.push(cursor);
            }
            cursor += 1;
        }
        existing.extend(produced);
        existing.sort_unstable();
        existing.dedup();
        deleted
    }

    pub(crate) fn record_edit(&mut self, edit: TrackEdit) {
        if !self.edits.contains(&edit) {
            self.edits.push(edit);
        }
    }

    pub(crate) fn keep(&self, time: u32) -> Option<Vec<bool>> {
        let removed = self.removed.get(time as usize)?;
        if removed.is_empty() {
            return None;
        }
        let count = self.expected.get(time as usize).copied().flatten()?;
        let mut keep = vec![true; count];
        for object in removed {
            if *object >= count {
                return None;
            }
            keep[*object] = false;
        }
        Some(keep)
    }

    pub(crate) fn clears(&mut self) -> usize {
        let dropped = self.removal_count() + self.edit_count();
        self.expected.clear();
        self.removed.clear();
        self.edits.clear();
        self.dropped += dropped;
        dropped
    }

    pub(crate) fn forget_edits(&mut self) -> usize {
        let dropped = self.edit_count();
        self.edits.clear();
        self.dropped += dropped;
        dropped
    }
}
