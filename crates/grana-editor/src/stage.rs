use glam::Vec2;
use grana_display::Viewport;
use grana_ui::{Button, Cursor, Modifiers, Rect, Ui};
use grana_volume::Spacing;

const SECTION_STEP_VOXELS: f32 = 1.0;
const CLICK_SLOP: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Volume,
    Section,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Interaction {
    pub camera: bool,
    pub pick: Option<(Pane, Vec2)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Orbit,
    Pan,
    Section,
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
    cursor: Option<Vec2>,
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
            cursor: None,
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

    pub fn ndc(&self, pane: Pane, point: Vec2) -> Vec2 {
        let region = self.region(pane);
        Vec2::new(
            (point.x - region[0] as f32) / region[2] as f32 * 2.0 - 1.0,
            1.0 - (point.y - region[1] as f32) / region[3] as f32 * 2.0,
        )
    }

    pub fn aspect(&self, pane: Pane) -> f32 {
        let region = self.region(pane);
        region[2] as f32 / region[3] as f32
    }

    pub fn hovered(&self) -> Option<Pane> {
        self.hovered
    }

    pub fn cursor(&self) -> Option<Vec2> {
        self.cursor
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

    pub fn input(&mut self, ui: &mut Ui, viewport: &mut Viewport, spacing: Spacing) -> Interaction {
        let mut interaction = Interaction::default();
        let blocked = ui.pointer_over_chrome() || ui.pointer_captured();
        let input = ui.input();
        self.modifiers = input.modifiers();
        let pointer = input.position();
        self.cursor = input.inside().then_some(pointer);
        self.hovered = self.pane_at(pointer);
        let delta = input.delta();
        if let Some(drag) = self.drag {
            if delta.length_squared() > 0.0 {
                match drag {
                    Drag::Orbit => viewport.camera_mut().orbit(delta),
                    Drag::Pan => viewport
                        .camera_mut()
                        .pan(delta, self.volume_region[3] as f32 / ui.scale()),
                    Drag::Section => {
                        let axis = viewport.section().axis();
                        let world_per_pixel =
                            viewport.section().span() / self.section_rect.height();
                        viewport
                            .camera_mut()
                            .pan_in_plane(axis, delta, world_per_pixel);
                    }
                }
                interaction.camera = true;
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
                self.drag = Some(match (pane, button) {
                    (Pane::Volume, Button::Primary) => Drag::Orbit,
                    (Pane::Volume, _) => Drag::Pan,
                    (Pane::Section, _) => Drag::Section,
                });
                self.drag_button = Some(button);
                self.press = Some(Press {
                    pane,
                    cursor: pointer,
                    dragged: false,
                });
                break;
            }
        }
        if self.drag.is_some() {
            let released = match self.drag_button {
                Some(Button::Primary) => left.released,
                Some(Button::Middle) => middle.released,
                Some(Button::Secondary) => right.released,
                None => true,
            };
            if released {
                self.drag = None;
                self.drag_button = None;
                if let Some(press) = self.press.take()
                    && !press.dragged
                    && self.pane_at(press.cursor).is_some()
                {
                    interaction.pick = Some((press.pane, press.cursor));
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
                        let axis = viewport.section().axis();
                        let step = axis.spacing(spacing) * SECTION_STEP_VOXELS;
                        viewport.camera_mut().move_along(axis, scroll * step);
                    }
                }
                None => {}
            }
            interaction.camera = true;
        }
        if self.hovered.is_some() && !blocked {
            ui.set_cursor(if self.hovered == Some(Pane::Section) {
                Cursor::Move
            } else {
                Cursor::Grab
            });
        }
        interaction
    }

    fn region(&self, pane: Pane) -> [u32; 4] {
        match pane {
            Pane::Volume => self.volume_region,
            Pane::Section => self.section_region,
        }
    }
}
