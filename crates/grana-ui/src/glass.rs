use grana_glass::{Material, Shadow, Shape, Tone};

pub const CHROME: i32 = 0;
pub const OVERLAY: i32 = 1;
pub const MENU: i32 = 2;
pub const HUD: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassStyle {
    pub material: Material,
    pub shape: Shape,
    pub layer: i32,
    pub interactive: bool,
    pub lens: bool,
}

impl GlassStyle {
    fn regular(shape: Shape, layer: i32) -> Self {
        Self {
            material: Material::regular(),
            shape,
            layer,
            interactive: false,
            lens: false,
        }
    }

    fn clear(shape: Shape, layer: i32) -> Self {
        Self {
            material: Material::clear(),
            shape,
            layer,
            interactive: false,
            lens: false,
        }
    }

    pub fn bar() -> Self {
        Self::regular(Shape::Rounded(24.0), CHROME)
    }

    pub fn button() -> Self {
        Self::regular(Shape::Rounded(12.0), CHROME)
    }

    pub fn button_disabled() -> Self {
        let mut style = Self::button();
        style.material.dim = 0.45;
        style
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
        Self::regular(Shape::Rounded(20.0), MENU)
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
            lens: false,
        }
    }

    pub fn primary_active() -> Self {
        let mut style = Self::primary();
        style.material = Material::tinted(grana_glass::Color::hex(0x3D9BFF));
        style
    }

    pub fn lens() -> Self {
        Self {
            material: clear_lens(),
            shape: Shape::Capsule,
            layer: OVERLAY,
            interactive: false,
            lens: false,
        }
    }

    pub fn tab_lens() -> Self {
        Self {
            material: clear_lens(),
            shape: Shape::Capsule,
            layer: OVERLAY,
            interactive: false,
            lens: false,
        }
    }

    pub fn knob() -> Self {
        let mut material = Material::clear();
        material.refraction = 0.6;
        material.depth = 12.0;
        material.dispersion = 0.06;
        material.frost = 2.2;
        material.splay = 0.35;
        material.specular = 1.05;
        material.rim = 0.4;
        material.rim_shade = 0.1;
        material.edge = 0.3;
        material.hairline = 0.16;
        material.shadow = Shadow {
            offset_y: 3.0,
            blur: 6.0,
            opacity: 0.22,
        };
        let tone = Tone {
            lift: 0.11,
            luminance: 1.0,
            luminance_mix: 0.22,
            lighten: 0.1,
            darken_to: 1.0,
            darken: 0.0,
        };
        material.light_tone = tone;
        material.dark_tone = tone;
        Self {
            material,
            shape: Shape::Capsule,
            layer: OVERLAY,
            interactive: false,
            lens: false,
        }
    }

    pub fn interactive(mut self) -> Self {
        self.interactive = true;
        self
    }

    pub fn draggable(mut self) -> Self {
        self.lens = true;
        self
    }
}

fn clear_lens() -> Material {
    let mut material = Material::clear();
    material.refraction = 0.9;
    material.depth = 18.0;
    material.dispersion = 0.1;
    material.frost = 0.8;
    material.splay = 0.5;
    material.specular = 0.6;
    material.rim = 0.5;
    material.rim_shade = 0.06;
    material.edge = 0.32;
    material.hairline = 0.14;
    material.shadow = Shadow {
        offset_y: 2.0,
        blur: 6.0,
        opacity: 0.18,
    };
    let tone = Tone {
        lift: 0.06,
        luminance: 1.0,
        luminance_mix: 0.08,
        lighten: 0.12,
        darken_to: 1.0,
        darken: 0.0,
    };
    material.light_tone = tone;
    material.dark_tone = tone;
    material
}
