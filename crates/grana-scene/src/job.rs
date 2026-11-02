use crate::object_set::ObjectSetId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Detect(ObjectSetId),
    Track(ObjectSetId),
    Relation {
        source: ObjectSetId,
        target: ObjectSetId,
    },
    Coloc,
}

impl Job {
    pub fn set(self) -> Option<ObjectSetId> {
        match self {
            Self::Detect(set) | Self::Track(set) => Some(set),
            Self::Relation { source, .. } => Some(source),
            Self::Coloc => None,
        }
    }

    pub fn involves(self, set: ObjectSetId) -> bool {
        match self {
            Self::Detect(id) | Self::Track(id) => id == set,
            Self::Relation { source, target } => source == set || target == set,
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
