use glam::{Vec2, Vec3};
use grana_render::{Camera, SectionCamera};
use grana_volume::{Axis, Spacing};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::ModifiersState;

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

#[derive(Clone, Copy)]
enum Drag {
    Orbit,
    Pan,
    Section,
}

#[derive(Clone, Copy)]
struct Press {
    pane: Pane,
    cursor: Vec2,
    button: MouseButton,
    dragged: bool,
}

pub struct Viewport {
    camera: Camera,
    section: SectionCamera,
    drag: Option<Drag>,
    press: Option<Press>,
    cursor: Option<Vec2>,
    hovered: Option<Pane>,
    volume_region: [u32; 4],
    section_region: [u32; 4],
    modifiers: ModifiersState,
    split: f32,
}

impl Viewport {
    pub fn new() -> Self {
        Self {
            camera: Camera::new(35.0),
            section: SectionCamera::new(Axis::Z),
            drag: None,
            press: None,
            cursor: None,
            hovered: None,
            volume_region: [0, 0, 1, 1],
            section_region: [0, 0, 1, 1],
            modifiers: ModifiersState::default(),
            split: 0.56,
        }
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    pub fn section(&self) -> &SectionCamera {
        &self.section
    }

    pub fn section_mut(&mut self) -> &mut SectionCamera {
        &mut self.section
    }

    pub fn split(&self) -> f32 {
        self.split
    }

    pub fn set_split(&mut self, split: f32) {
        self.split = split.clamp(0.25, 0.8);
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

    pub fn fit(&mut self, world: Vec3, voxel: Vec3, volume_aspect: f32, section_aspect: f32) {
        self.camera.fit(world, volume_aspect);
        self.section.fit(world, voxel, section_aspect);
    }

    pub fn fit_section(&mut self, world: Vec3, voxel: Vec3, aspect: f32) {
        self.section.fit(world, voxel, aspect);
    }

    pub fn clamp_focus(&mut self, half: Vec3) {
        self.camera.clamp_focus(half);
    }

    pub fn on_window_event(
        &mut self,
        event: &WindowEvent,
        consumed: bool,
        spacing: Spacing,
    ) -> Interaction {
        let mut interaction = Interaction::default();
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let current = Vec2::new(position.x as f32, position.y as f32);
                let previous = self.cursor.replace(current);
                self.hovered = self.pane_at(current);
                if let Some(press) = self.press.as_mut()
                    && (current - press.cursor).length() > CLICK_SLOP
                {
                    press.dragged = true;
                }
                if consumed {
                    return interaction;
                }
                let (Some(previous), Some(drag)) = (previous, self.drag) else {
                    return interaction;
                };
                let delta = current - previous;
                match drag {
                    Drag::Orbit => self.camera.orbit(delta),
                    Drag::Pan => self.camera.pan(delta, self.volume_region[3] as f32),
                    Drag::Section => {
                        let world_per_pixel = self.section.span() / self.section_region[3] as f32;
                        self.camera
                            .pan_in_plane(self.section.axis(), delta, world_per_pixel);
                    }
                }
                interaction.camera = true;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if consumed {
                    return interaction;
                }
                match state {
                    ElementState::Pressed => {
                        self.drag = self.hovered.map(|pane| match (pane, button) {
                            (Pane::Volume, MouseButton::Left) => Drag::Orbit,
                            (Pane::Volume, _) => Drag::Pan,
                            (Pane::Section, _) => Drag::Section,
                        });
                        if let (Some(pane), Some(cursor)) = (self.hovered, self.cursor) {
                            self.press = Some(Press {
                                pane,
                                cursor,
                                button: *button,
                                dragged: false,
                            });
                        }
                    }
                    ElementState::Released => {
                        self.drag = None;
                        if let Some(press) = self.press.take()
                            && press.button == *button
                            && press.button == MouseButton::Left
                            && !press.dragged
                        {
                            interaction.pick = Some((press.pane, press.cursor));
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if consumed {
                    return interaction;
                }
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => *lines,
                    MouseScrollDelta::PixelDelta(position) => position.y as f32 / 120.0,
                };
                self.hovered = self.cursor.and_then(|cursor| self.pane_at(cursor));
                match self.hovered {
                    Some(Pane::Volume) => self.camera.zoom(scroll),
                    Some(Pane::Section) => {
                        if self.modifiers.control_key() {
                            self.section.zoom(scroll);
                        } else {
                            let step = self.section.axis().spacing(spacing) * SECTION_STEP_VOXELS;
                            self.camera.move_along(self.section.axis(), scroll * step);
                        }
                    }
                    None => return interaction,
                }
                interaction.camera = true;
            }
            _ => {}
        }
        interaction
    }

    fn region(&self, pane: Pane) -> [u32; 4] {
        match pane {
            Pane::Volume => self.volume_region,
            Pane::Section => self.section_region,
        }
    }

    fn pane_at(&self, point: Vec2) -> Option<Pane> {
        if contains(self.volume_region, point) {
            Some(Pane::Volume)
        } else if contains(self.section_region, point) {
            Some(Pane::Section)
        } else {
            None
        }
    }
}

fn contains(region: [u32; 4], point: Vec2) -> bool {
    point.x >= region[0] as f32
        && point.x < (region[0] + region[2]) as f32
        && point.y >= region[1] as f32
        && point.y < (region[1] + region[3]) as f32
}
