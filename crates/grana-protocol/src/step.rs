use grana_measure::ColocPair;
use grana_scene::{Criteria, MaskFilter, ObjectSetId};
use grana_warp::Warp;

use crate::MoleculesSpec;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Drift {
        channel: usize,
        reference: u32,
        warps: Vec<Warp>,
    },
    Register {
        reference: usize,
        warps: Vec<Warp>,
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
    Localize {
        set: ObjectSetId,
        molecules: MoleculesSpec,
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
            Self::Drift { .. } | Self::Register { .. } => None,
            Self::Detect { set, .. }
            | Self::Import { set }
            | Self::Filter { set, .. }
            | Self::Copy { set, .. }
            | Self::Edit { set, .. }
            | Self::Merge { set, .. } => Some(*set),
            Self::Relation { .. }
            | Self::Coloc { .. }
            | Self::Localize { .. }
            | Self::ObjectColoc { .. }
            | Self::Labels { .. }
            | Self::Present => None,
        }
    }
}
