use grana_glass::Spring as Analytic;

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

    pub fn reduced(self) -> Self {
        Self::response(self.duration, 0.0)
    }

    pub fn spring(self) -> Analytic {
        Analytic::new(self.duration, self.bounce)
    }

    pub fn is_instant(self) -> bool {
        self.duration <= 0.0
    }
}
