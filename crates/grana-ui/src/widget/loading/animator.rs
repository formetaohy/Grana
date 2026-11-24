use crate::motion::{Animated, Spring};

const SHAPE_SECONDS: f32 = 0.65;
const SHAPE_LIMIT: f32 = 0.1;
const CONSTANT_ROTATION: f32 = 50.0;
const EXTRA_ROTATION: f32 = 90.0;
const MORPH: Spring = Spring::new(0.444_288_3, 0.4);

pub(crate) struct LoadingAnimator {
    morph: Animated,
    elapsed: f32,
    cycle: u32,
}

impl LoadingAnimator {
    pub(crate) fn new() -> Self {
        let mut morph = Animated::new(0.0, MORPH);
        morph.animate_to(1.0, MORPH);
        Self {
            morph,
            elapsed: 0.0,
            cycle: 0,
        }
    }

    pub(crate) fn step(&mut self, delta: f32) {
        let delta = delta.min(SHAPE_LIMIT);
        if delta <= 0.0 {
            return;
        }
        self.elapsed += delta;
        let cycle = (self.elapsed / SHAPE_SECONDS) as u32;
        if cycle > self.cycle {
            let advance = (cycle - self.cycle) as f32;
            self.morph.animate_to(self.morph.target + advance, MORPH);
            self.cycle = cycle;
        }
        self.morph.step(delta);
    }

    pub(crate) fn morph(&self) -> f32 {
        self.morph.value
    }

    pub(crate) fn rotation(&self) -> f32 {
        let base = self.morph.target - 1.0;
        let per_shape = self.morph.value - base;
        let fraction = (self.elapsed % SHAPE_SECONDS) / SHAPE_SECONDS;
        ((CONSTANT_ROTATION + EXTRA_ROTATION) * base
            + CONSTANT_ROTATION * fraction
            + EXTRA_ROTATION * per_shape)
            % 360.0
    }
}
