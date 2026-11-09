use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_millis(200);
const STALE: Duration = Duration::from_secs(2);
const SMOOTHING: f32 = 0.5;

#[derive(Default)]
pub struct Transfer {
    bytes: u64,
    start: Option<Instant>,
    rate: Option<f32>,
}

impl Transfer {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn record(&mut self, bytes: u64, now: Instant) {
        self.bytes = self.bytes.saturating_add(bytes);
        let Some(start) = self.start else {
            self.start = Some(now);
            return;
        };
        let elapsed = now.duration_since(start);
        if elapsed < WINDOW {
            return;
        }
        if elapsed > STALE {
            self.bytes = 0;
            self.start = Some(now);
            return;
        }
        let measured = self.bytes as f32 / elapsed.as_secs_f32();
        self.rate = Some(match self.rate {
            Some(previous) => SMOOTHING * measured + (1.0 - SMOOTHING) * previous,
            None => measured,
        });
        self.bytes = 0;
        self.start = Some(now);
    }

    pub fn rate(&self) -> Option<f32> {
        self.rate.filter(|rate| rate.is_finite() && *rate > 0.0)
    }
}
