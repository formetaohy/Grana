use glam::Vec2;

use crate::color::Color;
use crate::geometry::Corners;
use crate::style::{Border, Fill, Look, Shadow, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Panel,
    Sheet,
    Toolbar,
    Transport,
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

    pub const fn medium(self) -> Self {
        Self {
            weight: 500.0,
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
    pub brand: Color,
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
    pub separator: Color,
    pub border: Color,
    pub raised: Color,
    pub selected: Color,
    pub card: Color,
    pub field: Color,
    pub focus: Color,
    pub scrim: Color,
    pub scene_backdrop: Color,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            text_primary: Color::bytes(250, 250, 250),
            text_secondary: Color::bytes(161, 161, 170),
            text_tertiary: Color::bytes(113, 113, 122),
            text_quaternary: Color::bytes(82, 82, 91),
            text_on_accent: Color::bytes(24, 24, 27),
            accent: Color::bytes(250, 250, 250),
            accent_hover: Color::bytes(228, 228, 231),
            accent_press: Color::bytes(212, 212, 216),
            brand: Color::bytes(250, 250, 250),
            danger: Color::bytes(248, 113, 113),
            warning: Color::bytes(251, 191, 36),
            success: Color::bytes(74, 222, 128),
            separator: Color::bytes(31, 31, 35),
            border: Color::bytes(39, 39, 42),
            raised: Color::bytes(24, 24, 27),
            selected: Color::bytes(39, 39, 42),
            card: Color::bytes(10, 10, 11),
            field: Color::bytes(10, 10, 11),
            focus: Color::bytes(212, 212, 216),
            scrim: Color::rgba(0.0, 0.0, 0.0, 0.6),
            scene_backdrop: Color::bytes(5, 5, 6),
        }
    }

    pub fn light() -> Self {
        Self {
            text_primary: Color::bytes(9, 9, 11),
            text_secondary: Color::bytes(82, 82, 91),
            text_tertiary: Color::bytes(113, 113, 122),
            text_quaternary: Color::bytes(161, 161, 170),
            text_on_accent: Color::bytes(250, 250, 250),
            accent: Color::bytes(24, 24, 27),
            accent_hover: Color::bytes(39, 39, 42),
            accent_press: Color::bytes(63, 63, 70),
            brand: Color::bytes(9, 9, 11),
            danger: Color::bytes(220, 38, 38),
            warning: Color::bytes(180, 83, 9),
            success: Color::bytes(22, 163, 74),
            separator: Color::bytes(228, 228, 231),
            border: Color::bytes(228, 228, 231),
            raised: Color::bytes(244, 244, 245),
            selected: Color::bytes(228, 228, 231),
            card: Color::bytes(255, 255, 255),
            field: Color::bytes(255, 255, 255),
            focus: Color::bytes(24, 24, 27),
            scrim: Color::rgba(0.0, 0.0, 0.0, 0.4),
            scene_backdrop: Color::bytes(247, 247, 248),
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
    pub radius: f32,
    pub card_radius: f32,
    pub popover_radius: f32,
    pub panel_radius: f32,
    pub hairline: f32,
}

impl Metrics {
    pub const fn standard() -> Self {
        Self {
            control_height: 28.0,
            row_height: 30.0,
            gap: 6.0,
            padding: 12.0,
            inset: 12.0,
            radius: 6.0,
            card_radius: 8.0,
            popover_radius: 8.0,
            panel_radius: 8.0,
            hairline: 1.0,
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
            large_title: TypeStyle::new(22.0, 600.0, 28.0),
            title: TypeStyle::new(15.0, 600.0, 20.0),
            headline: TypeStyle::new(13.0, 500.0, 18.0),
            body: TypeStyle::new(13.0, 400.0, 18.0),
            callout: TypeStyle::new(12.0, 400.0, 16.0),
            caption: TypeStyle::new(11.0, 400.0, 14.0),
            micro: TypeStyle {
                size: 10.0,
                weight: 500.0,
                tracking: 0.3,
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

    pub fn look(&self, role: Role, state: State) -> Look {
        let palette = self.palette;
        let metrics = self.metrics;
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
        let border = Border {
            width: metrics.hairline,
            color: palette.border,
        };
        let radius = Corners::all(metrics.radius);
        let card = Corners::all(metrics.card_radius);
        let popover = Corners::all(metrics.popover_radius);
        match role {
            Role::Panel => Look {
                fill: Fill::Solid(palette.card),
                corners: card,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: palette.text_primary,
            },
            Role::Sheet => Look {
                fill: Fill::Solid(palette.card),
                corners: Corners::ZERO,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: palette.text_primary,
            },
            Role::Toolbar => Look {
                fill: Fill::Solid(palette.raised),
                corners: radius,
                border,
                shadow: Shadow::NONE,
                text: palette.text_primary,
            },
            Role::Transport => Look {
                fill: Fill::Solid(palette.raised),
                corners: radius,
                border,
                shadow: Shadow::NONE,
                text: palette.text_primary,
            },
            Role::Popover => Look {
                fill: Fill::Solid(palette.card),
                corners: popover,
                border,
                shadow: Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.55), 32.0, 10.0),
                text: palette.text_primary,
            },
            Role::Chip => Look {
                fill: Fill::Solid(Color::rgba(0.016, 0.016, 0.02, 0.72)),
                corners: radius,
                border: Border {
                    width: metrics.hairline,
                    color: Color::white_alpha(0.10),
                },
                shadow: Shadow::NONE,
                text: palette.text_primary,
            },
            Role::Hud => Look {
                fill: Fill::Solid(if pressed {
                    palette.selected
                } else if hovered {
                    palette.raised
                } else {
                    Color::rgba(0.039, 0.039, 0.043, 0.96)
                }),
                corners: radius,
                border,
                shadow: Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.4), 24.0, 8.0),
                text: palette.text_primary,
            },
            Role::Card => Look {
                fill: Fill::Solid(palette.card),
                corners: card,
                border,
                shadow: Shadow::NONE,
                text: palette.text_primary,
            },
            Role::Row => Look {
                fill: if pressed || selected {
                    Fill::Solid(palette.selected)
                } else if hovered {
                    Fill::Solid(palette.raised)
                } else {
                    Fill::None
                },
                corners: Corners::all(metrics.radius - 2.0),
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: text(if selected || hovered {
                    palette.text_primary
                } else {
                    palette.text_secondary
                }),
            },
            Role::Control => Look {
                fill: if disabled {
                    Fill::None
                } else if pressed {
                    Fill::Solid(palette.selected)
                } else if hovered {
                    Fill::Solid(palette.raised)
                } else {
                    Fill::None
                },
                corners: radius,
                border: Border {
                    width: metrics.hairline,
                    color: if disabled {
                        palette.separator
                    } else {
                        palette.border
                    },
                },
                shadow: Shadow::NONE,
                text: text(palette.text_primary),
            },
            Role::Prominent => Look {
                fill: Fill::Solid(if disabled {
                    palette.accent.with_alpha(0.4)
                } else if pressed {
                    palette.accent_press
                } else if hovered {
                    palette.accent_hover
                } else {
                    palette.accent
                }),
                corners: radius,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    palette.text_quaternary
                } else {
                    palette.text_on_accent
                },
            },
            Role::Tinted => Look {
                fill: Fill::Solid(if disabled {
                    palette.raised.with_alpha(0.5)
                } else if pressed || hovered {
                    palette.selected
                } else {
                    palette.raised
                }),
                corners: radius,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: text(palette.text_primary),
            },
            Role::Segment => Look {
                fill: if pressed {
                    Fill::Solid(palette.selected)
                } else if hovered {
                    Fill::Solid(palette.raised)
                } else {
                    Fill::None
                },
                corners: Corners::all(metrics.radius - 2.0),
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: text(palette.text_secondary),
            },
            Role::SegmentSelected => Look {
                fill: Fill::Solid(palette.selected),
                corners: Corners::all(metrics.radius - 2.0),
                border: Border {
                    width: metrics.hairline,
                    color: palette.border,
                },
                shadow: Shadow::NONE,
                text: text(palette.text_primary),
            },
            Role::Track => Look {
                fill: Fill::Solid(Color::bytes(63, 63, 70)),
                corners: Corners::capsule(),
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: palette.text_secondary,
            },
            Role::Thumb => Look {
                fill: Fill::Solid(palette.accent),
                corners: Corners::capsule(),
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: palette.text_primary,
            },
            Role::Quiet => Look {
                fill: if pressed {
                    Fill::Solid(palette.selected)
                } else if hovered {
                    Fill::Solid(palette.raised)
                } else {
                    Fill::None
                },
                corners: radius,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: text(palette.text_secondary),
            },
            Role::Field => Look {
                fill: Fill::Solid(palette.field),
                corners: radius,
                border: Border {
                    width: metrics.hairline,
                    color: if hovered && !disabled {
                        Color::bytes(63, 63, 70)
                    } else {
                        palette.border
                    },
                },
                shadow: Shadow::NONE,
                text: text(palette.text_primary),
            },
            Role::Danger => Look {
                fill: Fill::Solid(if pressed {
                    palette.danger.with_alpha(0.18)
                } else if hovered {
                    palette.danger.with_alpha(0.12)
                } else {
                    Color::TRANSPARENT
                }),
                corners: radius,
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

    pub fn panel_offset(&self) -> Vec2 {
        Vec2::splat(self.metrics.inset)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
