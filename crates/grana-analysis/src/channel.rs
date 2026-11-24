use grana_volume::NoData;

#[derive(Clone)]
pub struct ChannelSet {
    detection: usize,
    no_data: Vec<Option<NoData>>,
}

impl ChannelSet {
    pub fn new(detection: usize, no_data: &[Option<NoData>]) -> Self {
        assert!(
            !no_data.is_empty() && detection < no_data.len(),
            "the detection channel is one of the measured channels"
        );
        Self {
            detection,
            no_data: no_data.to_vec(),
        }
    }

    pub fn count(&self) -> usize {
        self.no_data.len()
    }

    pub fn detection(&self) -> usize {
        self.detection
    }

    pub fn no_data(&self, channel: usize) -> Option<NoData> {
        self.no_data[channel]
    }
}
