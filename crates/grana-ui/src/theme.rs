use glam::Vec2;

use crate::color::Color;
use crate::geometry::Corners;
use crate::style::{Border, Fill, Glass, Look, Shadow, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Panel,
    Sheet,
    Toolbar,
    Popover,
    Hud,
    Chip,
    Card,
    Row,
    Control,
    Prominent,
    Tinted,
    Segment,
    SegmentSelected,
    Track,
    Thumb,
    Quiet,
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
    pub scrim: Color,
    pub scene_backdrop: Color,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            text_primary: Color::rgba(1.0, 1.0, 1.0, 0.96),
            text_secondary: Color::rgba(1.0, 1.0, 1.0, 0.76),
            text_tertiary: Color::rgba(1.0, 1.0, 1.0, 0.54),
            text_quaternary: Color::rgba(1.0, 1.0, 1.0, 0.28),
            text_on_accent: Color::WHITE,
            accent: Color::bytes(10, 132, 255),
            accent_hover: Color::bytes(64, 156, 255),
            accent_press: Color::bytes(0, 100, 214),
            brand: Color::bytes(214, 232, 92),
            danger: Color::bytes(255, 69, 58),
            warning: Color::bytes(255, 214, 10),
            success: Color::bytes(48, 209, 88),
            separator: Color::rgba(1.0, 1.0, 1.0, 0.08),
            scrim: Color::rgba(0.0, 0.0, 0.0, 0.34),
            scene_backdrop: Color::bytes(8, 9, 13),
        }
    }

    pub fn light() -> Self {
        Self {
            text_primary: Color::rgba(0.0, 0.0, 0.0, 0.94),
            text_secondary: Color::rgba(0.0, 0.0, 0.0, 0.60),
            text_tertiary: Color::rgba(0.0, 0.0, 0.0, 0.38),
            text_quaternary: Color::rgba(0.0, 0.0, 0.0, 0.18),
            text_on_accent: Color::WHITE,
            accent: Color::bytes(0, 122, 255),
            accent_hover: Color::bytes(38, 145, 255),
            accent_press: Color::bytes(0, 100, 220),
            brand: Color::bytes(121, 138, 22),
            danger: Color::bytes(215, 0, 21),
            warning: Color::bytes(178, 128, 0),
            success: Color::bytes(28, 160, 60),
            separator: Color::rgba(0.0, 0.0, 0.0, 0.08),
            scrim: Color::rgba(0.0, 0.0, 0.0, 0.14),
            scene_backdrop: Color::bytes(242, 242, 247),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub control_height: f32,
    pub row_height: f32,
    pub gap: f32,
    pub padding: f32,
    pub inset: f32,
    pub toolbar_height: f32,
    pub radius: f32,
    pub panel_radius: f32,
    pub hairline: f32,
}

impl Metrics {
    pub const fn standard() -> Self {
        Self {
            control_height: 28.0,
            row_height: 32.0,
            gap: 8.0,
            padding: 14.0,
            inset: 12.0,
            toolbar_height: 54.0,
            radius: 9.0,
            panel_radius: 24.0,
            hairline: 0.5,
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
            large_title: TypeStyle::new(26.0, 700.0, 32.0),
            title: TypeStyle::new(17.0, 600.0, 22.0),
            headline: TypeStyle::new(13.0, 600.0, 18.0),
            body: TypeStyle::new(13.0, 400.0, 18.0),
            callout: TypeStyle::new(12.0, 400.0, 16.0),
            caption: TypeStyle::new(11.0, 400.0, 14.0),
            micro: TypeStyle {
                size: 10.0,
                weight: 600.0,
                tracking: 0.6,
                leading: 13.0,
            },
            monospace: TypeStyle::new(11.5, 400.0, 16.0),
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

    pub fn look(&self, role: Role, state: State) -> Look {
        let palette = self.palette;
        let metrics = self.metrics;
        let dark = self.dark;
        let disabled = state == State::Disabled;
        let pressed = state == State::Pressed;
        let hovered = state == State::Hover || pressed;
        let selected = state == State::Selected;
        let text = |base: Color| {
            if disabled {
                palette.text_quaternary
            } else {
                base
            }
        };
        let white = |alpha: f32| {
            if dark {
                Color::white_alpha(alpha)
            } else {
                Color::black_alpha(alpha * 0.72)
            }
        };
        match role {
            Role::Panel => Look {
                fill: Fill::Glass(Glass::REGULAR.tinted(if dark {
                    Color::rgba(0.150, 0.170, 0.210, 0.30)
                } else {
                    Color::rgba(1.0, 1.0, 1.0, 0.30)
                })),
                corners: Corners::all(metrics.panel_radius),
                border: Border {
                    width: metrics.hairline,
                    color: if dark {
                        Color::rgba(1.0, 1.0, 1.0, 0.30)
                    } else {
                        Color::rgba(1.0, 1.0, 1.0, 0.65)
                    },
                },
                shadow: Shadow::new(
                    Color::rgba(0.0, 0.0, 0.0, if dark { 0.26 } else { 0.14 }),
                    38.0,
                    13.0,
                ),
                text: palette.text_primary,
            },
            Role::Sheet => Look {
                fill: Fill::Glass(Glass::REGULAR.tinted(if dark {
                    Color::rgba(0.150, 0.170, 0.210, 0.30)
                } else {
                    Color::rgba(1.0, 1.0, 1.0, 0.30)
                })),
                corners: Corners::all(metrics.panel_radius),
                border: Border {
                    width: metrics.hairline,
                    color: if dark {
                        Color::rgba(1.0, 1.0, 1.0, 0.30)
                    } else {
                        Color::rgba(0.0, 0.0, 0.0, 0.06)
                    },
                },
                shadow: Shadow::new(
                    Color::rgba(0.0, 0.0, 0.0, if dark { 0.36 } else { 0.18 }),
                    44.0,
                    16.0,
                ),
                text: palette.text_primary,
            },
            Role::Toolbar => Look {
                fill: Fill::Glass(Glass::REGULAR.tinted(if dark {
                    Color::rgba(0.120, 0.140, 0.180, 0.18)
                } else {
                    Color::rgba(1.0, 1.0, 1.0, 0.30)
                })),
                corners: Corners::capsule(),
                border: Border {
                    width: metrics.hairline,
                    color: if dark {
                        Color::rgba(1.0, 1.0, 1.0, 0.30)
                    } else {
                        Color::rgba(1.0, 1.0, 1.0, 0.65)
                    },
                },
                shadow: Shadow::new(
                    Color::rgba(0.0, 0.0, 0.0, if dark { 0.22 } else { 0.10 }),
                    30.0,
                    11.0,
                ),
                text: palette.text_primary,
            },
            Role::Popover => Look {
                fill: Fill::Glass(Glass::REGULAR.tinted(if dark {
                    Color::rgba(0.150, 0.170, 0.210, 0.34)
                } else {
                    Color::rgba(0.98, 0.98, 0.99, 0.36)
                })),
                corners: Corners::all(metrics.radius + 5.0),
                border: Border {
                    width: metrics.hairline,
                    color: if dark {
                        Color::rgba(1.0, 1.0, 1.0, 0.20)
                    } else {
                        Color::rgba(0.0, 0.0, 0.0, 0.06)
                    },
                },
                shadow: Shadow::new(
                    Color::rgba(0.0, 0.0, 0.0, if dark { 0.42 } else { 0.20 }),
                    42.0,
                    16.0,
                ),
                text: palette.text_primary,
            },
            Role::Chip => Look {
                fill: Fill::Glass(Glass::CLEAR.tinted(if dark {
                    Color::rgba(0.82, 0.87, 0.96, 0.10)
                } else {
                    Color::rgba(1.0, 1.0, 1.0, 0.16)
                })),
                corners: Corners::all(metrics.radius + 1.0),
                border: Border {
                    width: metrics.hairline,
                    color: white(0.34),
                },
                shadow: Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.24), 22.0, 7.0),
                text: palette.text_primary,
            },
            Role::Hud => Look {
                fill: Fill::Solid(Color::rgba(
                    if dark { 0.008 } else { 0.06 },
                    if dark { 0.010 } else { 0.07 },
                    if dark { 0.014 } else { 0.09 },
                    if dark { 0.44 } else { 0.34 },
                )),
                corners: Corners::all(metrics.radius + 1.0),
                border: Border {
                    width: metrics.hairline,
                    color: white(0.10),
                },
                shadow: Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.18), 16.0, 4.0),
                text: palette.text_secondary,
            },
            Role::Card => Look {
                fill: Fill::Solid(if dark {
                    Color::rgba(0.035, 0.043, 0.060, 0.94)
                } else {
                    Color::rgba(1.0, 1.0, 1.0, 0.94)
                }),
                corners: Corners::all(metrics.radius + 4.0),
                border: Border {
                    width: metrics.hairline,
                    color: white(0.06),
                },
                shadow: Shadow::NONE,
                text: palette.text_primary,
            },
            Role::Row => Look {
                fill: if pressed {
                    Fill::Solid(white(0.10))
                } else if selected {
                    Fill::Solid(palette.accent.with_alpha(if dark { 0.20 } else { 0.14 }))
                } else if hovered {
                    Fill::Solid(white(0.055))
                } else {
                    Fill::None
                },
                corners: Corners::all(metrics.radius - 1.0),
                border: Border {
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::NONE,
                text: text(if selected {
                    palette.text_primary
                } else {
                    palette.text_secondary
                }),
            },
            Role::Control => Look {
                fill: Fill::Solid(if disabled {
                    white(0.025)
                } else if pressed {
                    white(0.18)
                } else if hovered {
                    white(0.13)
                } else {
                    white(0.085)
                }),
                corners: Corners::all(metrics.radius - 1.0),
                border: Border {
                    width: metrics.hairline,
                    color: white(0.07),
                },
                shadow: Shadow::NONE,
                text: text(palette.text_primary),
            },
            Role::Prominent => Look {
                fill: Fill::Solid(if disabled {
                    palette.accent.with_alpha(0.32)
                } else if pressed {
                    palette.accent_press
                } else if hovered {
                    palette.accent_hover
                } else {
                    palette.accent
                }),
                corners: Corners::all(metrics.radius - 1.0),
                border: Border {
                    width: metrics.hairline,
                    color: Color::WHITE.with_alpha(0.18),
                },
                shadow: if disabled {
                    Shadow::NONE
                } else {
                    Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.22), 14.0, 3.0)
                },
                text: palette.text_on_accent,
            },
            Role::Tinted => Look {
                fill: Fill::Solid(if pressed {
                    palette.accent.with_alpha(0.32)
                } else if hovered {
                    palette.accent.with_alpha(0.24)
                } else {
                    palette.accent.with_alpha(0.16)
                }),
                corners: Corners::all(metrics.radius - 1.0),
                border: Border {
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::NONE,
                text: text(palette.accent),
            },
            Role::Segment => Look {
                fill: if pressed {
                    Fill::Solid(white(0.10))
                } else if hovered {
                    Fill::Solid(white(0.06))
                } else {
                    Fill::None
                },
                corners: Corners::capsule(),
                border: Border {
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::NONE,
                text: text(palette.text_secondary),
            },
            Role::SegmentSelected => Look {
                fill: Fill::Solid(if dark {
                    Color::rgba(1.0, 1.0, 1.0, 0.16)
                } else {
                    Color::rgba(1.0, 1.0, 1.0, 0.94)
                }),
                corners: Corners::capsule(),
                border: Border {
                    width: metrics.hairline,
                    color: white(0.10),
                },
                shadow: Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.16), 10.0, 2.0),
                text: text(palette.text_primary),
            },
            Role::Track => Look {
                fill: Fill::Solid(white(if hovered { 0.16 } else { 0.12 })),
                corners: Corners::capsule(),
                border: Border {
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::NONE,
                text: palette.text_secondary,
            },
            Role::Thumb => Look {
                fill: Fill::Solid(if pressed {
                    Color::rgba(1.0, 1.0, 1.0, 0.92)
                } else {
                    Color::WHITE
                }),
                corners: Corners::capsule(),
                border: Border {
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.30), 12.0, 2.0),
                text: palette.text_primary,
            },
            Role::Quiet => Look {
                fill: if pressed {
                    Fill::Solid(white(0.13))
                } else if hovered {
                    Fill::Solid(white(0.07))
                } else {
                    Fill::None
                },
                corners: Corners::all(metrics.radius - 1.0),
                border: Border {
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::NONE,
                text: text(palette.text_secondary),
            },
            Role::Field => Look {
                fill: Fill::Solid(if disabled {
                    white(0.025)
                } else if hovered {
                    white(0.10)
                } else {
                    white(0.065)
                }),
                corners: Corners::all(metrics.radius - 1.0),
                border: Border {
                    width: metrics.hairline,
                    color: white(0.06),
                },
                shadow: Shadow::NONE,
                text: text(palette.text_primary),
            },
            Role::Danger => Look {
                fill: Fill::Solid(if pressed {
                    palette.danger.with_alpha(0.34)
                } else if hovered {
                    palette.danger.with_alpha(0.26)
                } else {
                    palette.danger.with_alpha(0.16)
                }),
                corners: Corners::all(metrics.radius - 1.0),
                border: Border {
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::NONE,
                text: palette.danger,
            },
        }
    }

    pub fn panel_offset(&self) -> Vec2 {
        Vec2::splat(self.metrics.inset)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
