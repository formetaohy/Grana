use glam::Vec2;
use grana_display::{PANE_COUNT, PaneKind, Panes};
use grana_ui::{Button, Cursor, Modifiers, Rect, Ui};
use grana_volume::Spacing;

use crate::state::AnnotationTool;

const SECTION_STEP_VOXELS: f32 = 1.0;
const CLICK_SLOP: f32 = 3.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct ToolDrag {
    pub pane: usize,
    pub start: Vec2,
    pub position: Vec2,
    pub finished: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GrabDrag {
    pub pane: usize,
    pub position: Vec2,
    pub delta: Vec2,
    pub finished: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PaintDrag {
    pub pane: usize,
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
    pub pick: Option<(usize, Vec2)>,
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
    pane: usize,
    cursor: Vec2,
    dragged: bool,
}

pub struct Stage {
    drag: Option<Drag>,
    drag_button: Option<Button>,
    press: Option<Press>,

    hovered: Option<usize>,
    rects: [Rect; PANE_COUNT],
    regions: [[u32; 4]; PANE_COUNT],
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
            rects: [Rect::ZERO; PANE_COUNT],
            regions: [[0, 0, 1, 1]; PANE_COUNT],
            modifiers: Modifiers::default(),
        }
    }

    pub fn set_rects(&mut self, rects: [Rect; PANE_COUNT]) {
        self.rects = rects;
    }

    pub fn rects(&self) -> [Rect; PANE_COUNT] {
        self.rects
    }

    pub fn rect(&self, index: usize) -> Rect {
        self.rects[index]
    }

    pub fn area(&self) -> Rect {
        self.rects
            .iter()
            .filter(|rect| rect.width() > 0.0 && rect.height() > 0.0)
            .fold(Rect::ZERO, |area, rect| {
                if area.width() <= 0.0 && area.height() <= 0.0 {
                    *rect
                } else {
                    Rect::from_min_max(area.min.min(rect.min), area.max.max(rect.max))
                }
            })
    }

    pub fn set_regions(&mut self, regions: [[u32; 4]; PANE_COUNT]) {
        self.regions = regions;
    }

    pub fn regions(&self) -> [[u32; 4]; PANE_COUNT] {
        self.regions
    }

    pub fn region(&self, index: usize) -> [u32; 4] {
        self.regions[index]
    }

    pub fn pane_at(&self, point: Vec2) -> Option<usize> {
        self.rects
            .iter()
            .position(|rect| rect.width() > 0.0 && rect.contains(point))
    }

    pub fn input<A: Clone>(
        &mut self,
        ui: &mut Ui<A>,
        panes: &mut Panes,
        active: &mut usize,
        spacing: Spacing,
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

        self.hovered = self.pane_at(pointer);
        let delta = input.delta();
        if let Some(drag) = self.drag {
            let moving = delta.length_squared() > 0.0;
            match drag {
                Drag::Orbit => {
                    if let Some(press) = self.press
                        && moving
                    {
                        panes.pane_mut(press.pane).camera_mut().orbit(delta);
                        interaction.camera = true;
                    }
                }
                Drag::Pan => {
                    if let Some(press) = self.press
                        && moving
                    {
                        let height = self.rects[press.pane].height();
                        panes
                            .pane_mut(press.pane)
                            .camera_mut()
                            .pan(delta, height / ui.scale());
                        interaction.camera = true;
                    }
                }
                Drag::Section => {
                    if let Some(press) = self.press
                        && moving
                    {
                        let height = self.rects[press.pane].height();
                        let pane = panes.pane_mut(press.pane);
                        let span = pane.section().span();
                        let world_per_pixel = span / height.max(1.0);
                        pane.section_mut().pan(delta, world_per_pixel);
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
                let kind = panes.pane(pane).kind();
                let drag = if button == Button::Primary && paint {
                    Drag::Paint
                } else if button == Button::Primary && tool.is_some() {
                    Drag::Tool
                } else if button == Button::Primary && grabbed {
                    Drag::Grab
                } else {
                    match (kind, button) {
                        (PaneKind::Volume, Button::Primary) => Drag::Orbit,
                        (PaneKind::Volume, _) => Drag::Pan,
                        (PaneKind::Section, _) => Drag::Section,
                    }
                };
                *active = pane;
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
        if !blocked
            && let Some(hovered) = self.hovered
            && input.wheel().y != 0.0
        {
            let scroll = input.wheel().y;
            let pane = panes.pane_mut(hovered);
            match pane.kind() {
                PaneKind::Volume => pane.camera_mut().zoom(scroll),
                PaneKind::Section => {
                    if self.modifiers.control {
                        pane.section_mut().zoom(scroll);
                    } else {
                        let step = section_step(pane.section().normal(), spacing);
                        pane.section_mut().shift(scroll * step);
                    }
                }
            }
            interaction.camera = true;
        }
        if let Some(hovered) = self.hovered
            && !blocked
            && (paint || tool.is_none())
            && !grabbed
        {
            ui.set_cursor(if paint {
                Cursor::Crosshair
            } else if panes.pane(hovered).kind() == PaneKind::Section {
                Cursor::Move
            } else {
                Cursor::Grab
            });
        }
        interaction
    }
}

fn section_step(normal: glam::Vec3, spacing: Spacing) -> f32 {
    let normal = normal.abs();
    (spacing.x() * normal.x + spacing.y() * normal.y + spacing.z() * normal.z) * SECTION_STEP_VOXELS
}
