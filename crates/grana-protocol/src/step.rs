use grana_measure::ColocPair;
use grana_scene::{Criteria, MaskFilter, ObjectSetId};

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Drift {
        channel: usize,
        reference: u32,
        offsets: Vec<[f32; 3]>,
    },
    Detect {
        set: ObjectSetId,
        mask: Option<ObjectSetId>,
        mask_radius: f32,
        filter: Option<MaskFilter>,
    },
    Import {
        set: ObjectSetId,
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
    Edit {
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
        key: String,
    },
    Coloc {
        pair: usize,
        mask: Option<ObjectSetId>,
        mask_radius: f32,
        filter: Option<MaskFilter>,
    },
    ObjectColoc {
        set: ObjectSetId,
        pair: ColocPair,
    },
    Labels {
        set: ObjectSetId,
    },
    Present,
}

impl Step {
    pub fn installs(&self) -> Option<ObjectSetId> {
        match self {
            Self::Drift { .. } => None,
            Self::Detect { set, .. }
            | Self::Import { set }
            | Self::Filter { set, .. }
            | Self::Copy { set, .. }
            | Self::Edit { set, .. }
            | Self::Merge { set, .. } => Some(*set),
            Self::Relation { .. }
            | Self::Coloc { .. }
            | Self::ObjectColoc { .. }
            | Self::Labels { .. }
            | Self::Present => None,
        }
    }
}
