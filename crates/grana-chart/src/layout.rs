use glam::Vec2;
use grana_ui::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insets {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Insets {
    pub const fn new(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    pub fn frame(self, rect: Rect) -> Rect {
        Rect::from_min_max(
            Vec2::new(rect.min.x + self.left, rect.min.y + self.top),
            Vec2::new(rect.max.x - self.right, rect.max.y - self.bottom),
        )
    }
}

pub fn slot(frame: Rect, count: usize) -> f32 {
    frame.width() / count.max(1) as f32
}

pub fn group_centre(frame: Rect, index: usize, count: usize) -> f32 {
    frame.min.x + slot(frame, count) * (index as f32 + 0.5)
}

pub fn group_at(frame: Rect, count: usize, pointer: Vec2) -> Option<usize> {
    if count == 0 || !frame.contains(pointer) {
        return None;
    }
    let index = ((pointer.x - frame.min.x) / slot(frame, count).max(1.0)) as usize;
    Some(index.min(count - 1))
}

pub fn cell_centre(frame: Rect, column: usize, row: usize, bins: usize) -> Vec2 {
    let width = frame.width() / bins.max(1) as f32;
    let height = frame.height() / bins.max(1) as f32;
    Vec2::new(
        frame.min.x + width * (column as f32 + 0.5),
        frame.max.y - height * (row as f32 + 0.5),
    )
}

pub fn cell_size(frame: Rect, bins: usize) -> Vec2 {
    Vec2::new(
        frame.width() / bins.max(1) as f32,
        frame.height() / bins.max(1) as f32,
    )
}
