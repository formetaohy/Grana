use grana_display::PANE_COUNT;

use crate::profile::Budget;

const STATES: [(u32, f32); 18] = [
    (1, 1.0),
    (1, 1.35),
    (1, 1.8),
    (1, 2.4),
    (1, 3.2),
    (1, 4.3),
    (1, 5.8),
    (1, 7.7),
    (1, 10.3),
    (1, 13.8),
    (2, 3.2),
    (2, 4.3),
    (2, 5.8),
    (2, 7.7),
    (2, 10.3),
    (4, 5.8),
    (4, 7.7),
    (4, 10.3),
];

const SETTLE: u8 = 10;
const TIGHT: f32 = 0.9;
const SLACK: f32 = 0.55;
const REVERSE: f32 = 1.05;
const JUMP: f32 = 0.28;
const JUMP_LIMIT: usize = 5;
const START: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Load {
    pub scalable: bool,
    pub moved: bool,
}

impl Load {
    pub const IDLE: Self = Self {
        scalable: false,
        moved: false,
    };

    pub fn volume(scalable: bool, moved: bool) -> Self {
        Self { scalable, moved }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resolution {
    pub scale: [u32; PANE_COUNT],
    pub penalty: f32,
}

impl Default for Resolution {
    fn default() -> Self {
        let (scale, penalty) = STATES[0];
        Self {
            scale: [scale; PANE_COUNT],
            penalty,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Rise,
    Fall,
}

pub struct Governor {
    budget: Budget,
    index: usize,
    observed: bool,
    measured: bool,
    settle: u8,
    last: Option<Move>,
}

impl Governor {
    pub fn new(target: f32) -> Self {
        Self {
            budget: Budget::new(target),
            index: START,
            observed: false,
            measured: false,
            settle: 0,
            last: None,
        }
    }

    pub fn target(&self) -> f32 {
        self.budget.target()
    }

    pub fn penalty(&self) -> f32 {
        STATES[self.index].1
    }

    pub fn retarget(&mut self, target: f32) {
        self.budget = self.budget.retarget(target);
        self.forget();
    }

    pub fn observe(&mut self, elapsed: f32) {
        self.budget.observe(elapsed);
        self.observed = true;
        self.measured = true;
    }

    pub fn decide(&mut self, loads: &[Load; PANE_COUNT], frozen: bool) -> Resolution {
        if frozen || !self.measured {
            self.forget();
            return Resolution::default();
        }
        let observed = std::mem::take(&mut self.observed);
        let interactive = loads.iter().any(|load| load.scalable && load.moved);
        if !interactive {
            self.forget();
            return Resolution::default();
        }
        if observed {
            self.advance();
        }
        let (scale, penalty) = STATES[self.index];
        Resolution {
            scale: [scale; PANE_COUNT],
            penalty,
        }
    }

    fn advance(&mut self) {
        if self.settle > 0 {
            self.settle -= 1;
            return;
        }
        let ratio = self.budget.pressure();
        if ratio > TIGHT {
            let confidence = if self.last == Some(Move::Rise) {
                REVERSE
            } else {
                TIGHT
            };
            if ratio > confidence {
                let jumps = (((ratio - TIGHT) / JUMP).ceil() as usize).clamp(1, JUMP_LIMIT);
                self.index = (self.index + jumps).min(STATES.len() - 1);
                self.settle = SETTLE;
                self.last = Some(Move::Fall);
            }
        } else if ratio < SLACK && self.index > 0 {
            self.index -= 1;
            self.settle = SETTLE;
            self.last = Some(Move::Rise);
        }
    }

    fn forget(&mut self) {
        self.observed = false;
        self.measured = false;
        self.settle = 0;
        self.last = None;
    }
}
