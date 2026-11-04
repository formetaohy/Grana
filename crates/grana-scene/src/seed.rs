use crate::object_set::ObjectSetId;

#[derive(Clone, Debug, PartialEq)]
pub enum Seeds {
    Points(Vec<Vec<[f32; 3]>>),
    Source(ObjectSetId),
}

impl Seeds {
    pub fn points(frames: usize) -> Self {
        Self::Points(vec![Vec::new(); frames])
    }

    pub fn source(&self) -> Option<ObjectSetId> {
        match self {
            Self::Source(source) => Some(*source),
            Self::Points(_) => None,
        }
    }

    pub fn frame(&self, time: u32) -> &[[f32; 3]] {
        match self {
            Self::Points(frames) => frames.get(time as usize).map_or(&[], Vec::as_slice),
            Self::Source(_) => &[],
        }
    }

    pub fn manual(&self) -> Option<&[Vec<[f32; 3]>]> {
        match self {
            Self::Points(frames) => Some(frames),
            Self::Source(_) => None,
        }
    }

    pub fn frames(&self) -> usize {
        match self {
            Self::Points(frames) => frames.len(),
            Self::Source(_) => 0,
        }
    }
}
