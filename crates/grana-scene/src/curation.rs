use grana_object::{ObjectAnchor, assign};

use crate::revision::Revision;

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
    Divide {
        parent: TrackAnchor,
        first: TrackAnchor,
        second: TrackAnchor,
    },
    Reparent {
        child: TrackAnchor,
        parent: TrackAnchor,
    },
    Detach {
        child: TrackAnchor,
    },
}

impl TrackEdit {
    fn anchors(self) -> Vec<TrackAnchor> {
        match self {
            Self::Cut(node) => vec![node],
            Self::Join { source, target } => vec![source, target],
            Self::Divide {
                parent,
                first,
                second,
            } => vec![parent, first, second],
            Self::Reparent { child, parent } => vec![child, parent],
            Self::Detach { child } => vec![child],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Curation {
    removed: Vec<Vec<ObjectAnchor>>,
    hidden: Vec<Vec<ObjectAnchor>>,
    edits: Vec<TrackEdit>,
    applied: Vec<Revision>,
    dropped: usize,
    pending: usize,
}

pub(crate) struct Kept {
    pub(crate) keep: Vec<bool>,
    pub(crate) pending: usize,
}

impl PartialEq for Curation {
    fn eq(&self, other: &Self) -> bool {
        self.removed == other.removed && self.hidden == other.hidden && self.edits == other.edits
    }
}

impl Curation {
    pub fn restored(
        frames: usize,
        removed: &[Vec<ObjectAnchor>],
        hidden: &[Vec<ObjectAnchor>],
        edits: &[TrackEdit],
    ) -> Result<Self, String> {
        if removed.len() != frames {
            return Err("the manual removals cover another set of frames".to_string());
        }
        if !hidden.is_empty() && hidden.len() != frames {
            return Err("the hidden objects cover another set of frames".to_string());
        }
        for edit in edits {
            for node in edit.anchors() {
                if node.time as usize >= frames {
                    return Err(format!(
                        "a track edit names the frame {} of {frames}",
                        node.time
                    ));
                }
            }
            match edit {
                TrackEdit::Cut(_) | TrackEdit::Detach { .. } => {}
                TrackEdit::Join { source, target } => {
                    if target.time <= source.time {
                        return Err("a track join follows the time axis".to_string());
                    }
                }
                TrackEdit::Divide {
                    parent,
                    first,
                    second,
                } => {
                    if first.time <= parent.time || second.time <= parent.time {
                        return Err("a division follows the time axis".to_string());
                    }
                    if first.time == second.time && first.object == second.object {
                        return Err("a division names two daughter tracks".to_string());
                    }
                }
                TrackEdit::Reparent { child, parent } => {
                    if child.time <= parent.time {
                        return Err("a parent track ends before its daughter starts".to_string());
                    }
                }
            }
        }
        Ok(Self {
            removed: removed.to_vec(),
            hidden: hidden.to_vec(),
            edits: edits.to_vec(),
            applied: vec![Revision::ZERO; frames],
            dropped: 0,
            pending: 0,
        })
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

    pub fn frames(&self) -> usize {
        self.removed.len()
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

    pub(crate) fn applied(&self, time: u32) -> Revision {
        self.applied
            .get(time as usize)
            .copied()
            .unwrap_or(Revision::ZERO)
    }

    pub(crate) fn set_applied(&mut self, time: u32, stamp: Revision) {
        if self.applied.len() <= time as usize {
            self.applied.resize(time as usize + 1, Revision::ZERO);
        }
        self.applied[time as usize] = stamp;
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

    pub(crate) fn seed(&mut self, frames: usize) {
        if !self.removed.is_empty() {
            if self.hidden.len() != frames {
                self.hidden = vec![Vec::new(); frames];
            }
            return;
        }
        self.removed = vec![Vec::new(); frames];
        self.hidden = vec![Vec::new(); frames];
        self.applied = vec![Revision::ZERO; frames];
    }

    pub(crate) fn record_hidden(&mut self, time: u32, anchors: Vec<ObjectAnchor>) {
        let time = time as usize;
        assert!(
            time < self.removed.len(),
            "a hidden frame lies inside the seeded frames"
        );
        if self.hidden.len() != self.removed.len() {
            self.hidden = vec![Vec::new(); self.removed.len()];
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
            time < self.removed.len(),
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

    pub(crate) fn remove_edit(&mut self, index: usize) -> bool {
        if index >= self.edits.len() {
            return false;
        }
        self.edits.remove(index);
        self.dropped += 1;
        true
    }

    pub(crate) fn combined(&self, other: &Self) -> Self {
        let frames = self.removed.len().max(other.removed.len());
        Self {
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
            applied: vec![Revision::ZERO; frames],
            dropped: self.dropped + other.dropped,
            pending: 0,
        }
    }
}
