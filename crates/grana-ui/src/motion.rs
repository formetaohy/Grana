use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub stiffness: f32,
    pub damping: f32,
}

impl Motion {
    pub const INSTANT: Self = Self {
        stiffness: 0.0,
        damping: 0.0,
    };

    pub fn response(seconds: f32, bounce: f32) -> Self {
        let stiffness = (TAU / seconds.max(0.01)).powi(2);
        let damping = 2.0 * (1.0 - bounce.clamp(0.0, 0.9)) * stiffness.sqrt();
        Self { stiffness, damping }
    }

    pub fn snappy() -> Self {
        Self::response(0.34, 0.12)
    }

    pub fn smooth() -> Self {
        Self::response(0.46, 0.0)
    }

    pub fn bouncy() -> Self {
        Self::response(0.42, 0.32)
    }

    pub fn quick() -> Self {
        Self::response(0.18, 0.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
}

impl Spring {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
        }
    }

    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() < 1.0e-4 && self.velocity.abs() < 1.0e-3
    }

    pub fn step(&mut self, motion: Motion, seconds: f32) {
        if motion.stiffness <= 0.0 {
            self.value = self.target;
            self.velocity = 0.0;
            return;
        }
        let mut remaining = seconds.clamp(0.0, 0.1);
        const SUBSTEP: f32 = 1.0 / 240.0;
        while remaining > 0.0 {
            let dt = remaining.min(SUBSTEP);
            let acceleration =
                motion.stiffness * (self.target - self.value) - motion.damping * self.velocity;
            self.velocity += acceleration * dt;
            self.value += self.velocity * dt;
            remaining -= dt;
        }
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }
}
