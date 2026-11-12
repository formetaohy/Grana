use glam::Vec2;

use crate::color::Color;
use crate::geometry::Corners;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    None,
    Solid(Color),
    Gradient(Color, Color),
    Scene([f32; 4]),
}

impl Fill {
    pub fn solid(color: Color) -> Self {
        Self::Solid(color)
    }

    pub fn is_visible(self) -> bool {
        match self {
            Self::None => false,
            Self::Solid(color) | Self::Gradient(color, _) => color.alpha > 0.0,
            Self::Scene(_) => true,
        }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        match (self, other) {
            (Self::Solid(a), Self::Solid(b)) => Self::Solid(a.lerp(b, t)),
            (Self::Gradient(a0, a1), Self::Gradient(b0, b1)) => {
                Self::Gradient(a0.lerp(b0, t), a1.lerp(b1, t))
            }
            (Self::Solid(a), Self::Gradient(b0, _)) => Self::Solid(a.lerp(b0, t)),
            (Self::Gradient(a0, _), Self::Solid(b)) => Self::Solid(a0.lerp(b, t)),
            (Self::Scene(a), Self::Scene(b)) => {
                let mut uv = [0.0; 4];
                for (index, value) in uv.iter_mut().enumerate() {
                    *value = a[index] + (b[index] - a[index]) * t;
                }
                Self::Scene(uv)
            }
            (Self::Scene(a), _) => Self::Scene(a),
            (_, Self::Scene(b)) => Self::Scene(b),
            (Self::None, other) => match other {
                Self::Solid(color) => Self::Solid(color.with_alpha(color.alpha * t)),
                Self::Gradient(a, b) => {
                    Self::Gradient(a.with_alpha(a.alpha * t), b.with_alpha(b.alpha * t))
                }
                other => other,
            },
            (current, Self::None) => match current {
                Self::Solid(color) => Self::Solid(color.with_alpha(color.alpha * (1.0 - t))),
                Self::Gradient(a, b) => Self::Gradient(
                    a.with_alpha(a.alpha * (1.0 - t)),
                    b.with_alpha(b.alpha * (1.0 - t)),
                ),
                current => current,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: f32,
    pub color: Color,
}

impl Border {
    pub const NONE: Self = Self {
        width: 0.0,
        color: Color::TRANSPARENT,
    };

    pub fn hairline(color: Color) -> Self {
        Self { width: 1.0, color }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            width: self.width + (other.width - self.width) * t,
            color: self.color.lerp(other.color, t),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub color: Color,
    pub radius: f32,
    pub offset: Vec2,
}

impl Shadow {
    pub const NONE: Self = Self {
        color: Color::TRANSPARENT,
        radius: 0.0,
        offset: Vec2::ZERO,
    };

    pub const fn new(color: Color, radius: f32, offset_y: f32) -> Self {
        Self {
            color,
            radius,
            offset: Vec2::new(0.0, offset_y),
        }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            color: self.color.lerp(other.color, t),
            radius: self.radius + (other.radius - self.radius) * t,
            offset: self.offset + (other.offset - self.offset) * t,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub fill: Fill,
    pub backdrop: f32,
    pub corners: Corners,
    pub border: Border,
    pub shadow: Shadow,
    pub text: Color,
}

impl Surface {
    pub const NONE: Self = Self {
        fill: Fill::None,
        backdrop: 0.0,
        corners: Corners::ZERO,
        border: Border::NONE,
        shadow: Shadow::NONE,
        text: Color::WHITE,
    };

    pub const fn with_backdrop(mut self, backdrop: f32) -> Self {
        self.backdrop = backdrop;
        self
    }

    pub const fn with_corners(mut self, corners: Corners) -> Self {
        self.corners = corners;
        self
    }

    pub const fn with_shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = shadow;
        self
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            fill: self.fill.lerp(other.fill, t),
            backdrop: self.backdrop + (other.backdrop - self.backdrop) * t,
            corners: self.corners.lerp(other.corners, t),
            border: self.border.lerp(other.border, t),
            shadow: self.shadow.lerp(other.shadow, t),
            text: self.text.lerp(other.text, t),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Rest,
    Hover,
    Pressed,
    Selected,
    Disabled,
}
