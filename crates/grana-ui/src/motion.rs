use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub duration: f32,
    pub bounce: f32,
}

impl Spring {
    pub const fn new(duration: f32, bounce: f32) -> Self {
        Self { duration, bounce }
    }

    fn angular_frequency(self) -> f32 {
        TAU / self.duration.max(1e-3)
    }

    fn damping_ratio(self) -> f32 {
        let bounce = self.bounce.clamp(-0.95, 0.95);
        if bounce >= 0.0 {
            1.0 - bounce
        } else {
            1.0 / (1.0 + bounce)
        }
    }

    pub fn solve(self, x0: f32, v0: f32, t: f32) -> (f32, f32) {
        let w0 = self.angular_frequency();
        let zeta = self.damping_ratio();
        if zeta < 1.0 {
            let a = zeta * w0;
            let wd = w0 * (1.0 - zeta * zeta).sqrt();
            let (s, c) = (wd * t).sin_cos();
            let e = (-a * t).exp();
            let b = (v0 + a * x0) / wd;
            (e * (x0 * c + b * s), e * (v0 * c - (x0 * wd + a * b) * s))
        } else if zeta == 1.0 {
            let e = (-w0 * t).exp();
            let b = v0 + w0 * x0;
            (e * (x0 + b * t), e * (v0 - w0 * b * t))
        } else {
            let q = (zeta * zeta - 1.0).sqrt();
            let (r1, r2) = (-w0 * (zeta - q), -w0 * (zeta + q));
            let c2 = (v0 - r1 * x0) / (r2 - r1);
            let c1 = x0 - c2;
            let (e1, e2) = ((r1 * t).exp(), (r2 * t).exp());
            (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Animated {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    pub spring: Spring,
}

const REST_EPSILON: f32 = 1e-3;
const REST_STEPS: f32 = 16.0;

impl Animated {
    pub const fn new(value: f32, spring: Spring) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            spring,
        }
    }

    pub fn animate_to(&mut self, target: f32, spring: Spring) {
        self.target = target;
        self.spring = spring;
    }

    pub fn snap(&mut self, value: f32) {
        *self = Self::new(value, self.spring);
    }

    pub fn step(&mut self, dt: f32) -> bool {
        if self.is_at_rest() {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        let (x, v) = self
            .spring
            .solve(self.value - self.target, self.velocity, dt);
        if !(x.is_finite() && v.is_finite()) {
            if !self.target.is_finite() {
                self.target = if self.value.is_finite() {
                    self.value
                } else {
                    0.0
                };
            }
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        let value = self.target + x;
        if dt > 0.0 && value == self.value && x.abs() <= self.stall_distance(dt) {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        self.value = value;
        self.velocity = v;
        true
    }

    pub fn is_at_rest(&self) -> bool {
        let offset = self.value - self.target;
        if offset.abs() < REST_EPSILON && self.velocity.abs() < REST_EPSILON {
            return true;
        }
        let limit = REST_STEPS * self.float_step();
        offset.abs() <= limit
            && offset.hypot(self.velocity / self.spring.angular_frequency()) <= limit
    }

    fn float_step(&self) -> f32 {
        self.target.abs().next_up() - self.target.abs()
    }

    fn stall_distance(&self, dt: f32) -> f32 {
        let creep_per_offset =
            self.spring.angular_frequency() / (2.0 * self.spring.damping_ratio());
        self.float_step() / (creep_per_offset * dt.max(1e-3))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    duration: f32,
    bounce: f32,
}

impl Motion {
    pub const INSTANT: Self = Self {
        duration: 0.0,
        bounce: 0.0,
    };

    pub fn response(seconds: f32, bounce: f32) -> Self {
        Self {
            duration: seconds.max(0.0),
            bounce: bounce.clamp(-0.95, 0.95),
        }
    }

    pub fn snappy() -> Self {
        Self::response(0.24, 0.04)
    }

    pub fn smooth() -> Self {
        Self::response(0.32, 0.0)
    }

    pub fn bouncy() -> Self {
        Self::response(0.26, 0.12)
    }

    pub fn quick() -> Self {
        Self::response(0.14, 0.0)
    }

    pub fn reduced(self) -> Self {
        Self::response(self.duration, 0.0)
    }

    pub fn spring(self) -> Spring {
        Spring::new(self.duration, self.bounce)
    }

    pub fn is_instant(self) -> bool {
        self.duration <= 0.0
    }
}
