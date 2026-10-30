use glam::{Vec2, Vec3};
use grana_render::Camera;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};

#[derive(Clone, Copy)]
enum Drag {
    Orbit,
    Pan,
}

pub struct Viewport {
    camera: Camera,
    drag: Option<Drag>,
    cursor: Option<Vec2>,
    height: f32,
}

impl Viewport {
    pub fn new() -> Self {
        Self {
            camera: Camera::new(35.0),
            drag: None,
            cursor: None,
            height: 1.0,
        }
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn fit(&mut self, world_size: [f32; 3], aspect: f32) {
        self.camera.fit(Vec3::from(world_size), aspect);
    }

    pub fn resize(&mut self, height: f32) {
        self.height = height.max(1.0);
    }

    pub fn on_window_event(&mut self, event: &WindowEvent, consumed: bool) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let current = Vec2::new(position.x as f32, position.y as f32);
                let previous = self.cursor.replace(current);
                if consumed {
                    return false;
                }
                let (Some(previous), Some(drag)) = (previous, self.drag) else {
                    return false;
                };
                let delta = current - previous;
                match drag {
                    Drag::Orbit => self.camera.orbit(delta),
                    Drag::Pan => self.camera.pan(delta, self.height),
                }
                true
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if consumed {
                    return false;
                }
                match (state, button) {
                    (ElementState::Pressed, MouseButton::Left) => self.drag = Some(Drag::Orbit),
                    (ElementState::Pressed, MouseButton::Middle | MouseButton::Right) => {
                        self.drag = Some(Drag::Pan);
                    }
                    (ElementState::Released, _) => self.drag = None,
                    _ => {}
                }
                false
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if consumed {
                    return false;
                }
                match delta {
                    MouseScrollDelta::LineDelta(_, lines) => self.camera.zoom(lines * 0.5),
                    MouseScrollDelta::PixelDelta(position) => {
                        self.camera.zoom(position.y as f32 / 120.0);
                    }
                }
                true
            }
            _ => false,
        }
    }
}
