use crate::color::Color;
use crate::geometry::Corners;
use crate::style::{Border, Fill, Shadow, State, Surface};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Card,
    Control,
    Prominent,
    Quiet,
    Tinted,
    Field,
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    LargeTitle,
    Title,
    Headline,
    Body,
    Callout,
    Caption,
    Micro,
    Monospace,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeStyle {
    pub size: f32,
    pub weight: f32,
    pub tracking: f32,
    pub leading: f32,
}

impl TypeStyle {
    pub const fn new(size: f32, weight: f32, leading: f32) -> Self {
        Self {
            size,
            weight,
            tracking: 0.0,
            leading,
        }
    }

    pub const fn medium(self) -> Self {
        Self {
            weight: 500.0,
            ..self
        }
    }

    pub const fn semibold(self) -> Self {
        Self {
            weight: 600.0,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Follow System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Self::System => system_dark,
            Self::Light => false,
            Self::Dark => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub text_quaternary: Color,
    pub text_on_accent: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_press: Color,
    pub brand: Color,
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
    pub separator: Color,
    pub border: Color,
    pub selected: Color,
    pub card: Color,
    pub field: Color,
    pub focus: Color,
    pub scrim: Color,
    pub scene_backdrop: Color,
    pub material_fill: Color,
    pub material_hover: Color,
    pub material_press: Color,
    pub material_border: Color,
    pub hud_fill: Color,
    pub hud_border: Color,
    pub track: Color,
    pub shade: Color,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            text_primary: Color::white_alpha(0.95),
            text_secondary: Color::white_alpha(0.68),
            text_tertiary: Color::white_alpha(0.45),
            text_quaternary: Color::white_alpha(0.26),
            text_on_accent: Color::bytes(255, 255, 255),
            accent: Color::bytes(10, 132, 255),
            accent_hover: Color::bytes(64, 156, 255),
            accent_press: Color::bytes(0, 112, 224),
            brand: Color::bytes(214, 232, 92),
            danger: Color::bytes(255, 69, 58),
            warning: Color::bytes(255, 159, 10),
            success: Color::bytes(48, 209, 88),
            separator: Color::white_alpha(0.09),
            border: Color::white_alpha(0.14),
            selected: Color::white_alpha(0.17),
            card: Color::white_alpha(0.055),
            field: Color::black_alpha(0.24),
            focus: Color::bytes(10, 132, 255),
            scrim: Color::rgba(0.0, 0.0, 0.0, 0.55),
            scene_backdrop: Color::bytes(6, 6, 8),
            material_fill: Color::white_alpha(0.10),
            material_hover: Color::white_alpha(0.16),
            material_press: Color::white_alpha(0.24),
            material_border: Color::white_alpha(0.13),
            hud_fill: Color::rgba(0.03, 0.03, 0.04, 0.66),
            hud_border: Color::white_alpha(0.14),
            track: Color::white_alpha(0.18),
            shade: Color::black_alpha(0.30),
        }
    }

    pub fn light() -> Self {
        Self {
            text_primary: Color::black_alpha(0.92),
            text_secondary: Color::black_alpha(0.62),
            text_tertiary: Color::black_alpha(0.42),
            text_quaternary: Color::black_alpha(0.24),
            text_on_accent: Color::bytes(255, 255, 255),
            accent: Color::bytes(0, 122, 255),
            accent_hover: Color::bytes(51, 149, 255),
            accent_press: Color::bytes(0, 100, 214),
            brand: Color::bytes(121, 138, 22),
            danger: Color::bytes(255, 59, 48),
            warning: Color::bytes(255, 149, 0),
            success: Color::bytes(52, 199, 89),
            separator: Color::black_alpha(0.10),
            border: Color::black_alpha(0.12),
            selected: Color::black_alpha(0.12),
            card: Color::white_alpha(0.46),
            field: Color::white_alpha(0.60),
            focus: Color::bytes(0, 122, 255),
            scrim: Color::rgba(0.0, 0.0, 0.0, 0.35),
            scene_backdrop: Color::bytes(238, 238, 240),
            material_fill: Color::black_alpha(0.07),
            material_hover: Color::black_alpha(0.11),
            material_press: Color::black_alpha(0.17),
            material_border: Color::black_alpha(0.10),
            hud_fill: Color::white_alpha(0.72),
            hud_border: Color::black_alpha(0.10),
            track: Color::black_alpha(0.14),
            shade: Color::black_alpha(0.14),
        }
    }

    pub fn contrasted(self) -> Self {
        let lift = |color: Color| color.with_alpha((color.alpha * 1.3 + 0.08).min(1.0));
        Self {
            text_primary: lift(self.text_primary),
            text_secondary: lift(self.text_secondary),
            text_tertiary: lift(self.text_tertiary),
            text_quaternary: lift(self.text_quaternary),
            separator: lift(self.separator),
            border: lift(self.border),
            material_border: lift(self.material_border),
            hud_border: lift(self.hud_border),
            track: lift(self.track),
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub control_height: f32,
    pub row_height: f32,
    pub gap: f32,
    pub radius: f32,
    pub card_radius: f32,
    pub hairline: f32,
    pub track: f32,
    pub thumb: f32,
    pub switch_width: f32,
    pub switch_height: f32,
}

impl Metrics {
    pub const fn standard() -> Self {
        Self {
            control_height: 28.0,
            row_height: 30.0,
            gap: 8.0,
            radius: 12.0,
            card_radius: 16.0,
            hairline: 1.0,
            track: 4.0,
            thumb: 20.0,
            switch_width: 46.0,
            switch_height: 26.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Typography {
    pub large_title: TypeStyle,
    pub title: TypeStyle,
    pub headline: TypeStyle,
    pub body: TypeStyle,
    pub callout: TypeStyle,
    pub caption: TypeStyle,
    pub micro: TypeStyle,
    pub monospace: TypeStyle,
}

impl Typography {
    pub const fn standard() -> Self {
        Self {
            large_title: TypeStyle::new(24.0, 700.0, 30.0),
            title: TypeStyle::new(17.0, 600.0, 22.0),
            headline: TypeStyle::new(15.0, 600.0, 20.0),
            body: TypeStyle::new(13.0, 400.0, 18.0),
            callout: TypeStyle::new(12.0, 400.0, 16.0),
            caption: TypeStyle::new(11.0, 400.0, 14.0),
            micro: TypeStyle {
                size: 10.0,
                weight: 500.0,
                tracking: 0.2,
                leading: 13.0,
            },
            monospace: TypeStyle::new(11.0, 400.0, 15.0),
        }
    }

    pub fn style(&self, level: Level) -> TypeStyle {
        match level {
            Level::LargeTitle => self.large_title,
            Level::Title => self.title,
            Level::Headline => self.headline,
            Level::Body => self.body,
            Level::Callout => self.callout,
            Level::Caption => self.caption,
            Level::Micro => self.micro,
            Level::Monospace => self.monospace,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub palette: Palette,
    pub typography: Typography,
    pub metrics: Metrics,
    pub dark: bool,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            palette: Palette::dark(),
            typography: Typography::standard(),
            metrics: Metrics::standard(),
            dark: true,
        }
    }

    pub fn light() -> Self {
        Self {
            palette: Palette::light(),
            typography: Typography::standard(),
            metrics: Metrics::standard(),
            dark: false,
        }
    }

    pub fn with_contrast(self, contrast: bool) -> Self {
        if contrast {
            Self {
                palette: self.palette.contrasted(),
                ..self
            }
        } else {
            self
        }
    }

    pub fn text(&self, level: Level) -> Color {
        match level {
            Level::LargeTitle | Level::Title | Level::Headline | Level::Body => {
                self.palette.text_primary
            }
            Level::Callout | Level::Monospace => self.palette.text_secondary,
            Level::Caption | Level::Micro => self.palette.text_tertiary,
        }
    }

    pub fn font(&self, level: Level) -> TypeStyle {
        self.typography.style(level)
    }

    pub fn button_font(&self) -> TypeStyle {
        self.typography.body.medium()
    }

    pub fn label_font(&self) -> TypeStyle {
        self.typography.caption.medium()
    }

    pub fn surface(&self, role: Role, state: State) -> Surface {
        let palette = self.palette;
        let metrics = self.metrics;
        let disabled = state == State::Disabled;
        let hovered = state == State::Hover;
        let pressed = state == State::Pressed;
        let interactive = hovered || pressed;
        let tint = if disabled {
            Color::TRANSPARENT
        } else if pressed {
            palette.material_press
        } else if hovered {
            palette.material_hover
        } else {
            palette.material_fill
        };
        let text = if disabled {
            palette.text_quaternary
        } else {
            palette.text_primary
        };
        match role {
            Role::Card => Surface {
                fill: Fill::Solid(palette.card),
                corners: Corners::all(metrics.card_radius),
                border: Border {
                    width: metrics.hairline,
                    color: palette.material_border,
                },
                shadow: Shadow::NONE,
                text,
            },
            Role::Control => Surface {
                fill: Fill::Gradient(
                    if disabled { Color::TRANSPARENT } else { tint },
                    if disabled {
                        Color::TRANSPARENT
                    } else {
                        tint.with_alpha(tint.alpha * 0.72)
                    },
                ),
                corners: Corners::capsule(),
                border: Border {
                    width: metrics.hairline,
                    color: if disabled {
                        Color::TRANSPARENT
                    } else {
                        palette.material_border
                    },
                },
                shadow: Shadow::NONE,
                text,
            },
            Role::Prominent => Surface {
                fill: Fill::Gradient(
                    if disabled {
                        palette.accent.with_alpha(0.3)
                    } else if pressed {
                        palette.accent_press
                    } else if hovered {
                        palette.accent_hover
                    } else {
                        palette.accent
                    },
                    if disabled {
                        palette.accent.with_alpha(0.24)
                    } else if pressed {
                        palette.accent_press
                    } else {
                        palette.accent.with_alpha(0.88)
                    },
                ),
                corners: Corners::capsule(),
                border: Border {
                    width: metrics.hairline,
                    color: Color::white_alpha(0.22),
                },
                shadow: Shadow::new(Color::black_alpha(0.22), 10.0, 3.0),
                text: if disabled {
                    palette.text_on_accent.with_alpha(0.5)
                } else {
                    palette.text_on_accent
                },
            },
            Role::Quiet => Surface {
                fill: Fill::Solid(if disabled || !interactive {
                    Color::TRANSPARENT
                } else {
                    tint
                }),
                corners: Corners::capsule(),
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    palette.text_quaternary
                } else {
                    palette
                        .text_secondary
                        .lerp(palette.text_primary, f32::from(hovered || pressed))
                },
            },
            Role::Tinted => Surface {
                fill: Fill::Gradient(
                    if disabled {
                        palette.accent.with_alpha(0.12)
                    } else if pressed {
                        palette.accent.with_alpha(0.36)
                    } else if hovered {
                        palette.accent.with_alpha(0.28)
                    } else {
                        palette.accent.with_alpha(0.20)
                    },
                    if disabled {
                        palette.accent.with_alpha(0.10)
                    } else {
                        palette.accent.with_alpha(0.14)
                    },
                ),
                corners: Corners::capsule(),
                border: Border {
                    width: metrics.hairline,
                    color: palette.accent.with_alpha(if pressed { 0.5 } else { 0.28 }),
                },
                shadow: Shadow::NONE,
                text: palette.text_primary,
            },
            Role::Field => Surface {
                fill: Fill::Gradient(
                    if disabled {
                        palette.field.with_alpha(0.4)
                    } else {
                        palette.field
                    },
                    if disabled {
                        palette.field.with_alpha(0.3)
                    } else {
                        palette.field.with_alpha(palette.field.alpha * 0.86)
                    },
                ),
                corners: Corners::all(10.0),
                border: Border {
                    width: metrics.hairline,
                    color: if interactive && !disabled {
                        palette.border.with_alpha(0.26)
                    } else {
                        palette.material_border
                    },
                },
                shadow: Shadow::NONE,
                text,
            },
            Role::Danger => Surface {
                fill: Fill::Solid(if pressed {
                    palette.danger.with_alpha(0.26)
                } else if hovered {
                    palette.danger.with_alpha(0.16)
                } else {
                    Color::TRANSPARENT
                }),
                corners: Corners::capsule(),
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    palette.text_quaternary
                } else {
                    palette.danger
                },
            },
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
