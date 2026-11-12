use crate::color::Color;
use crate::geometry::Corners;
use crate::style::{Border, Fill, Shadow, State, Surface};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Panel,
    Hud,
    Popover,
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
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
    pub separator: Color,
    pub border: Color,
    pub selected: Color,
    pub panel: Color,
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
    pub popover_fill: Color,
    pub track: Color,
    pub shade: Color,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            text_primary: Color::white_alpha(0.94),
            text_secondary: Color::white_alpha(0.72),
            text_tertiary: Color::white_alpha(0.50),
            text_quaternary: Color::white_alpha(0.28),
            text_on_accent: Color::bytes(255, 255, 255),
            accent: Color::bytes(10, 132, 255),
            accent_hover: Color::bytes(56, 152, 255),
            accent_press: Color::bytes(0, 110, 224),
            danger: Color::bytes(255, 69, 58),
            warning: Color::bytes(255, 159, 10),
            success: Color::bytes(48, 209, 88),
            separator: Color::white_alpha(0.08),
            border: Color::white_alpha(0.12),
            selected: Color::white_alpha(0.14),
            panel: Color::rgba(0.075, 0.082, 0.098, 0.98),
            card: Color::rgba(0.110, 0.118, 0.141, 0.96),
            field: Color::rgba(0.047, 0.051, 0.063, 0.96),
            focus: Color::bytes(10, 132, 255),
            scrim: Color::rgba(0.0, 0.0, 0.0, 0.58),
            scene_backdrop: Color::bytes(7, 8, 10),
            material_fill: Color::white_alpha(0.09),
            material_hover: Color::white_alpha(0.15),
            material_press: Color::white_alpha(0.22),
            material_border: Color::white_alpha(0.10),
            hud_fill: Color::rgba(0.082, 0.090, 0.110, 0.78),
            hud_border: Color::white_alpha(0.14),
            popover_fill: Color::rgba(0.094, 0.102, 0.125, 0.90),
            track: Color::white_alpha(0.16),
            shade: Color::black_alpha(0.42),
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

    pub fn opaque(self) -> Self {
        Self {
            panel: self.panel.with_alpha(1.0),
            card: self.card.with_alpha(1.0),
            field: self.field.with_alpha(1.0),
            hud_fill: self.hud_fill.with_alpha(0.98),
            popover_fill: self.popover_fill.with_alpha(0.99),
            ..self
        }
    }

    pub fn series(self) -> [Color; 6] {
        [
            self.accent,
            Color::bytes(48, 209, 88),
            Color::bytes(255, 159, 10),
            Color::bytes(255, 69, 58),
            Color::bytes(191, 90, 242),
            Color::bytes(90, 200, 250),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub control_height: f32,
    pub row_height: f32,
    pub gap: f32,
    pub radius: f32,
    pub card_radius: f32,
    pub panel_radius: f32,
    pub hairline: f32,
    pub track: f32,
    pub thumb: f32,
    pub switch_width: f32,
    pub switch_height: f32,
}

impl Metrics {
    pub const fn standard() -> Self {
        Self {
            control_height: 26.0,
            row_height: 28.0,
            gap: 6.0,
            radius: 6.0,
            card_radius: 8.0,
            panel_radius: 10.0,
            hairline: 1.0,
            track: 4.0,
            thumb: 14.0,
            switch_width: 34.0,
            switch_height: 18.0,
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
            large_title: TypeStyle::new(22.0, 700.0, 28.0),
            title: TypeStyle::new(16.0, 600.0, 21.0),
            headline: TypeStyle::new(14.0, 600.0, 18.0),
            body: TypeStyle::new(12.5, 400.0, 17.0),
            callout: TypeStyle::new(11.5, 400.0, 15.0),
            caption: TypeStyle::new(10.5, 400.0, 14.0),
            micro: TypeStyle {
                size: 9.5,
                weight: 500.0,
                tracking: 0.25,
                leading: 12.0,
            },
            monospace: TypeStyle::new(11.0, 400.0, 14.0),
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
    pub reduce_transparency: bool,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            palette: Palette::dark(),
            typography: Typography::standard(),
            metrics: Metrics::standard(),
            reduce_transparency: false,
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

    pub fn with_transparency(self, reduce_transparency: bool) -> Self {
        if reduce_transparency {
            Self {
                palette: self.palette.opaque(),
                reduce_transparency: true,
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
        self.typography.callout.medium()
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
        let selected = state == State::Selected;
        let interactive = hovered || pressed || selected;
        let tint = if disabled {
            palette.material_fill.with_alpha(0.04)
        } else if pressed {
            palette.material_press
        } else if hovered || selected {
            palette.material_hover
        } else {
            palette.material_fill
        };
        let text = if disabled {
            palette.text_quaternary
        } else {
            palette.text_primary
        };
        let blur = |radius: f32| {
            if self.reduce_transparency {
                0.0
            } else {
                radius
            }
        };
        match role {
            Role::Panel => Surface {
                fill: Fill::Solid(palette.panel),
                backdrop: 0.0,
                corners: Corners::all(metrics.panel_radius),
                border: Border {
                    width: metrics.hairline,
                    color: palette.border,
                },
                shadow: Shadow::new(Color::black_alpha(0.46), 18.0, 5.0),
                text,
            },
            Role::Hud => Surface {
                fill: Fill::Solid(palette.hud_fill),
                backdrop: blur(16.0),
                corners: Corners::all(metrics.panel_radius),
                border: Border {
                    width: metrics.hairline,
                    color: palette.hud_border,
                },
                shadow: Shadow::new(Color::black_alpha(0.38), 14.0, 4.0),
                text,
            },
            Role::Popover => Surface {
                fill: Fill::Solid(palette.popover_fill),
                backdrop: blur(22.0),
                corners: Corners::all(metrics.panel_radius),
                border: Border {
                    width: metrics.hairline,
                    color: palette.hud_border,
                },
                shadow: Shadow::new(Color::black_alpha(0.54), 22.0, 7.0),
                text,
            },
            Role::Card => Surface {
                fill: Fill::Solid(palette.card),
                backdrop: 0.0,
                corners: Corners::all(metrics.card_radius),
                border: Border {
                    width: metrics.hairline,
                    color: palette.material_border,
                },
                shadow: Shadow::NONE,
                text,
            },
            Role::Control => Surface {
                fill: Fill::Gradient(tint, tint.with_alpha(tint.alpha * 0.82)),
                backdrop: 0.0,
                corners: Corners::all(metrics.radius),
                border: Border {
                    width: metrics.hairline,
                    color: if disabled {
                        palette.material_border.with_alpha(0.04)
                    } else if interactive {
                        palette.border
                    } else {
                        palette.material_border
                    },
                },
                shadow: if disabled {
                    Shadow::NONE
                } else {
                    Shadow::new(Color::black_alpha(0.18), 3.0, 1.0)
                },
                text,
            },
            Role::Prominent => Surface {
                fill: Fill::Gradient(
                    if disabled {
                        palette.accent.with_alpha(0.28)
                    } else if pressed {
                        palette.accent_press
                    } else if hovered {
                        palette.accent_hover
                    } else {
                        palette.accent
                    },
                    if disabled {
                        palette.accent.with_alpha(0.22)
                    } else if pressed {
                        palette.accent_press
                    } else {
                        palette.accent.with_alpha(0.90)
                    },
                ),
                backdrop: 0.0,
                corners: Corners::all(metrics.radius),
                border: Border {
                    width: metrics.hairline,
                    color: Color::white_alpha(if disabled { 0.08 } else { 0.22 }),
                },
                shadow: if disabled {
                    Shadow::NONE
                } else {
                    Shadow::new(Color::black_alpha(0.28), 6.0, 2.0)
                },
                text: if disabled {
                    palette.text_on_accent.with_alpha(0.45)
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
                backdrop: 0.0,
                corners: Corners::all(metrics.radius),
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    palette.text_quaternary
                } else {
                    palette
                        .text_secondary
                        .lerp(palette.text_primary, f32::from(interactive))
                },
            },
            Role::Tinted => Surface {
                fill: Fill::Gradient(
                    if disabled {
                        palette.accent.with_alpha(0.10)
                    } else if pressed {
                        palette.accent.with_alpha(0.36)
                    } else if hovered {
                        palette.accent.with_alpha(0.28)
                    } else {
                        palette.accent.with_alpha(0.20)
                    },
                    if disabled {
                        palette.accent.with_alpha(0.08)
                    } else {
                        palette.accent.with_alpha(0.15)
                    },
                ),
                backdrop: 0.0,
                corners: Corners::all(metrics.radius),
                border: Border {
                    width: metrics.hairline,
                    color: palette.accent.with_alpha(if pressed { 0.55 } else { 0.36 }),
                },
                shadow: Shadow::NONE,
                text: palette.text_primary,
            },
            Role::Field => Surface {
                fill: Fill::Solid(if disabled {
                    palette.field.with_alpha(0.5)
                } else {
                    palette.field
                }),
                backdrop: 0.0,
                corners: Corners::all(metrics.radius),
                border: Border {
                    width: metrics.hairline,
                    color: if interactive && !disabled {
                        palette.border.with_alpha(0.24)
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
                backdrop: 0.0,
                corners: Corners::all(metrics.radius),
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
