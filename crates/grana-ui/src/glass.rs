use grana_glass::{Material, Shape};

pub const CHROME: i32 = 0;
pub const FLOATING: i32 = 1;
pub const HUD: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassStyle {
    pub material: Material,
    pub shape: Shape,
    pub layer: i32,
    pub interactive: bool,
}

impl GlassStyle {
    fn regular(shape: Shape, layer: i32) -> Self {
        Self {
            material: Material::regular(),
            shape,
            layer,
            interactive: false,
        }
    }

    fn clear(shape: Shape, layer: i32) -> Self {
        Self {
            material: Material::clear(),
            shape,
            layer,
            interactive: false,
        }
    }

    pub fn bar() -> Self {
        Self::regular(Shape::Rounded(24.0), CHROME)
    }

    pub fn panel() -> Self {
        Self::regular(Shape::Rounded(26.0), CHROME)
    }

    pub fn sheet() -> Self {
        Self::regular(Shape::Rounded(28.0), CHROME)
    }

    pub fn transport() -> Self {
        Self::regular(Shape::Capsule, CHROME)
    }

    pub fn status() -> Self {
        Self::regular(Shape::Capsule, CHROME)
    }

    pub fn popover() -> Self {
        Self::regular(Shape::Rounded(20.0), FLOATING)
    }

    pub fn tooltip() -> Self {
        Self::regular(Shape::Rounded(12.0), HUD)
    }

    pub fn toast() -> Self {
        Self::regular(Shape::Rounded(18.0), HUD)
    }

    pub fn stage_control() -> Self {
        Self::clear(Shape::Capsule, CHROME)
    }

    pub fn hud_chip() -> Self {
        Self::regular(Shape::Capsule, CHROME)
    }

    pub fn primary() -> Self {
        Self {
            material: Material::tinted(grana_glass::Color::hex(0x0A84FF)),
            shape: Shape::Capsule,
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn lens() -> Self {
        Self::clear(Shape::Capsule, FLOATING)
    }

    pub fn interactive(mut self) -> Self {
        self.interactive = true;
        self
    }
}
