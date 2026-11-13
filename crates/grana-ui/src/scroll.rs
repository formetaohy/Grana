use crate::element::Axis;
use crate::motion::{Animated, Motion};
use crate::node::NodeId;
use crate::ui::Ui;

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

impl<A: Clone> Ui<A> {
    pub(crate) fn advance_scroll(&mut self) {
        let elapsed = self.elapsed;
        let motion = Motion::quick();
        let mut scrolling = std::mem::take(&mut self.scrolling);
        let mut moving = Vec::with_capacity(scrolling.len());
        for id in scrolling.drain(..) {
            if !self.tree.nodes.contains_key(id) {
                continue;
            }
            let (moved, settled) = {
                let Some(scroll) = self.tree.nodes[id].scroll.as_mut() else {
                    continue;
                };
                if scroll.settled() {
                    continue;
                }
                let before = scroll.offset;
                scroll.update(elapsed, motion);
                (scroll.offset != before, scroll.settled())
            };
            if moved || !settled {
                self.tree.nodes[id].paint_dirty = true;
                self.tree.invalidate_arrange(id);
                self.animating = true;
            }
            if !settled {
                moving.push(id);
            }
        }
        self.scrolling = moving;
    }

    pub(crate) fn update_scroll_metrics(&mut self) {
        let mut wake = Vec::new();
        for (id, node) in self.tree.nodes.iter_mut() {
            let Some(spec) = node.scroll_spec else {
                continue;
            };
            let viewport = match spec.axis {
                Axis::Column => node.rect.height(),
                Axis::Row => node.rect.width(),
            };
            let content = spec.content.unwrap_or(node.content_main).max(viewport);
            let scroll = node.scroll.get_or_insert_with(ScrollState::new);
            scroll.viewport = viewport;
            scroll.content = content;
            scroll.target = scroll.target.clamp(0.0, scroll.limit());
            if (scroll.target - scroll.offset).abs() > 1.0e-4 {
                scroll.moving = true;
            }
            if scroll.moving {
                wake.push(id);
            }
        }
        for id in wake {
            self.wake_scroll(id);
        }
    }

    pub(crate) fn wake_scroll(&mut self, id: NodeId) {
        if !self.scrolling.contains(&id) {
            self.scrolling.push(id);
        }
    }
}
