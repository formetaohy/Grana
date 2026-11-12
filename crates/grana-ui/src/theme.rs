use crate::color::Color;
use crate::geometry::Corners;
use crate::style::{Border, Fill, Shadow, State, Surface};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Sheet,
    FloatingToolbar,
    VibrantToolbar,
    Menu,
    Card,
    CardElevated,
    Filled,
    PrimaryContainer,
    Tonal,
    TertiaryContainer,
    SurfaceHigh,
    Outlined,
    Standard,
    Field,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Display,
    Headline,
    Title,
    Body,
    Label,
    Caption,
    Overline,
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

    pub const fn bold(self) -> Self {
        Self {
            weight: 700.0,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub surface_dim: Color,
    pub surface: Color,
    pub surface_bright: Color,
    pub surface_container_lowest: Color,
    pub surface_container_low: Color,
    pub surface_container: Color,
    pub surface_container_high: Color,
    pub surface_container_highest: Color,
    pub on_surface: Color,
    pub on_surface_variant: Color,
    pub outline: Color,
    pub outline_variant: Color,
    pub primary: Color,
    pub on_primary: Color,
    pub primary_container: Color,
    pub on_primary_container: Color,
    pub secondary: Color,
    pub on_secondary: Color,
    pub secondary_container: Color,
    pub on_secondary_container: Color,
    pub tertiary: Color,
    pub on_tertiary: Color,
    pub tertiary_container: Color,
    pub on_tertiary_container: Color,
    pub error: Color,
    pub on_error: Color,
    pub error_container: Color,
    pub on_error_container: Color,
    pub warning: Color,
    pub success: Color,
    pub scrim: Color,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            surface_dim: Color::bytes(9, 11, 16),
            surface: Color::bytes(15, 18, 24),
            surface_bright: Color::bytes(52, 57, 70),
            surface_container_lowest: Color::bytes(7, 9, 13),
            surface_container_low: Color::bytes(22, 26, 35),
            surface_container: Color::bytes(28, 33, 44),
            surface_container_high: Color::bytes(38, 44, 58),
            surface_container_highest: Color::bytes(49, 56, 72),
            on_surface: Color::bytes(226, 228, 236),
            on_surface_variant: Color::bytes(194, 198, 212),
            outline: Color::bytes(140, 144, 159),
            outline_variant: Color::bytes(66, 71, 84),
            primary: Color::bytes(168, 199, 250),
            on_primary: Color::bytes(6, 46, 111),
            primary_container: Color::bytes(8, 66, 160),
            on_primary_container: Color::bytes(211, 227, 253),
            secondary: Color::bytes(190, 198, 220),
            on_secondary: Color::bytes(40, 48, 65),
            secondary_container: Color::bytes(51, 68, 94),
            on_secondary_container: Color::bytes(216, 226, 255),
            tertiary: Color::bytes(120, 243, 224),
            on_tertiary: Color::bytes(0, 55, 49),
            tertiary_container: Color::bytes(0, 80, 71),
            on_tertiary_container: Color::bytes(156, 242, 228),
            error: Color::bytes(242, 184, 181),
            on_error: Color::bytes(96, 20, 16),
            error_container: Color::bytes(140, 29, 24),
            on_error_container: Color::bytes(249, 222, 220),
            warning: Color::bytes(255, 185, 81),
            success: Color::bytes(109, 213, 140),
            scrim: Color::rgba(0.0, 0.0, 0.0, 0.62),
        }
    }

    pub fn contrasted(self) -> Self {
        Self {
            on_surface: Color::WHITE,
            on_surface_variant: Color::bytes(224, 228, 240),
            outline: Color::bytes(184, 190, 206),
            outline_variant: Color::bytes(108, 114, 132),
            surface_container_low: Color::bytes(24, 29, 40),
            surface_container: Color::bytes(32, 38, 52),
            surface_container_high: Color::bytes(44, 52, 68),
            surface_container_highest: Color::bytes(58, 67, 86),
            primary: Color::bytes(196, 218, 255),
            secondary_container: Color::bytes(64, 84, 116),
            ..self
        }
    }

    pub fn series(self) -> [Color; 6] {
        [
            self.primary,
            self.tertiary,
            self.warning,
            self.error,
            Color::bytes(216, 180, 254),
            Color::bytes(109, 213, 140),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub control_height: f32,
    pub row_height: f32,
    pub gap: f32,
    pub group_gap: f32,
    pub corner_xs: f32,
    pub corner_sm: f32,
    pub corner_md: f32,
    pub corner_lg: f32,
    pub corner_xl: f32,
    pub corner_xxl: f32,
    pub slider_track: f32,
    pub slider_gap: f32,
    pub slider_handle_width: f32,
    pub slider_handle_height: f32,
    pub switch_width: f32,
    pub switch_height: f32,
}

impl Metrics {
    pub const fn expressive() -> Self {
        Self {
            control_height: 30.0,
            row_height: 32.0,
            gap: 6.0,
            group_gap: 2.0,
            corner_xs: 4.0,
            corner_sm: 8.0,
            corner_md: 12.0,
            corner_lg: 16.0,
            corner_xl: 24.0,
            corner_xxl: 28.0,
            slider_track: 14.0,
            slider_gap: 5.0,
            slider_handle_width: 4.0,
            slider_handle_height: 22.0,
            switch_width: 40.0,
            switch_height: 22.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Typography {
    pub display: TypeStyle,
    pub headline: TypeStyle,
    pub title: TypeStyle,
    pub body: TypeStyle,
    pub label: TypeStyle,
    pub caption: TypeStyle,
    pub overline: TypeStyle,
    pub monospace: TypeStyle,
}

impl Typography {
    pub const fn expressive() -> Self {
        Self {
            display: TypeStyle {
                size: 22.0,
                weight: 700.0,
                tracking: -0.2,
                leading: 28.0,
            },
            headline: TypeStyle {
                size: 15.0,
                weight: 600.0,
                tracking: 0.0,
                leading: 20.0,
            },
            title: TypeStyle {
                size: 13.0,
                weight: 600.0,
                tracking: 0.0,
                leading: 18.0,
            },
            body: TypeStyle {
                size: 12.5,
                weight: 400.0,
                tracking: 0.0,
                leading: 17.0,
            },
            label: TypeStyle {
                size: 12.0,
                weight: 500.0,
                tracking: 0.1,
                leading: 16.0,
            },
            caption: TypeStyle {
                size: 11.0,
                weight: 400.0,
                tracking: 0.1,
                leading: 14.5,
            },
            overline: TypeStyle {
                size: 10.0,
                weight: 600.0,
                tracking: 0.65,
                leading: 13.5,
            },
            monospace: TypeStyle {
                size: 11.0,
                weight: 400.0,
                tracking: 0.0,
                leading: 15.0,
            },
        }
    }

    pub fn style(&self, level: Level) -> TypeStyle {
        match level {
            Level::Display => self.display,
            Level::Headline => self.headline,
            Level::Title => self.title,
            Level::Body => self.body,
            Level::Label => self.label,
            Level::Caption => self.caption,
            Level::Overline => self.overline,
            Level::Monospace => self.monospace,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub palette: Palette,
    pub typography: Typography,
    pub metrics: Metrics,
    pub reduce_motion: bool,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            palette: Palette::dark(),
            typography: Typography::expressive(),
            metrics: Metrics::expressive(),
            reduce_motion: false,
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

    pub fn with_reduce_motion(self, reduce_motion: bool) -> Self {
        Self {
            reduce_motion,
            ..self
        }
    }

    pub fn text(&self, level: Level) -> Color {
        match level {
            Level::Display | Level::Headline | Level::Title | Level::Body => {
                self.palette.on_surface
            }
            Level::Label | Level::Monospace => self.palette.on_surface_variant,
            Level::Caption | Level::Overline => self.palette.outline,
        }
    }

    pub fn font(&self, level: Level) -> TypeStyle {
        self.typography.style(level)
    }

    pub fn button_font(&self) -> TypeStyle {
        self.typography.label
    }

    pub fn label_font(&self) -> TypeStyle {
        self.typography.caption
    }

    pub fn surface(&self, role: Role, state: State) -> Surface {
        let p = self.palette;
        let m = self.metrics;
        let disabled = state == State::Disabled;
        let hovered = state == State::Hover;
        let pressed = state == State::Pressed;
        let selected = state == State::Selected;
        let button_corners = if pressed {
            Corners::all(m.corner_sm)
        } else if selected {
            Corners::all(m.corner_md)
        } else {
            Corners::capsule()
        };
        let lift = |base: Color, on: Color| {
            if disabled {
                base.with_alpha(0.38)
            } else if pressed {
                base.lerp(on, 0.16)
            } else if hovered || selected {
                base.lerp(on, 0.10)
            } else {
                base
            }
        };
        match role {
            Role::Sheet => Surface {
                fill: Fill::Solid(p.surface_container_low),
                backdrop: 0.0,
                corners: Corners::all(m.corner_xl),
                border: Border::NONE,
                shadow: Shadow::new(Color::black_alpha(0.45), 24.0, 8.0),
                text: p.on_surface,
            },
            Role::FloatingToolbar => Surface {
                fill: Fill::Solid(p.surface_container.with_alpha(0.95)),
                backdrop: 18.0,
                corners: Corners::capsule(),
                border: Border::NONE,
                shadow: Shadow::new(Color::black_alpha(0.50), 22.0, 6.0),
                text: p.on_surface,
            },
            Role::VibrantToolbar => Surface {
                fill: Fill::Solid(p.primary_container.with_alpha(0.96)),
                backdrop: 18.0,
                corners: Corners::capsule(),
                border: Border::NONE,
                shadow: Shadow::new(Color::black_alpha(0.52), 24.0, 7.0),
                text: p.on_primary_container,
            },
            Role::Menu => Surface {
                fill: Fill::Solid(p.surface_container_high.with_alpha(0.98)),
                backdrop: 20.0,
                corners: Corners::all(m.corner_lg),
                border: Border::NONE,
                shadow: Shadow::new(Color::black_alpha(0.56), 26.0, 8.0),
                text: p.on_surface,
            },
            Role::Card => Surface {
                fill: Fill::Solid(p.surface_container),
                backdrop: 0.0,
                corners: Corners::all(m.corner_lg),
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: p.on_surface,
            },
            Role::CardElevated => Surface {
                fill: Fill::Solid(p.surface_container_high),
                backdrop: 0.0,
                corners: Corners::all(m.corner_lg),
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: p.on_surface,
            },
            Role::Filled => Surface {
                fill: Fill::Solid(lift(p.primary, p.on_primary)),
                backdrop: 0.0,
                corners: button_corners,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    p.on_surface.with_alpha(0.38)
                } else {
                    p.on_primary
                },
            },
            Role::PrimaryContainer => Surface {
                fill: Fill::Solid(lift(p.primary_container, p.on_primary_container)),
                backdrop: 0.0,
                corners: button_corners,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    p.on_surface.with_alpha(0.38)
                } else {
                    p.on_primary_container
                },
            },
            Role::Tonal => Surface {
                fill: Fill::Solid(lift(p.secondary_container, p.on_secondary_container)),
                backdrop: 0.0,
                corners: button_corners,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    p.on_surface.with_alpha(0.38)
                } else {
                    p.on_secondary_container
                },
            },
            Role::TertiaryContainer => Surface {
                fill: Fill::Solid(lift(p.tertiary_container, p.on_tertiary_container)),
                backdrop: 0.0,
                corners: button_corners,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    p.on_surface.with_alpha(0.38)
                } else {
                    p.on_tertiary_container
                },
            },
            Role::SurfaceHigh => Surface {
                fill: Fill::Solid(lift(p.surface_container_high, p.on_surface)),
                backdrop: 0.0,
                corners: button_corners,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    p.on_surface.with_alpha(0.38)
                } else {
                    p.on_surface
                },
            },
            Role::Outlined => Surface {
                fill: Fill::Solid(if pressed {
                    p.on_surface.with_alpha(0.12)
                } else if hovered || selected {
                    p.on_surface.with_alpha(0.08)
                } else {
                    Color::TRANSPARENT
                }),
                backdrop: 0.0,
                corners: button_corners,
                border: Border {
                    width: 1.0,
                    color: if disabled {
                        p.outline_variant.with_alpha(0.38)
                    } else if hovered || pressed {
                        p.outline
                    } else {
                        p.outline_variant
                    },
                },
                shadow: Shadow::NONE,
                text: if disabled {
                    p.on_surface.with_alpha(0.38)
                } else {
                    p.on_surface_variant.lerp(
                        p.on_surface,
                        if hovered || pressed || selected {
                            1.0
                        } else {
                            0.0
                        },
                    )
                },
            },
            Role::Standard => Surface {
                fill: Fill::Solid(if disabled {
                    Color::TRANSPARENT
                } else if pressed {
                    p.on_surface.with_alpha(0.14)
                } else if hovered || selected {
                    p.on_surface.with_alpha(0.09)
                } else {
                    Color::TRANSPARENT
                }),
                backdrop: 0.0,
                corners: button_corners,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    p.on_surface.with_alpha(0.38)
                } else if hovered || pressed || selected {
                    p.on_surface
                } else {
                    p.on_surface_variant
                },
            },
            Role::Field => Surface {
                fill: Fill::Solid(if disabled {
                    p.surface_container_highest.with_alpha(0.45)
                } else if pressed || selected {
                    p.surface_container_highest.lerp(p.primary, 0.10)
                } else if hovered {
                    p.surface_container_highest.lerp(p.on_surface, 0.06)
                } else {
                    p.surface_container_highest
                }),
                backdrop: 0.0,
                corners: Corners::all(m.corner_sm),
                border: Border {
                    width: 1.0,
                    color: if pressed || selected {
                        p.primary.with_alpha(0.75)
                    } else {
                        Color::TRANSPARENT
                    },
                },
                shadow: Shadow::NONE,
                text: if disabled {
                    p.on_surface.with_alpha(0.38)
                } else {
                    p.on_surface
                },
            },
            Role::Error => Surface {
                fill: Fill::Solid(if disabled {
                    Color::TRANSPARENT
                } else if pressed {
                    p.error_container.with_alpha(0.85)
                } else if hovered {
                    p.error_container.with_alpha(0.55)
                } else {
                    p.error_container.with_alpha(0.28)
                }),
                backdrop: 0.0,
                corners: button_corners,
                border: Border::NONE,
                shadow: Shadow::NONE,
                text: if disabled {
                    p.on_surface.with_alpha(0.38)
                } else if hovered || pressed {
                    p.on_error_container
                } else {
                    p.error
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
