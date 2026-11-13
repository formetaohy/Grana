use grana_editor::WindowAction;
use winit::dpi::LogicalPosition;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::CursorIcon;

use super::{App, Resume};
use crate::window::Press;

impl App {
    pub(super) fn track_window(&mut self, event: &WindowEvent) {
        let App {
            surface, chrome, ..
        } = self;
        let Some(surface) = surface.as_ref() else {
            return;
        };
        if let Some(cursor) = chrome.track(&surface.window, event) {
            surface.window.set_cursor(cursor);
        }
    }

    pub(super) fn claim_window(&mut self, event: &WindowEvent) -> Option<Press> {
        let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button,
            ..
        } = event
        else {
            return None;
        };
        let App {
            surface, chrome, ..
        } = self;
        let surface = surface.as_ref()?;
        let over_widget = surface.ui.pointer_over_widget_at(chrome.pointer());
        match button {
            MouseButton::Left => chrome.press(&surface.window, over_widget),
            MouseButton::Right => chrome
                .menu(&surface.window, over_widget)
                .then_some(Press::Menu),
            _ => None,
        }
    }

    pub(super) fn apply_window_press(&mut self, press: Press) {
        if !matches!(press, Press::Resize(_)) {
            self.dismiss_window_overlays();
        }
        let Some(window) = self.surface.as_ref().map(|surface| surface.window.clone()) else {
            return;
        };
        match press {
            Press::Drag => {
                let _ = window.drag_window();
            }
            Press::Maximize => window.set_maximized(!window.is_maximized()),
            Press::Resize(direction) => {
                let _ = window.drag_resize_window(direction);
            }
            Press::Menu => {
                let point = self.chrome.pointer();
                window.show_window_menu(LogicalPosition::new(point.x as f64, point.y as f64));
            }
        }
        self.chrome.refresh(&window);
        self.request_redraw();
    }

    pub(super) fn perform_window(&mut self, actions: Vec<WindowAction>) {
        if actions.is_empty() {
            return;
        }
        let Some(window) = self.surface.as_ref().map(|surface| surface.window.clone()) else {
            return;
        };
        for action in actions {
            match action {
                WindowAction::Minimize => window.set_minimized(true),
                WindowAction::Maximize => window.set_maximized(!window.is_maximized()),
                WindowAction::Close => self.closing = true,
            }
        }
        self.chrome.refresh(&window);
        self.request_redraw();
    }

    pub(super) fn sync_window_cursor(&self) {
        let Some(edge) = self.chrome.edge() else {
            return;
        };
        if let Some(surface) = self.surface.as_ref() {
            surface.window.set_cursor(CursorIcon::from(edge));
        }
    }

    pub(super) fn request_close(&mut self, event_loop: &ActiveEventLoop) {
        if self.capture.is_some() {
            event_loop.exit();
            return;
        }
        if self.export.running() {
            self.confirm_export_close();
            return;
        }
        if self.release_document(Resume::Exit) {
            event_loop.exit();
        }
    }

    fn dismiss_window_overlays(&mut self) {
        if let Some(surface) = self.surface.as_mut() {
            surface.ui.close_popups();
        }
        if self.workspace.palette.is_some() {
            self.dispatch(grana_editor::Message::ClosePalette);
        }
    }
}
