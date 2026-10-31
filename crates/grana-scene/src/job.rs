use crate::object_set::ObjectSetId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Detect,
    Track,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub set: ObjectSetId,
    pub job: Job,
    pub time: u32,
    pub done: usize,
    pub total: usize,
}
