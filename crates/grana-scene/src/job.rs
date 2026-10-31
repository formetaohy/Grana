use crate::object_set::ObjectSetId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Detect(ObjectSetId),
    Track(ObjectSetId),
    Coloc,
}

impl Job {
    pub fn set(self) -> Option<ObjectSetId> {
        match self {
            Self::Detect(set) | Self::Track(set) => Some(set),
            Self::Coloc => None,
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
