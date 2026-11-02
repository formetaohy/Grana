use std::collections::HashMap;
use std::ops::Range;

use glam::Vec2;

use crate::geometry::Rect;
use crate::motion::{Motion, Spring};

const CLICK_SLOP: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Pointing,
    Crosshair,
    Grab,
    Grabbing,
    ResizeHorizontal,
    ResizeVertical,
    Move,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub key: u64,
    pub rect: Rect,
    pub clip: Rect,
}

#[derive(Clone, Copy, Debug)]
pub struct Scroll {
    pub offset: f32,
    pub target: f32,
    pub velocity: f32,
    pub content: f32,
    pub viewport: f32,
    pub bar: f32,
    pub moving: bool,
}

impl Scroll {
    fn new() -> Self {
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
        let mut spring = Spring {
            value: self.offset,
            velocity: self.velocity,
            target: self.target,
        };
        spring.step(motion, elapsed);
        self.offset = spring.value;
        self.velocity = spring.velocity;
        self.moving = !spring.is_settled();
        let goal = if self.moving { 1.0 } else { 0.0 };
        let mut bar = Spring {
            value: self.bar,
            velocity: 0.0,
            target: goal,
        };
        bar.step(Motion::smooth(), elapsed);
        self.bar = bar.value;
    }

    pub fn settled(&self) -> bool {
        !self.moving && self.bar < 0.01
    }
}

#[derive(Clone, Copy, Debug)]
struct WheelTarget {
    key: u64,
    rect: Rect,
    overlay: bool,
}

pub struct Registry {
    hits: Vec<Hit>,
    next_hits: Vec<Hit>,
    hover: Option<u64>,
    press: Option<u64>,
    drag: Option<u64>,
    click: Option<u64>,
    press_position: Vec2,
    springs: HashMap<u64, Spring>,
    heights: HashMap<u64, f32>,
    values: HashMap<u64, f32>,
    open: HashMap<u64, bool>,
    scroll: HashMap<u64, Scroll>,
    popup: Option<u64>,
    popup_interacted: bool,
    occluders: Vec<Rect>,
    next_occluders: Vec<Rect>,
    wheel_targets: Vec<WheelTarget>,
    next_wheel_targets: Vec<WheelTarget>,
    wheel_owner: Option<u64>,
    cursor: Cursor,
    animating: bool,
    wake: bool,
    hovered_since: Option<(u64, f32)>,
    tooltip: Option<String>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            hits: Vec::new(),
            next_hits: Vec::new(),
            hover: None,
            press: None,
            drag: None,
            click: None,
            press_position: Vec2::ZERO,
            springs: HashMap::new(),
            heights: HashMap::new(),
            values: HashMap::new(),
            open: HashMap::new(),
            scroll: HashMap::new(),
            popup: None,
            popup_interacted: false,
            occluders: Vec::new(),
            next_occluders: Vec::new(),
            wheel_targets: Vec::new(),
            next_wheel_targets: Vec::new(),
            wheel_owner: None,
            cursor: Cursor::Arrow,
            animating: false,
            wake: false,
            hovered_since: None,
            tooltip: None,
        }
    }

    pub fn begin(&mut self, position: Vec2, pressed: bool, released: bool, down: bool, time: f32) {
        self.next_hits.clear();
        self.next_occluders.clear();
        self.next_wheel_targets.clear();
        let overlay = self
            .wheel_targets
            .iter()
            .rev()
            .find(|target| target.overlay && target.rect.contains(position));
        let any = self
            .wheel_targets
            .iter()
            .rev()
            .find(|target| target.rect.contains(position));
        self.wheel_owner = overlay.or(any).map(|target| target.key);
        self.hover = self
            .hits
            .iter()
            .rev()
            .find(|hit| hit.clip.contains(position) && hit.rect.contains(position))
            .map(|hit| hit.key);
        if pressed {
            self.press = self.hover;
            self.press_position = position;
            self.drag = None;
            self.click = None;
        }
        if down
            && let Some(press) = self.press
            && self.drag.is_none()
            && (position - self.press_position).length() > CLICK_SLOP
        {
            self.drag = Some(press);
        }
        if released {
            self.click = if self.drag.is_none() && self.hover == self.press {
                self.press
            } else {
                None
            };
            self.press = None;
            self.drag = None;
        }
        match (self.hover, self.hovered_since) {
            (Some(key), Some((previous, since))) if key == previous => {
                self.tooltip = (time - since > 0.55).then_some(String::new());
            }
            _ => {
                self.hovered_since = self.hover.map(|key| (key, time));
                self.tooltip = None;
            }
        }
        self.cursor = Cursor::Arrow;
        self.animating = false;
        self.wake = false;
        self.popup_interacted = false;
    }

    pub fn end(&mut self, elapsed: f32) {
        for scroll in self.scroll.values_mut() {
            if !scroll.settled() {
                scroll.update(elapsed, Motion::quick());
            }
        }
        std::mem::swap(&mut self.hits, &mut self.next_hits);
        std::mem::swap(&mut self.occluders, &mut self.next_occluders);
        std::mem::swap(&mut self.wheel_targets, &mut self.next_wheel_targets);
    }

    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    pub fn register(&mut self, key: u64, rect: Rect, clip: Rect) {
        self.next_hits.push(Hit { key, rect, clip });
    }

    pub fn register_occluder(&mut self, rect: Rect) {
        self.next_occluders.push(rect);
    }

    pub fn register_wheel(&mut self, key: u64, rect: Rect, overlay: bool) {
        self.next_wheel_targets
            .push(WheelTarget { key, rect, overlay });
    }

    pub fn hit_count(&self) -> usize {
        self.next_hits.len()
    }

    pub fn wheel_count(&self) -> usize {
        self.next_wheel_targets.len()
    }

    pub fn shift_hits(&mut self, range: Range<usize>, delta: Vec2) {
        for hit in &mut self.next_hits[range] {
            hit.rect = hit.rect.translate(delta);
            hit.clip = hit.clip.translate(delta);
        }
    }

    pub fn shift_wheels(&mut self, range: Range<usize>, delta: Vec2) {
        for target in &mut self.next_wheel_targets[range] {
            target.rect = target.rect.translate(delta);
        }
    }

    pub fn wheel_owner(&self) -> Option<u64> {
        self.wheel_owner
    }

    pub fn occluders(&self) -> &[Rect] {
        &self.occluders
    }

    pub fn hovered(&self, key: u64) -> bool {
        self.hover == Some(key)
    }

    pub fn pressed(&self, key: u64) -> bool {
        self.press == Some(key)
    }

    pub fn dragging(&self, key: u64) -> bool {
        self.drag == Some(key)
    }

    pub fn clicked(&self, key: u64) -> bool {
        self.click == Some(key)
    }

    pub fn active(&self, key: u64) -> bool {
        self.press == Some(key)
    }

    pub fn spring(&mut self, key: u64, target: f32, motion: Motion, elapsed: f32) -> f32 {
        let spring = self
            .springs
            .entry(key)
            .or_insert_with(|| Spring::new(target));
        spring.target = target;
        if !spring.is_settled() {
            spring.step(motion, elapsed);
            self.animating = true;
        }
        spring.value
    }

    pub fn spring_value(&self, key: u64, fallback: f32) -> f32 {
        self.springs
            .get(&key)
            .map_or(fallback, |spring| spring.value)
    }

    pub fn open_state(&self, key: u64) -> Option<bool> {
        self.open.get(&key).copied()
    }

    pub fn set_open_state(&mut self, key: u64, open: bool) {
        self.open.insert(key, open);
    }

    pub fn value(&self, key: u64) -> Option<f32> {
        self.values.get(&key).copied()
    }

    pub fn set_value(&mut self, key: u64, value: f32) {
        self.values.insert(key, value);
    }

    pub fn height(&self, key: u64) -> Option<f32> {
        self.heights.get(&key).copied()
    }

    pub fn set_height(&mut self, key: u64, height: f32) {
        self.heights.insert(key, height);
    }

    pub fn scroll_state(&self, key: u64) -> Scroll {
        self.scroll.get(&key).copied().unwrap_or_else(|| {
            let mut scroll = Scroll::new();
            scroll.offset = 0.0;
            scroll
        })
    }

    pub fn captured(&self) -> bool {
        self.press.is_some()
    }

    pub fn scroll(&mut self, key: u64) -> &mut Scroll {
        self.scroll.entry(key).or_insert_with(Scroll::new)
    }

    pub fn popup(&self) -> Option<u64> {
        self.popup
    }

    pub fn open_popup(&mut self, key: u64) {
        self.popup = Some(key);
        self.popup_interacted = true;
    }

    pub fn close_popup(&mut self) {
        self.popup = None;
    }

    pub fn popup_interacted(&self) -> bool {
        self.popup_interacted
    }

    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.cursor = cursor;
    }

    pub fn animating(&self) -> bool {
        self.animating || self.wake
    }

    pub fn wake(&mut self) {
        self.wake = true;
    }

    pub fn tooltip(&self) -> Option<&str> {
        self.tooltip.as_deref()
    }

    pub fn request_tooltip(&mut self, text: &str) {
        if self.tooltip.as_deref().is_some_and(str::is_empty) {
            self.tooltip = Some(text.to_string());
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}
