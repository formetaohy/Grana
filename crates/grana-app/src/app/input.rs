use grana_session::Command;
use grana_ui::{Button as UiButton, Event as UiEvent, Modifiers as UiModifiers, Vec2 as UiVec2};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::CursorIcon;

use super::App;
impl App {
    pub(super) fn sync_ime(&mut self) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let editing = surface.ui.editing();
        if editing != self.ime {
            surface.window.set_ime_allowed(editing);
            self.ime = editing;
            self.ime_area = None;
        }
        let Some(caret) = surface.ui.edit_caret() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let area = (
            (caret.min.x * scale).round() as i32,
            (caret.min.y * scale).round() as i32,
            (caret.width().max(1.0) * scale).round().max(1.0) as u32,
            (caret.height().max(1.0) * scale).round().max(1.0) as u32,
        );
        if self.ime_area == Some(area) {
            return;
        }
        surface.window.set_ime_cursor_area(
            PhysicalPosition::new(area.0, area.1),
            PhysicalSize::new(area.2, area.3),
        );
        self.ime_area = Some(area);
    }

    pub(super) fn on_key(&mut self, event: &winit::event::KeyEvent) {
        if event.state != ElementState::Pressed || event.repeat {
            return;
        }
        let key = interface_key(&event.logical_key);
        let claimed = key.is_some_and(|key| {
            self.surface
                .as_ref()
                .is_some_and(|surface| surface.ui.claims_key(key))
        });
        if shortcut(self.modifiers) {
            if let Key::Character(text) = &event.logical_key
                && let Some(character) = text.chars().next()
                && let Some(message) = grana_editor::shortcut(character, self.modifiers.shift_key())
                && !claimed
            {
                self.dispatch(message);
            }
            self.request_redraw();
            return;
        }
        if self.workspace.palette.is_some() {
            if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                self.dispatch(grana_editor::Message::ClosePalette);
            }
            self.request_redraw();
            return;
        }
        if claimed {
            self.request_redraw();
            return;
        }
        match event.logical_key {
            Key::Named(NamedKey::Space) => self.toggle_play(),
            Key::Named(NamedKey::ArrowRight) => self.step_time(1),
            Key::Named(NamedKey::ArrowLeft) => self.step_time(-1),
            Key::Named(NamedKey::ArrowUp) if self.camera.slice().is_some() => self.step_slice(1),
            Key::Named(NamedKey::ArrowDown) if self.camera.slice().is_some() => self.step_slice(-1),
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown) => return,
            Key::Named(NamedKey::Escape) => {
                if self.workspace.annotation_pending.take().is_none()
                    && self.workspace.annotation_tool.take().is_none()
                    && self.workspace.seed.take().is_none()
                {
                    return;
                }
            }
            Key::Named(NamedKey::Delete) => {
                let annotation = self
                    .session
                    .scene()
                    .and_then(|scene| scene.annotations.selected());
                if let Some(index) = annotation {
                    self.apply(vec![Command::RemoveAnnotation(index)]);
                } else if self.session.delete_selected() {
                    self.output = None;
                }
            }
            Key::Character(ref text) if text.as_str().eq_ignore_ascii_case("h") => {
                self.session.hide_selected();
            }
            _ => return,
        }
        self.request_redraw();
    }

    pub(super) fn push_event(&mut self, event: &WindowEvent) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let input = surface.ui.input_mut();
        match event {
            WindowEvent::CursorMoved { position, .. } => input.push(UiEvent::Moved(UiVec2::new(
                position.x as f32 / scale,
                position.y as f32 / scale,
            ))),
            WindowEvent::CursorLeft { .. } => input.push(UiEvent::Leave),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => UiButton::Primary,
                    MouseButton::Right => UiButton::Secondary,
                    MouseButton::Middle => UiButton::Middle,
                    _ => return,
                };
                input.push(match state {
                    ElementState::Pressed => UiEvent::Down(button),
                    ElementState::Released => UiEvent::Up(button),
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(x, y) => UiVec2::new(*x, *y),
                    MouseScrollDelta::PixelDelta(position) => {
                        UiVec2::new(position.x as f32, position.y as f32) / 48.0
                    }
                };
                input.push(UiEvent::Wheel(lines));
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                input.push(UiEvent::Modifiers(UiModifiers {
                    shift: state.shift_key(),
                    control: state.control_key(),
                    alt: state.alt_key(),
                    logo: state.super_key(),
                }));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let Some(key) = interface_key(&event.logical_key) else {
                    return;
                };
                input.push(UiEvent::Key {
                    key,
                    pressed: event.state == ElementState::Pressed,
                    repeat: event.repeat,
                });
            }
            WindowEvent::Ime(Ime::Preedit(text, _)) => {
                input.push(UiEvent::Preedit((!text.is_empty()).then(|| text.clone())));
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                input.push(UiEvent::Text(text.clone()));
            }
            _ => return,
        }
        self.interacting = true;
        self.request_redraw();
    }
}

pub(super) fn interface_key(key: &Key) -> Option<grana_ui::InputKey> {
    match key {
        Key::Named(NamedKey::Escape) => Some(grana_ui::InputKey::Escape),
        Key::Named(NamedKey::Enter) => Some(grana_ui::InputKey::Enter),
        Key::Named(NamedKey::Tab) => Some(grana_ui::InputKey::Tab),
        Key::Named(NamedKey::Space) => Some(grana_ui::InputKey::Space),
        Key::Named(NamedKey::ArrowLeft) => Some(grana_ui::InputKey::Left),
        Key::Named(NamedKey::ArrowRight) => Some(grana_ui::InputKey::Right),
        Key::Named(NamedKey::ArrowUp) => Some(grana_ui::InputKey::Up),
        Key::Named(NamedKey::ArrowDown) => Some(grana_ui::InputKey::Down),
        Key::Named(NamedKey::Home) => Some(grana_ui::InputKey::Home),
        Key::Named(NamedKey::End) => Some(grana_ui::InputKey::End),
        Key::Named(NamedKey::PageUp) => Some(grana_ui::InputKey::PageUp),
        Key::Named(NamedKey::PageDown) => Some(grana_ui::InputKey::PageDown),
        Key::Named(NamedKey::Backspace) => Some(grana_ui::InputKey::Backspace),
        Key::Named(NamedKey::Delete) => Some(grana_ui::InputKey::Delete),
        Key::Character(text) => text.chars().next().map(grana_ui::InputKey::Character),
        _ => None,
    }
}

pub(crate) fn interface_cursor(cursor: grana_ui::Cursor) -> CursorIcon {
    match cursor {
        grana_ui::Cursor::Arrow => CursorIcon::Default,
        grana_ui::Cursor::Pointing => CursorIcon::Pointer,
        grana_ui::Cursor::Crosshair => CursorIcon::Crosshair,
        grana_ui::Cursor::Grab => CursorIcon::Grab,
        grana_ui::Cursor::Grabbing => CursorIcon::Grabbing,
        grana_ui::Cursor::ResizeHorizontal => CursorIcon::ColResize,
        grana_ui::Cursor::ResizeVertical => CursorIcon::RowResize,
        grana_ui::Cursor::Move => CursorIcon::Move,
        grana_ui::Cursor::Text => CursorIcon::Text,
    }
}

pub(super) fn shortcut(modifiers: ModifiersState) -> bool {
    modifiers.control_key() || modifiers.super_key()
}
