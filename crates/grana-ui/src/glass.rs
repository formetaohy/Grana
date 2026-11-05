use grana_glass::{Color as GlassColor, Material, Shape};

pub use grana_glass::Shape as GlassShape;

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
            material: chrome(),
            shape: Shape::Rounded(22.0),
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn panel() -> Self {
        Self {
            material: chrome(),
            shape: Shape::Rounded(26.0),
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn sheet() -> Self {
        Self {
            material: chrome(),
            shape: Shape::Rounded(30.0),
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn floating() -> Self {
        Self {
            material: frosted(),
            shape: Shape::Rounded(20.0),
            layer: FLOATING,
            interactive: false,
        }
    }

    pub fn popover() -> Self {
        Self {
            material: frosted(),
            shape: Shape::Rounded(20.0),
            layer: FLOATING,
            interactive: false,
        }
    }

    pub fn capsule() -> Self {
        Self {
            material: Material::clear_bar(),
            shape: Shape::Capsule,
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn hud() -> Self {
        Self {
            material: Material::clear_bar(),
            shape: Shape::Rounded(18.0),
            layer: CHROME,
            interactive: false,
        }
    }

    pub fn tooltip() -> Self {
        Self {
            material: frosted(),
            shape: Shape::Rounded(10.0),
            layer: HUD,
            interactive: false,
        }
    }

    pub fn tinted(color: [f32; 3]) -> Self {
        Self {
            material: Material::tinted(GlassColor::hex(rgb(color))),
            shape: Shape::Capsule,
            layer: CHROME,
            interactive: true,
        }
    }

    pub fn with_shape(mut self, shape: Shape) -> Self {
        self.shape = shape;
        self
    }

    pub fn layer(mut self, layer: i32) -> Self {
        self.layer = layer;
        self
    }

    pub fn interactive(mut self) -> Self {
        self.interactive = true;
        self
    }
}

fn rgb(color: [f32; 3]) -> u32 {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
    (channel(color[0]) << 16) | (channel(color[1]) << 8) | channel(color[2])
}

fn chrome() -> Material {
    let mut material = Material::regular();
    material.frost = 14.0;
    material.dark_tone.darken = 0.36;
    material.dark_tone.luminance_mix = 0.26;
    material.light_tone.lighten = 0.32;
    material.shadow.opacity = 0.16;
    material.shadow.blur = 32.0;
    material
}

fn frosted() -> Material {
    let mut material = Material::regular();
    material.frost = 22.0;
    material.depth = 26.0;
    material.dark_tone.darken = 0.42;
    material.dark_tone.luminance_mix = 0.2;
    material.light_tone.lighten = 0.6;
    material.light_tone.luminance_mix = 0.42;
    material.shadow.opacity = 0.2;
    material.shadow.blur = 36.0;
    material.shadow.offset_y = 12.0;
    material.edge = 0.22;
    material
}
