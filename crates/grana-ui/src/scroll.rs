use grana_glass::Animated;

use crate::motion::Motion;

#[derive(Clone, Copy, Debug)]
pub struct ScrollState {
    pub offset: f32,
    pub target: f32,
    pub velocity: f32,
    pub content: f32,
    pub viewport: f32,
    pub bar: f32,
    pub moving: bool,
}

impl ScrollState {
    pub fn new() -> Self {
        Self {
            offset: 0.0,
            target: 0.0,
            velocity: 0.0,
            content: 0.0,
            viewport: 0.0,
            bar: 0.0,
            moving: false,
        }
    }

    pub fn limit(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }

    pub fn scroll_by(&mut self, amount: f32) {
        self.target = (self.target + amount).clamp(0.0, self.limit());
        self.moving = true;
    }

    pub fn update(&mut self, elapsed: f32, motion: Motion) {
        self.target = self.target.clamp(0.0, self.limit());
        let mut spring = Animated::new(self.offset, motion.spring());
        spring.velocity = self.velocity;
        spring.animate_to(self.target, motion.spring());
        if motion.is_instant() {
            spring.snap(self.target);
        } else {
            spring.step(elapsed);
        }
        self.offset = spring.value;
        self.velocity = spring.velocity;
        self.moving = !spring.is_at_rest();
        let goal = if self.moving { 1.0 } else { 0.0 };
        let smooth = if motion.is_instant() {
            Motion::INSTANT
        } else {
            Motion::smooth()
        };
        let mut bar = Animated::new(self.bar, smooth.spring());
        bar.animate_to(goal, smooth.spring());
        if motion.is_instant() {
            bar.snap(goal);
        } else {
            bar.step(elapsed);
        }
        self.bar = bar.value;
    }

    pub fn settled(&self) -> bool {
        !self.moving && self.bar < 0.01
    }
}

impl Default for ScrollState {
    fn default() -> Self {
        Self::new()
    }
}
