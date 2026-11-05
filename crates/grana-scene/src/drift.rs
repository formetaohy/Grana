use std::sync::Arc;

const LIMIT: i32 = 1 << 20;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Drift {
    offsets: Arc<[[i32; 3]]>,
}

impl Drift {
    pub const LIMIT: i32 = LIMIT;

    pub fn new(offsets: Vec<[i32; 3]>) -> Self {
        assert!(
            offsets
                .iter()
                .flatten()
                .all(|value| value.unsigned_abs() <= LIMIT as u32),
            "a drift offset stays inside the volume"
        );
        Self {
            offsets: offsets.into(),
        }
    }

    pub fn offsets(&self) -> &[[i32; 3]] {
        &self.offsets
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    pub fn frames(&self) -> u32 {
        self.offsets.len() as u32
    }

    pub fn offset(&self, time: u32) -> [i32; 3] {
        self.offsets.get(time as usize).copied().unwrap_or([0; 3])
    }

    pub fn magnitude(&self) -> i32 {
        self.offsets
            .iter()
            .flatten()
            .map(|value| value.abs())
            .max()
            .unwrap_or(0)
    }
}
