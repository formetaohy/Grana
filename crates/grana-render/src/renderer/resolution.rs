use grana_display::PANE_COUNT;

use crate::profile::Budget;

const SCALE: u32 = 2;
const QUARTER: u32 = 4;
const MOTION_FLOOR: u64 = 1 << 21;
const OVERRUN_FLOOR: u64 = 1 << 20;
const RECOVERY: u8 = 8;
const STEP: f32 = 1.15;
const PENALTY_FLOOR: f32 = 1.0;
const PENALTY_CEILING: f32 = 4.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Cause {
    #[default]
    None,
    Motion,
    Overrun,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Load {
    pub pixels: u64,
    pub allowed: bool,
    pub moved: bool,
}

impl Load {
    pub const IDLE: Self = Self {
        pixels: 0,
        allowed: false,
        moved: false,
    };

    pub fn volume(pixels: u64, allowed: bool, moved: bool) -> Self {
        Self {
            pixels,
            allowed,
            moved,
        }
    }

    fn motion(self) -> bool {
        self.allowed && self.moved && self.pixels >= MOTION_FLOOR
    }

    fn overrun(self) -> bool {
        self.allowed && self.pixels >= OVERRUN_FLOOR
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

pub struct Governor {
    budget: Budget,
    cause: [Cause; PANE_COUNT],
    streak: [u8; PANE_COUNT],
    probed: [bool; PANE_COUNT],
    penalty: f32,
    observed: bool,
}

impl Governor {
    pub fn new(target: f32) -> Self {
        Self {
            budget: Budget::new(target),
            cause: [Cause::None; PANE_COUNT],
            streak: [0; PANE_COUNT],
            probed: [false; PANE_COUNT],
            penalty: PENALTY_FLOOR,
            observed: false,
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
    }

    pub fn decide(&mut self, loads: &[Load; PANE_COUNT], frozen: bool) -> Resolution {
        if frozen {
            self.forget();
            return Resolution::default();
        }
        let observed = std::mem::take(&mut self.observed);
        let overrun = self.budget.overrun();
        let headroom = self.budget.headroom();
        let severe = self.budget.severe();
        let mut escalated = false;
        for (index, load) in loads.iter().enumerate() {
            let motion = load.motion();
            let scalable = load.overrun();
            self.cause[index] = match self.cause[index] {
                Cause::None => {
                    if motion {
                        self.reset(index);
                        Cause::Motion
                    } else if scalable && overrun {
                        escalated = true;
                        self.streak[index] = 0;
                        Cause::Overrun
                    } else {
                        self.settle(index, observed, overrun);
                        Cause::None
                    }
                }
                Cause::Motion => {
                    if motion {
                        Cause::Motion
                    } else if scalable && overrun {
                        escalated = true;
                        self.streak[index] = 0;
                        Cause::Overrun
                    } else {
                        self.reset(index);
                        Cause::None
                    }
                }
                Cause::Overrun => {
                    if motion {
                        self.reset(index);
                        Cause::Motion
                    } else if !scalable {
                        self.reset(index);
                        Cause::None
                    } else if headroom && observed {
                        self.streak[index] = self.streak[index].saturating_add(1);
                        if self.streak[index] >= RECOVERY {
                            self.streak[index] = 0;
                            if self.probed[index] {
                                Cause::Overrun
                            } else {
                                self.probed[index] = true;
                                Cause::None
                            }
                        } else {
                            Cause::Overrun
                        }
                    } else {
                        if !headroom {
                            self.streak[index] = 0;
                        }
                        Cause::Overrun
                    }
                }
            };
        }
        if observed {
            if overrun && !escalated {
                self.penalty = (self.penalty * STEP).min(PENALTY_CEILING);
            } else if headroom {
                self.penalty = (self.penalty / STEP).max(PENALTY_FLOOR);
            }
        }
        Resolution {
            scale: self.cause.map(|cause| match (cause, severe) {
                (Cause::None, _) => 1,
                (Cause::Motion, _) => SCALE,
                (Cause::Overrun, true) => QUARTER,
                (Cause::Overrun, false) => SCALE,
            }),
            penalty: self.penalty,
        }
    }

    fn reset(&mut self, index: usize) {
        self.streak[index] = 0;
        self.probed[index] = false;
    }

    fn settle(&mut self, index: usize, observed: bool, overrun: bool) {
        if !observed {
            return;
        }
        if overrun {
            self.streak[index] = 0;
            return;
        }
        self.streak[index] = self.streak[index].saturating_add(1);
        if self.streak[index] >= RECOVERY {
            self.streak[index] = 0;
            self.probed[index] = false;
        }
    }

    fn forget(&mut self) {
        self.cause = [Cause::None; PANE_COUNT];
        self.streak = [0; PANE_COUNT];
        self.probed = [false; PANE_COUNT];
        self.penalty = PENALTY_FLOOR;
        self.observed = false;
    }
}
