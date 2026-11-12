use crate::object_set::ObjectSetId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Detect(ObjectSetId),
    Import(ObjectSetId),
    Recut(ObjectSetId),
    Track(ObjectSetId),
    Relation {
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    },
    Coloc,
    ObjectColoc(ObjectSetId),
    Drift,
    Register,
    Calibrate,
}

impl Job {
    pub fn streams(self, set: ObjectSetId) -> bool {
        match self {
            Self::Detect(id)
            | Self::Import(id)
            | Self::Recut(id)
            | Self::Track(id)
            | Self::ObjectColoc(id) => id == set,
            Self::Relation { source, .. } => source == set,
            Self::Coloc | Self::Drift | Self::Register | Self::Calibrate => false,
        }
    }

    pub fn installs(self, set: ObjectSetId) -> bool {
        match self {
            Self::Detect(id) | Self::Import(id) | Self::Recut(id) | Self::Track(id) => id == set,
            Self::Relation { .. }
            | Self::Coloc
            | Self::ObjectColoc(_)
            | Self::Drift
            | Self::Register
            | Self::Calibrate => false,
        }
    }

    pub fn involves(self, set: ObjectSetId) -> bool {
        match self {
            Self::Detect(id)
            | Self::Import(id)
            | Self::Recut(id)
            | Self::Track(id)
            | Self::ObjectColoc(id) => id == set,
            Self::Relation { source, target, .. } => source == set || target == set,
            Self::Coloc | Self::Drift | Self::Register | Self::Calibrate => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub job: Job,
    pub time: u32,
    pub done: usize,
    pub total: usize,
}
