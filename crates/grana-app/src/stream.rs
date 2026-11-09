use std::time::{Duration, Instant};

const COARSE_START: usize = 2;
const LATENCY: f32 = 0.35;
const STEP: Duration = Duration::from_millis(400);

pub struct View {
    pub remote: bool,
    pub levels: usize,
    pub rate: Option<f32>,
    pub bricks: usize,
    pub rendered: usize,
    pub brick_bytes: u64,
    pub now: Instant,
}

pub struct Stream {
    levels: usize,
    cap: usize,
    stepped: Option<Instant>,
}

impl Default for Stream {
    fn default() -> Self {
        Self::new()
    }
}

impl Stream {
    pub fn new() -> Self {
        Self {
            levels: 0,
            cap: 0,
            stepped: None,
        }
    }

    pub fn cap(&mut self, view: View) -> usize {
        if view.levels == 0 {
            return 0;
        }
        let finest = view.levels - 1;
        if view.levels != self.levels {
            self.levels = view.levels;
            self.stepped = Some(view.now);
            self.cap = if view.remote {
                finest.saturating_sub(COARSE_START)
            } else {
                finest
            };
            return self.cap;
        }
        let target = if view.remote {
            if view.bricks == 0 {
                self.cap
            } else {
                affordable(&view).min(finest)
            }
        } else {
            finest
        };
        if target < self.cap {
            self.cap = target;
            self.stepped = Some(view.now);
        } else if target > self.cap
            && self
                .stepped
                .is_none_or(|at| view.now.duration_since(at) >= STEP)
        {
            self.cap += 1;
            self.stepped = Some(view.now);
        }
        self.cap
    }
}

fn affordable(view: &View) -> usize {
    let Some(rate) = view.rate else {
        return view.levels - 1 - COARSE_START.min(view.levels - 1);
    };
    let burst = view.bricks as f32 * view.brick_bytes as f32;
    let ratio = (LATENCY * rate / burst).max(f32::MIN_POSITIVE);
    let delta = (ratio.ln() / 4.0f32.ln()).floor();
    let level = (view.rendered as f32 + delta).max(0.0);
    (level as usize).min(view.levels - 1)
}
