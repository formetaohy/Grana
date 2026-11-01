use glam::Vec2;

use crate::color::Color;
use crate::geometry::Corners;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
    pub displacement: f32,
    pub aberration: f32,
    pub saturation: f32,
    pub blur: f32,
    pub frost: f32,
    pub tint: Color,
    pub rim: f32,
    pub refraction: f32,
    pub specular: f32,
}

impl Glass {
    pub const REFERENCE: Self = Self {
        displacement: 70.0,
        aberration: 2.0,
        saturation: 1.4,
        blur: 4.0,
        frost: 0.0,
        tint: Color::TRANSPARENT,
        rim: 1.0,
        refraction: 0.0,
        specular: 0.0,
    };

    pub const REGULAR: Self = Self {
        displacement: 72.0,
        aberration: 2.2,
        saturation: 1.36,
        blur: 10.0,
        frost: 1.0,
        tint: Color::TRANSPARENT,
        rim: 0.72,
        refraction: 0.85,
        specular: 0.55,
    };

    pub const CLEAR: Self = Self {
        displacement: 64.0,
        aberration: 1.8,
        saturation: 1.38,
        blur: 0.0,
        frost: 0.0,
        tint: Color::TRANSPARENT,
        rim: 0.95,
        refraction: 1.0,
        specular: 0.90,
    };

    pub fn tinted(self, tint: Color) -> Self {
        Self { tint, ..self }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            displacement: self.displacement + (other.displacement - self.displacement) * t,
            aberration: self.aberration + (other.aberration - self.aberration) * t,
            saturation: self.saturation + (other.saturation - self.saturation) * t,
            blur: self.blur + (other.blur - self.blur) * t,
            frost: self.frost + (other.frost - self.frost) * t,
            tint: self.tint.lerp(other.tint, t),
            rim: self.rim + (other.rim - self.rim) * t,
            refraction: self.refraction + (other.refraction - self.refraction) * t,
            specular: self.specular + (other.specular - self.specular) * t,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ambient {
    pub uv: [f32; 4],
    pub tint: Color,
    pub blur: f32,
    pub saturation: f32,
    pub brightness: f32,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    None,
    Solid(Color),
    Gradient(Color, Color),
    Glass(Glass),
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
            Self::Glass(glass) => glass.tint.alpha > 0.0 || glass.rim > 0.0,
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
            (Self::Glass(a), Self::Glass(b)) => Self::Glass(a.lerp(b, t)),
            (Self::Scene(a), Self::Scene(b)) => {
                let mut uv = [0.0; 4];
                for (index, value) in uv.iter_mut().enumerate() {
                    *value = a[index] + (b[index] - a[index]) * t;
                }
                Self::Scene(uv)
            }
            (Self::Scene(a), _) => Self::Scene(a),
            (_, Self::Scene(b)) => Self::Scene(b),
            (Self::Glass(glass), Self::Solid(b)) => Self::Solid(glass.tint.lerp(b, t)),
            (Self::Glass(glass), Self::Gradient(b0, _)) => Self::Solid(glass.tint.lerp(b0, t)),
            (current, Self::Glass(glass)) => match current {
                Self::Solid(color) => Self::Solid(color.lerp(glass.tint, t)),
                Self::Gradient(a, b) => {
                    Self::Gradient(a.lerp(glass.tint, t), b.lerp(glass.tint, t))
                }
                current => current,
            },
            (Self::None, other) => match other {
                Self::Solid(color) => Self::Solid(color.with_alpha(color.alpha * t)),
                Self::Gradient(a, b) => {
                    Self::Gradient(a.with_alpha(a.alpha * t), b.with_alpha(b.alpha * t))
                }
                Self::Glass(glass) => Self::Glass(Glass {
                    tint: glass.tint.with_alpha(glass.tint.alpha * t),
                    rim: glass.rim * t,
                    ..glass
                }),
                other => other,
            },
            (current, Self::None) => match current {
                Self::Solid(color) => Self::Solid(color.with_alpha(color.alpha * (1.0 - t))),
                Self::Gradient(a, b) => Self::Gradient(
                    a.with_alpha(a.alpha * (1.0 - t)),
                    b.with_alpha(b.alpha * (1.0 - t)),
                ),
                Self::Glass(glass) => Self::Glass(Glass {
                    tint: glass.tint.with_alpha(glass.tint.alpha * (1.0 - t)),
                    rim: glass.rim * (1.0 - t),
                    ..glass
                }),
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
    pub fn hairline(color: Color) -> Self {
        Self { width: 0.5, color }
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
pub struct Look {
    pub fill: Fill,
    pub corners: Corners,
    pub border: Border,
    pub shadow: Shadow,
    pub text: Color,
}

impl Look {
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            fill: self.fill.lerp(other.fill, t),
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
