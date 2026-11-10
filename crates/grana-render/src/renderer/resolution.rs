use grana_display::PANE_COUNT;

use crate::profile::Budget;

const SCALE: u32 = 2;
const COARSE: u32 = 4;
const RECOVERY: u8 = 8;
const COOLDOWN: u16 = 30;
const SETTLE: u8 = 4;
const PRESSURE: u8 = 2;
const STEP: f32 = 1.15;
const PENALTY_FLOOR: f32 = 1.0;
const PENALTY_CEILING: f32 = 4.0;

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
        Self {
            scale: [1; PANE_COUNT],
            penalty: PENALTY_FLOOR,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Pane {
    scale: u32,
    probing: bool,
    streak: u8,
    cooldown: u16,
}

impl Default for Pane {
    fn default() -> Self {
        Self {
            scale: 1,
            probing: false,
            streak: 0,
            cooldown: 0,
        }
    }
}

pub struct Governor {
    budget: Budget,
    panes: [Pane; PANE_COUNT],
    penalty: f32,
    observed: bool,
    measured: bool,
    settle: u8,
    pressure: u8,
}

impl Governor {
    pub fn new(target: f32) -> Self {
        Self {
            budget: Budget::new(target),
            panes: [Pane::default(); PANE_COUNT],
            penalty: PENALTY_FLOOR,
            observed: false,
            measured: false,
            settle: 0,
            pressure: 0,
        }
    }

    pub fn target(&self) -> f32 {
        self.budget.target()
    }

    pub fn penalty(&self) -> f32 {
        self.penalty
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
        if frozen {
            self.forget();
            return Resolution::default();
        }
        if !self.measured {
            return Resolution::default();
        }
        let observed = std::mem::take(&mut self.observed);
        let overrun = self.budget.overrun();
        self.pressure = if observed && overrun {
            (self.pressure + 1).min(PRESSURE)
        } else {
            0
        };
        let headroom = self.budget.headroom();
        let mut interactive = false;
        for (index, load) in loads.iter().enumerate() {
            let pane = &mut self.panes[index];
            if !(load.scalable && load.moved) {
                *pane = Pane::default();
                continue;
            }
            interactive = true;
            if self.settle > 0 {
                continue;
            }
            if overrun && self.pressure >= PRESSURE {
                pane.streak = 0;
                if pane.probing {
                    pane.probing = false;
                    pane.cooldown = COOLDOWN;
                }
                pane.scale = (pane.scale * SCALE).min(COARSE);
                self.settle = SETTLE;
                self.pressure = 0;
                continue;
            }
            if pane.scale == 1 {
                continue;
            }
            if pane.cooldown > 0 {
                pane.cooldown -= 1;
                pane.streak = 0;
                continue;
            }
            if !observed || !headroom {
                pane.streak = 0;
                continue;
            }
            pane.streak = pane.streak.saturating_add(1);
            if pane.streak < RECOVERY {
                continue;
            }
            pane.streak = 0;
            pane.probing = true;
            pane.scale /= SCALE;
            self.settle = SETTLE;
        }
        if self.settle > 0 {
            self.settle -= 1;
        }
        if observed && interactive {
            let coarsest = self.panes.iter().any(|pane| pane.scale == COARSE);
            if overrun && coarsest {
                self.penalty = (self.penalty * STEP).min(PENALTY_CEILING);
            } else if headroom {
                self.penalty = (self.penalty / STEP).max(PENALTY_FLOOR);
            }
        }
        Resolution {
            scale: std::array::from_fn(|index| self.panes[index].scale),
            penalty: if interactive {
                self.penalty
            } else {
                PENALTY_FLOOR
            },
        }
    }

    fn forget(&mut self) {
        self.panes = [Pane::default(); PANE_COUNT];
        self.penalty = PENALTY_FLOOR;
        self.observed = false;
        self.measured = false;
        self.settle = 0;
        self.pressure = 0;
    }
}
