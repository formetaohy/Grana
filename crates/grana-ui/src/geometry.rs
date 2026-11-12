use glam::Vec2;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    pub const ZERO: Self = Self {
        min: Vec2::ZERO,
        max: Vec2::ZERO,
    };

    pub fn from_min_size(min: Vec2, size: Vec2) -> Self {
        Self {
            min,
            max: min + size,
        }
    }

    pub fn from_min_max(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        Self {
            min: center - 0.5 * size,
            max: center + 0.5 * size,
        }
    }

    pub fn from_bounds(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            min: Vec2::new(left, top),
            max: Vec2::new(right, bottom),
        }
    }

    pub fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn size(self) -> Vec2 {
        self.max - self.min
    }

    pub fn center(self) -> Vec2 {
        0.5 * (self.min + self.max)
    }

    pub fn left(self) -> f32 {
        self.min.x
    }

    pub fn top(self) -> f32 {
        self.min.y
    }

    pub fn right(self) -> f32 {
        self.max.x
    }

    pub fn bottom(self) -> f32 {
        self.max.y
    }

    pub fn is_empty(self) -> bool {
        self.max.x <= self.min.x || self.max.y <= self.min.y
    }

    pub fn contains(self, point: Vec2) -> bool {
        point.x >= self.min.x
            && point.x < self.max.x
            && point.y >= self.min.y
            && point.y < self.max.y
    }

    pub fn intersects(self, other: Self) -> bool {
        self.min.x < other.max.x
            && other.min.x < self.max.x
            && self.min.y < other.max.y
            && other.min.y < self.max.y
    }

    pub fn intersect(self, other: Self) -> Self {
        Self {
            min: self.min.max(other.min),
            max: self.max.min(other.max),
        }
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn translate(self, offset: Vec2) -> Self {
        Self {
            min: self.min + offset,
            max: self.max + offset,
        }
    }

    pub fn shrink(self, amount: f32) -> Self {
        self.shrink2(Vec2::splat(amount))
    }

    pub fn shrink2(self, amount: Vec2) -> Self {
        Self {
            min: self.min + amount,
            max: self.max - amount,
        }
    }

    pub fn expand(self, amount: f32) -> Self {
        self.shrink(-amount)
    }

    pub fn with_height(self, height: f32) -> Self {
        Self {
            min: self.min,
            max: Vec2::new(self.max.x, self.min.y + height),
        }
    }

    pub fn with_width(self, width: f32) -> Self {
        Self {
            min: self.min,
            max: Vec2::new(self.min.x + width, self.max.y),
        }
    }

    pub fn clamp_to(self, bounds: Self) -> Self {
        let size = self.size().min(bounds.size());
        let min = self.min.max(bounds.min).min(bounds.max - size);
        Self {
            min,
            max: min + size,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Edges {
    pub const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    pub fn all(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            top: vertical,
            right: horizontal,
            bottom: vertical,
        }
    }

    pub fn horizontal(self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(self) -> f32 {
        self.top + self.bottom
    }

    pub fn size(self) -> Vec2 {
        Vec2::new(self.horizontal(), self.vertical())
    }

    pub fn shrink(self, rect: Rect) -> Rect {
        Rect {
            min: rect.min + Vec2::new(self.left, self.top),
            max: rect.max - Vec2::new(self.right, self.bottom),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Corners {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl Corners {
    pub const ZERO: Self = Self::all(0.0);

    pub const CAPSULE: f32 = 4096.0;

    pub const fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    pub const fn all(value: f32) -> Self {
        Self {
            top_left: value,
            top_right: value,
            bottom_right: value,
            bottom_left: value,
        }
    }

    pub const fn top(value: f32) -> Self {
        Self {
            top_left: value,
            top_right: value,
            bottom_right: 0.0,
            bottom_left: 0.0,
        }
    }

    pub const fn bottom(value: f32) -> Self {
        Self {
            top_left: 0.0,
            top_right: 0.0,
            bottom_right: value,
            bottom_left: value,
        }
    }

    pub const fn left(value: f32, inner: f32) -> Self {
        Self {
            top_left: value,
            top_right: inner,
            bottom_right: inner,
            bottom_left: value,
        }
    }

    pub const fn right(value: f32, inner: f32) -> Self {
        Self {
            top_left: inner,
            top_right: value,
            bottom_right: value,
            bottom_left: inner,
        }
    }

    pub const fn capsule() -> Self {
        Self::all(Self::CAPSULE)
    }

    pub const fn split_leading(inner: f32) -> Self {
        Self::left(Self::CAPSULE, inner)
    }

    pub const fn split_trailing(inner: f32) -> Self {
        Self::right(Self::CAPSULE, inner)
    }

    pub fn group_vertical(
        index: usize,
        count: usize,
        outer: f32,
        inner: f32,
        active: bool,
    ) -> Self {
        if active || count <= 1 {
            return Self::all(outer);
        }
        if index == 0 {
            Self::new(outer, outer, inner, inner)
        } else if index + 1 >= count {
            Self::new(inner, inner, outer, outer)
        } else {
            Self::all(inner)
        }
    }

    pub fn group_horizontal(
        index: usize,
        count: usize,
        outer: f32,
        inner: f32,
        active: bool,
    ) -> Self {
        if active || count <= 1 {
            return Self::all(outer);
        }
        if index == 0 {
            Self::left(outer, inner)
        } else if index + 1 >= count {
            Self::right(outer, inner)
        } else {
            Self::all(inner)
        }
    }

    pub fn resolved(self, size: Vec2) -> Self {
        let limit = 0.5 * size.min_element();
        Self {
            top_left: self.top_left.min(limit),
            top_right: self.top_right.min(limit),
            bottom_right: self.bottom_right.min(limit),
            bottom_left: self.bottom_left.min(limit),
        }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            top_left: self.top_left + (other.top_left - self.top_left) * t,
            top_right: self.top_right + (other.top_right - self.top_right) * t,
            bottom_right: self.bottom_right + (other.bottom_right - self.bottom_right) * t,
            bottom_left: self.bottom_left + (other.bottom_left - self.bottom_left) * t,
        }
    }
}
