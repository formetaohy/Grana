use std::any::Any;
use std::sync::Arc;

use grana_store::{Decode, Payload};

use crate::{IntensityTable, SampleStatistics, Spot, SpotSet};

const SPOT_BYTES: usize = std::mem::size_of::<Spot>();
const RECORD_BYTES: usize = 32;
const STATISTICS_BYTES: usize = 24;
const HEADER_BYTES: usize = 12;

pub struct SpotPage {
    spots: Arc<SpotSet>,
}

impl SpotPage {
    pub fn new(spots: Arc<SpotSet>) -> Self {
        Self { spots }
    }

    pub fn spots(&self) -> Arc<SpotSet> {
        Arc::clone(&self.spots)
    }
}

impl Payload for SpotPage {
    fn encode(&self) -> Vec<u8> {
        let set = self.spots.as_ref();
        let spots = set.spots();
        let intensities = set.intensities();
        let table = intensities.values();
        let mut bytes = Vec::with_capacity(encoded_bytes(spots.len(), intensities.channels()));
        bytes.extend_from_slice(&(set.level() as u32).to_le_bytes());
        bytes.extend_from_slice(&(spots.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(intensities.channels() as u32).to_le_bytes());
        for spot in spots {
            for value in spot.center().into_iter().chain(spot.sigma()) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&spot.quality().to_le_bytes());
            bytes.extend_from_slice(&spot.group().unwrap_or(u32::MAX).to_le_bytes());
        }
        for statistics in table {
            bytes.extend_from_slice(&statistics.mean().to_le_bytes());
            bytes.extend_from_slice(&statistics.minimum().to_le_bytes());
            bytes.extend_from_slice(&statistics.maximum().to_le_bytes());
            bytes.extend_from_slice(&statistics.deviation().to_le_bytes());
            bytes.extend_from_slice(&statistics.sum().to_le_bytes());
        }
        bytes
    }

    fn stored_bytes(&self) -> u64 {
        let set = self.spots.as_ref();
        (set.len() * (SPOT_BYTES + STATISTICS_BYTES * set.intensities().channels())) as u64
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

impl Decode for SpotPage {
    fn decode(bytes: &[u8]) -> Option<Self> {
        let level = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?) as usize;
        let count = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
        let channels = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?) as usize;
        if channels == 0 || bytes.len() != encoded_bytes(count, channels) {
            return None;
        }
        let mut cursor = HEADER_BYTES;
        let mut spots = Vec::with_capacity(count);
        for _ in 0..count {
            let mut value = || -> Option<f32> {
                let read = f32::from_le_bytes(bytes.get(cursor..cursor + 4)?.try_into().ok()?);
                cursor += 4;
                Some(read)
            };
            let center = [value()?, value()?, value()?];
            let sigma = [value()?, value()?, value()?];
            let quality = value()?;
            let group = u32::from_le_bytes(bytes.get(cursor..cursor + 4)?.try_into().ok()?);
            cursor += 4;
            if !center.iter().all(|value| value.is_finite())
                || !sigma.iter().all(|value| value.is_finite() && *value > 0.0)
                || !quality.is_finite()
            {
                return None;
            }
            let group = (group != u32::MAX).then_some(group);
            spots.push(Spot::grouped(center, sigma, quality, group));
        }
        let mut table = Vec::with_capacity(count * channels);
        for _ in 0..count * channels {
            let mut value = || -> Option<f32> {
                let read = f32::from_le_bytes(bytes.get(cursor..cursor + 4)?.try_into().ok()?);
                cursor += 4;
                Some(read)
            };
            let mean = value()?;
            let minimum = value()?;
            let maximum = value()?;
            let deviation = value()?;
            let sum = f64::from_le_bytes(bytes.get(cursor..cursor + 8)?.try_into().ok()?);
            cursor += 8;
            if ![mean, minimum, maximum, deviation]
                .iter()
                .all(|value| value.is_finite())
                || minimum > mean
                || mean > maximum
                || deviation < 0.0
                || !sum.is_finite()
                || sum < 0.0
            {
                return None;
            }
            table.push(SampleStatistics::new(
                mean, minimum, maximum, deviation, sum,
            ));
        }
        Some(Self::new(Arc::new(SpotSet::new(
            level,
            spots,
            IntensityTable::new(channels, table),
        ))))
    }
}

fn encoded_bytes(count: usize, channels: usize) -> usize {
    HEADER_BYTES + count * (RECORD_BYTES + STATISTICS_BYTES * channels)
}
