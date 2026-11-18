#[derive(Clone, Copy)]
pub struct ChannelSet {
    count: usize,
    detection: usize,
}

impl ChannelSet {
    pub fn new(count: usize, detection: usize) -> Self {
        assert!(
            count > 0 && detection < count,
            "the detection channel is one of the measured channels"
        );
        Self { count, detection }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn detection(&self) -> usize {
        self.detection
    }
}
