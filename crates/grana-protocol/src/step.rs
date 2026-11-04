use grana_scene::{Criteria, ObjectSetId};

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Detect {
        set: ObjectSetId,
        mask: Option<ObjectSetId>,
    },
    Filter {
        set: ObjectSetId,
        source: ObjectSetId,
        criteria: Criteria,
    },
    Copy {
        set: ObjectSetId,
        source: ObjectSetId,
    },
    Merge {
        set: ObjectSetId,
        part: ObjectSetId,
    },
    Relation {
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    },
    Coloc {
        pair: usize,
        mask: Option<ObjectSetId>,
    },
    Present,
}

impl Step {
    pub fn installs(&self) -> Option<ObjectSetId> {
        match self {
            Self::Detect { set, .. }
            | Self::Filter { set, .. }
            | Self::Copy { set, .. }
            | Self::Merge { set, .. } => Some(*set),
            Self::Relation { .. } | Self::Coloc { .. } | Self::Present => None,
        }
    }
}
