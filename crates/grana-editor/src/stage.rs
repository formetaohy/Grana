use glam::Vec2;
use grana_display::Camera;
use grana_ui::{Button, Cursor, Modifiers, Rect, Ui};

use crate::state::AnnotationTool;

const CLICK_SLOP: f32 = 3.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct ToolDrag {
    pub start: Vec2,
    pub position: Vec2,
    pub finished: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GrabDrag {
    pub position: Vec2,
    pub delta: Vec2,
    pub finished: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PaintDrag {
    pub position: Vec2,
    pub finished: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Intent {
    pub tool: Option<AnnotationTool>,
    pub paint: bool,
    pub grabbed: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Interaction {
    pub camera: bool,
    pub pick: Option<Vec2>,
    pub tool: Option<ToolDrag>,
    pub grab: Option<GrabDrag>,
    pub paint: Option<PaintDrag>,
    pub modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Orbit,
    Pan,
    Tool,
    Grab,
    Paint,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    cursor: Vec2,
    dragged: bool,
}

pub struct Stage {
    drag: Option<Drag>,
    drag_button: Option<Button>,
    press: Option<Press>,

    hovered: bool,
    viewport: Rect,
    region: [u32; 4],
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

            hovered: false,
            viewport: Rect::ZERO,
            region: [0, 0, 1, 1],
            modifiers: Modifiers::default(),
        }
    }

    pub fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
    }

    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    pub fn set_region(&mut self, region: [u32; 4]) {
        self.region = region;
    }

    pub fn region(&self) -> [u32; 4] {
        self.region
    }

    pub fn contains(&self, point: Vec2) -> bool {
        self.viewport.width() > 0.0 && self.viewport.contains(point)
    }

    pub fn aspect(&self) -> f32 {
        self.viewport.width().max(1.0) / self.viewport.height().max(1.0)
    }

    pub fn input<A: Clone>(
        &mut self,
        ui: &mut Ui<A>,
        camera: &mut Camera,
        intent: Intent,
    ) -> Interaction {
        let Intent {
            tool,
            paint,
            grabbed,
        } = intent;
        let mut interaction = Interaction::default();
        let blocked = ui.pointer_over_chrome() || ui.pointer_over_widget() || ui.pointer_captured();
        let input = ui.input();
        self.modifiers = input.modifiers();
        interaction.modifiers = self.modifiers;
        let pointer = input.position();

        self.hovered = self.contains(pointer);
        let delta = input.delta();
        if let Some(drag) = self.drag {
            let moving = delta.length_squared() > 0.0;
            match drag {
                Drag::Orbit => {
                    if self.press.is_some() && moving {
                        camera.orbit(delta);
                        interaction.camera = true;
                    }
                }
                Drag::Pan => {
                    if self.press.is_some() && moving {
                        camera.pan(delta, self.viewport.height() / ui.scale());
                        interaction.camera = true;
                    }
                }
                Drag::Tool => {
                    if let Some(press) = self.press {
                        interaction.tool = Some(ToolDrag {
                            start: press.cursor,
                            position: pointer,
                            finished: false,
                        });
                    }
                }
                Drag::Grab => {
                    if self.press.is_some() {
                        interaction.grab = Some(GrabDrag {
                            position: pointer,
                            delta,
                            finished: false,
                        });
                    }
                }
                Drag::Paint => {
                    if self.press.is_some() {
                        interaction.paint = Some(PaintDrag {
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
        if self.drag.is_none() && !blocked && self.hovered {
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
                } else if button == Button::Primary {
                    Drag::Orbit
                } else {
                    Drag::Pan
                };
                self.drag = Some(drag);
                self.drag_button = Some(button);
                self.press = Some(Press {
                    cursor: pointer,
                    dragged: false,
                });
                if drag == Drag::Paint {
                    interaction.paint = Some(PaintDrag {
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
                            start: press.cursor,
                            position: pointer,
                            finished: true,
                        });
                    }
                    (Drag::Grab, Some(_)) => {
                        interaction.grab = Some(GrabDrag {
                            position: pointer,
                            delta,
                            finished: true,
                        });
                    }
                    (Drag::Paint, Some(_)) => {
                        interaction.paint = Some(PaintDrag {
                            position: pointer,
                            finished: true,
                        });
                    }
                    (_, Some(press)) if !press.dragged && self.contains(press.cursor) => {
                        interaction.pick = Some(press.cursor);
                    }
                    _ => {}
                }
            }
        }
        if !blocked && self.hovered && input.wheel().y != 0.0 {
            camera.zoom(input.wheel().y);
            interaction.camera = true;
        }
        if self.hovered && !blocked && (paint || tool.is_none()) && !grabbed {
            ui.set_cursor(if paint {
                Cursor::Crosshair
            } else {
                Cursor::Grab
            });
        }
        interaction
    }
}
