use crate::object_set::ObjectSetId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Detect(ObjectSetId),
    Recut(ObjectSetId),
    Track(ObjectSetId),
    Relation {
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    },
    Coloc,
}

impl Job {
    pub fn streams(self, set: ObjectSetId) -> bool {
        match self {
            Self::Detect(id) | Self::Recut(id) | Self::Track(id) => id == set,
            Self::Relation { source, .. } => source == set,
            Self::Coloc => false,
        }
    }

    pub fn installs(self, set: ObjectSetId) -> bool {
        match self {
            Self::Detect(id) | Self::Recut(id) | Self::Track(id) => id == set,
            Self::Relation { .. } | Self::Coloc => false,
        }
    }

    pub fn involves(self, set: ObjectSetId) -> bool {
        match self {
            Self::Detect(id) | Self::Recut(id) | Self::Track(id) => id == set,
            Self::Relation { source, target, .. } => source == set || target == set,
            Self::Coloc => false,
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
