use glam::Vec2;
use grana_display::Viewport;
use grana_ui::{Button, Cursor, Modifiers, Rect, Ui};
use grana_volume::Spacing;

use crate::state::AnnotationTool;

const SECTION_STEP_VOXELS: f32 = 1.0;
const CLICK_SLOP: f32 = 3.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pane {
    #[default]
    Volume,
    Section,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ToolDrag {
    pub pane: Pane,
    pub start: Vec2,
    pub position: Vec2,
    pub finished: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GrabDrag {
    pub pane: Pane,
    pub position: Vec2,
    pub delta: Vec2,
    pub finished: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PaintDrag {
    pub pane: Pane,
    pub position: Vec2,
    pub finished: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Interaction {
    pub camera: bool,
    pub pick: Option<(Pane, Vec2)>,
    pub tool: Option<ToolDrag>,
    pub grab: Option<GrabDrag>,
    pub paint: Option<PaintDrag>,
    pub modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Orbit,
    Pan,
    Section,
    Tool,
    Grab,
    Paint,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    pane: Pane,
    cursor: Vec2,
    dragged: bool,
}

pub struct Stage {
    drag: Option<Drag>,
    drag_button: Option<Button>,
    press: Option<Press>,

    hovered: Option<Pane>,
    volume_region: [u32; 4],
    section_region: [u32; 4],
    volume_rect: Rect,
    section_rect: Rect,
    modifiers: Modifiers,
}

impl Default for Stage {
    fn default() -> Self {
        Self::new()
    }
}

impl Stage {
    pub fn new() -> Self {
        Self {
            drag: None,
            drag_button: None,
            press: None,

            hovered: None,
            volume_region: [0, 0, 1, 1],
            section_region: [0, 0, 1, 1],
            volume_rect: Rect::ZERO,
            section_rect: Rect::ZERO,
            modifiers: Modifiers::default(),
        }
    }

    pub fn set_rects(&mut self, volume: Rect, section: Rect) {
        self.volume_rect = volume;
        self.section_rect = section;
    }

    pub fn volume_rect(&self) -> Rect {
        self.volume_rect
    }

    pub fn section_rect(&self) -> Rect {
        self.section_rect
    }

    pub fn set_regions(&mut self, volume: [u32; 4], section: [u32; 4]) {
        self.volume_region = volume;
        self.section_region = section;
    }

    pub fn volume_region(&self) -> [u32; 4] {
        self.volume_region
    }

    pub fn pane_at(&self, point: Vec2) -> Option<Pane> {
        if self.volume_rect.contains(point) {
            Some(Pane::Volume)
        } else if self.section_rect.contains(point) {
            Some(Pane::Section)
        } else {
            None
        }
    }

    pub fn input<A: Clone>(
        &mut self,
        ui: &mut Ui<A>,
        viewport: &mut Viewport,
        spacing: Spacing,
        tool: Option<AnnotationTool>,
        paint: bool,
        grabbed: bool,
    ) -> Interaction {
        let mut interaction = Interaction::default();
        let blocked = ui.pointer_over_chrome() || ui.pointer_over_widget() || ui.pointer_captured();
        let input = ui.input();
        self.modifiers = input.modifiers();
        interaction.modifiers = self.modifiers;
        let pointer = input.position();

        self.hovered = self.pane_at(pointer);
        let delta = input.delta();
        if let Some(drag) = self.drag {
            let moving = delta.length_squared() > 0.0;
            match drag {
                Drag::Orbit => {
                    if moving {
                        viewport.camera_mut().orbit(delta);
                        interaction.camera = true;
                    }
                }
                Drag::Pan => {
                    if moving {
                        viewport
                            .camera_mut()
                            .pan(delta, self.volume_region[3] as f32 / ui.scale());
                        interaction.camera = true;
                    }
                }
                Drag::Section => {
                    if moving {
                        let span = viewport.section().span();
                        let world_per_pixel = span / self.section_rect.height();
                        viewport.section_mut().pan(delta, world_per_pixel);
                        interaction.camera = true;
                    }
                }
                Drag::Tool => {
                    if let Some(press) = self.press {
                        interaction.tool = Some(ToolDrag {
                            pane: press.pane,
                            start: press.cursor,
                            position: pointer,
                            finished: false,
                        });
                    }
                }
                Drag::Grab => {
                    if let Some(press) = self.press {
                        interaction.grab = Some(GrabDrag {
                            pane: press.pane,
                            position: pointer,
                            delta,
                            finished: false,
                        });
                    }
                }
                Drag::Paint => {
                    if let Some(press) = self.press {
                        interaction.paint = Some(PaintDrag {
                            pane: press.pane,
                            position: pointer,
                            finished: false,
                        });
                    }
                }
            }
            if let Some(press) = self.press.as_mut()
                && (pointer - press.cursor).length() > CLICK_SLOP
            {
                press.dragged = true;
            }
        }
        let left = input.button(Button::Primary);
        let middle = input.button(Button::Middle);
        let right = input.button(Button::Secondary);
        if self.drag.is_none()
            && !blocked
            && let Some(pane) = self.hovered
        {
            for (state, button) in [
                (left, Button::Primary),
                (middle, Button::Middle),
                (right, Button::Secondary),
            ] {
                if !state.pressed {
                    continue;
                }
                let drag = if button == Button::Primary && paint {
                    Drag::Paint
                } else if button == Button::Primary && tool.is_some() {
                    Drag::Tool
                } else if button == Button::Primary && grabbed {
                    Drag::Grab
                } else {
                    match (pane, button) {
                        (Pane::Volume, Button::Primary) => Drag::Orbit,
                        (Pane::Volume, _) => Drag::Pan,
                        (Pane::Section, _) => Drag::Section,
                    }
                };
                self.drag = Some(drag);
                self.drag_button = Some(button);
                self.press = Some(Press {
                    pane,
                    cursor: pointer,
                    dragged: false,
                });
                if drag == Drag::Paint {
                    interaction.paint = Some(PaintDrag {
                        pane,
                        position: pointer,
                        finished: false,
                    });
                }
                break;
            }
        }
        if let Some(drag) = self.drag {
            let released = match self.drag_button {
                Some(Button::Primary) => left.released,
                Some(Button::Middle) => middle.released,
                Some(Button::Secondary) => right.released,
                None => true,
            };
            if released {
                self.drag = None;
                self.drag_button = None;
                let press = self.press.take();
                match (drag, press) {
                    (Drag::Tool, Some(press)) => {
                        interaction.tool = Some(ToolDrag {
                            pane: press.pane,
                            start: press.cursor,
                            position: pointer,
                            finished: true,
                        });
                    }
                    (Drag::Grab, Some(press)) => {
                        interaction.grab = Some(GrabDrag {
                            pane: press.pane,
                            position: pointer,
                            delta,
                            finished: true,
                        });
                    }
                    (Drag::Paint, Some(press)) => {
                        interaction.paint = Some(PaintDrag {
                            pane: press.pane,
                            position: pointer,
                            finished: true,
                        });
                    }
                    (_, Some(press)) if !press.dragged && self.pane_at(press.cursor).is_some() => {
                        interaction.pick = Some((press.pane, press.cursor));
                    }
                    _ => {}
                }
            }
        }
        if !blocked && self.hovered.is_some() && input.wheel().y != 0.0 {
            let scroll = input.wheel().y;
            match self.hovered {
                Some(Pane::Volume) => viewport.camera_mut().zoom(scroll),
                Some(Pane::Section) => {
                    if self.modifiers.control {
                        viewport.section_mut().zoom(scroll);
                    } else {
                        let step = section_step(viewport, spacing);
                        viewport.section_mut().shift(scroll * step);
                    }
                }
                None => {}
            }
            interaction.camera = true;
        }
        if self.hovered.is_some() && !blocked && (paint || tool.is_none()) && !grabbed {
            ui.set_cursor(if paint {
                Cursor::Crosshair
            } else if self.hovered == Some(Pane::Section) {
                Cursor::Move
            } else {
                Cursor::Grab
            });
        }
        interaction
    }
}

fn section_step(viewport: &Viewport, spacing: Spacing) -> f32 {
    let normal = viewport.section().normal().abs();
    (spacing.x() * normal.x + spacing.y() * normal.y + spacing.z() * normal.z) * SECTION_STEP_VOXELS
}
