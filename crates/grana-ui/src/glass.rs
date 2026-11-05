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
    pub fn bar() -> Self {
        Self {
            material: Material::regular(),
            shape: Shape::Rounded(22.0),
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn panel() -> Self {
        Self {
            material: Material::regular(),
            shape: Shape::Rounded(26.0),
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn sheet() -> Self {
        Self {
            material: Material::regular(),
            shape: Shape::Rounded(30.0),
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn transport() -> Self {
        Self {
            material: Material::regular(),
            shape: Shape::Rounded(27.0),
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn status_pill() -> Self {
        Self {
            material: Material::regular(),
            shape: Shape::Capsule,
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn media_control() -> Self {
        Self {
            material: Material::clear(),
            shape: Shape::Capsule,
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn popover() -> Self {
        Self {
            material: Material::regular(),
            shape: Shape::Rounded(20.0),
            layer: FLOATING,
            interactive: false,
        }
    }

    pub fn tooltip() -> Self {
        Self {
            material: Material::regular(),
            shape: Shape::Rounded(10.0),
            layer: HUD,
            interactive: false,
        }
    }

    pub fn toast() -> Self {
        Self {
            material: Material::regular(),
            shape: Shape::Rounded(18.0),
            layer: HUD,
            interactive: false,
        }
    }

    pub fn interactive(mut self) -> Self {
        self.interactive = true;
        self
    }
}
