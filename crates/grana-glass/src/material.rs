#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLUE: Self = Self::hex(0x0088FF);

    pub const GREEN: Self = Self::hex(0x34C759);

    pub const RED: Self = Self::hex(0xFF383C);

    pub const YELLOW: Self = Self::hex(0xFFCC00);

    pub const CYAN: Self = Self::hex(0x00C0E8);

    pub const WHITE: Self = Self::hex(0xFFFFFF);

    pub const fn hex(rgb: u32) -> Self {
        Self {
            r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
            g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
            b: (rgb & 0xFF) as f32 / 255.0,
            a: 1.0,
        }
    }

    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    pub(crate) fn to_oklab(self) -> [f32; 3] {
        let lin = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (lin(self.r), lin(self.g), lin(self.b));
        let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        [
            0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    pub lift: f32,
    pub luminance: f32,
    pub luminance_mix: f32,
    pub lighten: f32,
    pub darken_to: f32,
    pub darken: f32,
}

impl Tone {
    pub const NONE: Self = Self {
        lift: 0.0,
        luminance: 0.0,
        luminance_mix: 0.0,
        lighten: 0.0,
        darken_to: 1.0,
        darken: 0.0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub offset_y: f32,
    pub blur: f32,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub refraction: f32,
    pub depth: f32,
    pub dispersion: f32,
    pub frost: f32,
    pub splay: f32,
    pub specular: f32,
    pub rim: f32,
    pub rim_shade: f32,
    pub hairline: f32,
    pub edge: f32,
    pub shadow: Shadow,
    pub light_tone: Tone,
    pub dark_tone: Tone,
    pub adaptive: bool,
    pub tint: Option<Color>,
    pub dim: f32,
}

impl Material {
    pub const fn regular() -> Self {
        Self {
            refraction: 0.7,
            depth: 30.0,
            dispersion: 0.2,
            frost: 10.0,
            splay: 0.2,
            specular: 0.3,
            rim: 0.157,
            rim_shade: 0.05,
            hairline: 0.0,
            edge: 0.2,
            shadow: Shadow {
                offset_y: 8.0,
                blur: 15.0,
                opacity: 0.06,
            },
            light_tone: Tone {
                lift: 0.0,
                luminance: 0.92,
                luminance_mix: 0.3,
                lighten: 0.2,
                darken_to: 0.75,
                darken: 0.1,
            },
            dark_tone: Tone {
                lift: 0.03,
                luminance: 0.25,
                luminance_mix: 0.2,
                lighten: 0.0,
                darken_to: 0.0,
                darken: 0.25,
            },
            adaptive: true,
            tint: None,
            dim: 0.0,
        }
    }

    pub const fn clear() -> Self {
        let tone = Tone {
            lift: 0.063,
            luminance: 1.0,
            luminance_mix: 0.04,
            lighten: 0.0,
            darken_to: 1.0,
            darken: 0.0,
        };
        Self {
            refraction: 0.7,
            depth: 30.0,
            dispersion: 0.2,
            frost: 6.0,
            splay: 0.2,
            specular: 0.4,
            rim: 0.157,
            rim_shade: 0.098,
            hairline: 0.0,
            edge: 0.2,
            shadow: Shadow {
                offset_y: 8.0,
                blur: 15.0,
                opacity: 0.02,
            },
            light_tone: tone,
            dark_tone: tone,
            adaptive: false,
            tint: None,
            dim: 0.0,
        }
    }

    pub const fn clear_bar() -> Self {
        let tone = Tone {
            lift: 0.0,
            luminance: 0.6,
            luminance_mix: 0.33,
            lighten: 0.0,
            darken_to: 1.0,
            darken: 0.0,
        };
        Self {
            frost: 3.0,
            specular: 0.2,
            rim_shade: 0.0,
            hairline: 0.157,
            light_tone: tone,
            dark_tone: tone,
            ..Self::clear()
        }
    }

    pub const fn tinted(color: Color) -> Self {
        Self {
            tint: Some(color),
            ..Self::regular()
        }
    }

    pub const fn identity() -> Self {
        Self {
            refraction: 0.0,
            depth: 1.0,
            dispersion: 0.0,
            frost: 0.0,
            splay: 0.0,
            specular: 0.0,
            rim: 0.0,
            rim_shade: 0.0,
            hairline: 0.0,
            edge: 0.0,
            shadow: Shadow {
                offset_y: 0.0,
                blur: 0.0,
                opacity: 0.0,
            },
            light_tone: Tone::NONE,
            dark_tone: Tone::NONE,
            adaptive: false,
            tint: None,
            dim: 0.0,
        }
    }

    pub const fn tint(self, color: Color) -> Self {
        Self {
            tint: Some(color),
            ..self
        }
    }

    pub(crate) fn is_identity(&self) -> bool {
        *self == Self::identity()
    }

    pub(crate) fn lerp(&self, to: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);

        let f = |a: f32, b: f32| a * (1.0 - t) + b * t;
        let tone = |a: Tone, b: Tone| Tone {
            lift: f(a.lift, b.lift),
            luminance: f(a.luminance, b.luminance),
            luminance_mix: f(a.luminance_mix, b.luminance_mix),
            lighten: f(a.lighten, b.lighten),
            darken_to: f(a.darken_to, b.darken_to),
            darken: f(a.darken, b.darken),
        };
        let tint = match (self.tint, to.tint) {
            (None, None) => None,
            (Some(a), None) => Some(a.with_alpha(a.a * (1.0 - t))),
            (None, Some(b)) => Some(b.with_alpha(b.a * t)),
            (Some(a), Some(b)) => Some(Color {
                r: f(a.r, b.r),
                g: f(a.g, b.g),
                b: f(a.b, b.b),
                a: f(a.a, b.a),
            }),
        };
        Self {
            refraction: f(self.refraction, to.refraction),
            depth: f(self.depth, to.depth),
            dispersion: f(self.dispersion, to.dispersion),
            frost: f(self.frost, to.frost),
            splay: f(self.splay, to.splay),
            specular: f(self.specular, to.specular),
            rim: f(self.rim, to.rim),
            rim_shade: f(self.rim_shade, to.rim_shade),
            hairline: f(self.hairline, to.hairline),
            edge: f(self.edge, to.edge),
            shadow: Shadow {
                offset_y: f(self.shadow.offset_y, to.shadow.offset_y),
                blur: f(self.shadow.blur, to.shadow.blur),
                opacity: f(self.shadow.opacity, to.shadow.opacity),
            },
            light_tone: tone(self.light_tone, to.light_tone),
            dark_tone: tone(self.dark_tone, to.dark_tone),
            adaptive: if t < 0.5 { self.adaptive } else { to.adaptive },
            tint,
            dim: f(self.dim, to.dim),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Appearance {
    pub dark: bool,
    pub transparency: f32,
    pub reduce_transparency: bool,
    pub increase_contrast: bool,
    pub reduce_motion: bool,
    pub adapt_to_content: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            dark: false,
            transparency: 0.5,
            reduce_transparency: false,
            increase_contrast: false,
            reduce_motion: false,
            adapt_to_content: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Resolved {
    pub material: Material,
    pub adapt: f32,
    pub contrast: f32,
}

pub(crate) fn thickness(min_side: f32) -> f32 {
    smoothstep(44.0, 320.0, min_side)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn more_opaque(amount: f32, k: f32) -> f32 {
    if k >= 0.0 {
        amount + (1.0 - amount) * k
    } else {
        amount * (1.0 + k)
    }
}

pub(crate) const NEAR_BLACK: f32 = 0x1C as f32 / 255.0;

const TINTED_DARKEN: f32 = 0.6;
const TINTED_LIGHTEN: f32 = 0.6;

const TINTED_MAX: f32 = 0.88;

fn tint_faces(m: &mut Material, k: f32) {
    let dark = &mut m.dark_tone;

    dark.darken_to += (NEAR_BLACK - dark.darken_to) * k;
    dark.darken = more_opaque(dark.darken, TINTED_DARKEN * k).min(TINTED_MAX.max(dark.darken));
    let light = &mut m.light_tone;
    light.lighten =
        more_opaque(light.lighten, TINTED_LIGHTEN * k).min(TINTED_MAX.max(light.lighten));
}

impl Material {
    pub(crate) fn resolve(&self, min_side: f32, appearance: &Appearance) -> Resolved {
        let mut m = *self;
        let mut adapt = 0.0;
        if m.adaptive {
            let tau = thickness(min_side);
            m.frost *= 1.0 + 0.6 * tau;
            m.depth *= 1.0 + 0.25 * tau;
            m.specular -= 0.05 * tau;
            m.shadow.blur += 33.0 * tau;
            m.shadow.opacity += 0.19 * tau;

            m.light_tone.lighten += 0.55 * tau;
            m.light_tone.luminance_mix += 0.15 * tau;
            m.dark_tone.darken += 0.27 * tau;
            if appearance.adapt_to_content {
                adapt = 1.0 - smoothstep(0.0, 0.5, tau);
            }
        }

        let k = (appearance.transparency.clamp(0.0, 1.0) - 0.5) * 2.0;
        if appearance.reduce_transparency {
            m.frost = (m.frost * 3.0).max(20.0) * 1.1f32.exp2();
            tint_faces(&mut m, 1.0);
        } else if k > 0.0 {
            m.frost *= (0.5 * k).exp2();
            tint_faces(&mut m, k);
            m.edge = (m.edge + 0.25 * k).min(1.0);
            m.rim += 0.08 * k;
            m.specular += 0.08 * k;
        } else {
            m.frost *= (1.1 * k).exp2();
            for tone in [&mut m.light_tone, &mut m.dark_tone] {
                tone.luminance_mix = more_opaque(tone.luminance_mix, 0.45 * k);
                tone.lighten = more_opaque(tone.lighten, 0.35 * k);
                tone.darken = more_opaque(tone.darken, 0.45 * k);
            }
        }

        let contrast = if appearance.increase_contrast {
            m.edge = (m.edge * 2.5).min(1.0);
            1.0
        } else {
            0.0
        };
        Resolved {
            material: m,
            adapt,
            contrast,
        }
    }
}
