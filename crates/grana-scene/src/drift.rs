use std::sync::Arc;

const LIMIT: i32 = 1 << 20;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Drift {
    channel: usize,
    reference: u32,
    offsets: Arc<[[i32; 3]]>,
}

impl Drift {
    pub const LIMIT: i32 = LIMIT;

    pub fn new(channel: usize, reference: u32, offsets: Vec<[i32; 3]>) -> Self {
        assert!(
            offsets
                .iter()
                .flatten()
                .all(|value| value.unsigned_abs() <= LIMIT as u32),
            "a drift offset stays inside the volume"
        );
        assert!(
            offsets.is_empty() || reference < offsets.len() as u32,
            "a drift reference is one of the aligned frames"
        );
        assert!(
            offsets.is_empty() || offsets[reference as usize] == [0; 3],
            "the reference frame holds still"
        );
        Self {
            channel,
            reference,
            offsets: offsets.into(),
        }
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn reference(&self) -> u32 {
        self.reference
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

    pub fn referenced_at(&self, reference: u32) -> Self {
        assert!(
            (reference as usize) < self.offsets.len(),
            "a drift reference is one of the aligned frames"
        );
        let anchor = self.offsets[reference as usize];
        let offsets = self
            .offsets
            .iter()
            .map(|offset| std::array::from_fn(|axis| offset[axis] - anchor[axis]))
            .collect();
        Self::new(self.channel, reference, offsets)
    }

    pub fn with_offset(&self, time: u32, offset: [i32; 3]) -> Self {
        assert!(
            (time as usize) < self.offsets.len(),
            "a drift offset belongs to an aligned frame"
        );
        assert!(time != self.reference, "the reference frame holds still");
        let mut offsets = self.offsets.to_vec();
        offsets[time as usize] = offset;
        Self::new(self.channel, self.reference, offsets)
    }
}
