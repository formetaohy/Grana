use std::time::{Duration, Instant};

use glam::Vec2;
use grana_editor::ChromeView;
use grana_ui::Rect;
use winit::event::WindowEvent;
use winit::window::{CursorIcon, ResizeDirection, Window, WindowAttributes};

const RESIZE_BORDER: f32 = 6.0;
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_SLOP: f32 = 4.0;
const TRAFFIC_LIGHTS: f32 = 78.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Drag,
    Maximize,
    Resize(ResizeDirection),
    Menu,
}

pub struct Chrome {
    pointer: Vec2,
    edge: Option<ResizeDirection>,
    click: Option<(Instant, Vec2)>,
}

impl Chrome {
    pub fn new() -> Self {
        Self {
            pointer: Vec2::ZERO,
            edge: None,
            click: None,
        }
    }

    pub fn view(&self, window: &Window) -> ChromeView {
        ChromeView {
            maximized: window.is_maximized(),
            controls: custom_controls(),
            leading: leading(),
        }
    }

    pub fn pointer(&self) -> Vec2 {
        self.pointer
    }

    pub fn edge(&self) -> Option<ResizeDirection> {
        self.edge
    }

    pub fn track(&mut self, window: &Window, event: &WindowEvent) -> Option<CursorIcon> {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let scale = window.scale_factor() as f32;
                self.pointer = Vec2::new(position.x as f32, position.y as f32) / scale;
                self.refresh(window);
                self.edge.map(CursorIcon::from)
            }
            WindowEvent::CursorLeft { .. } => {
                self.edge = None;
                None
            }
            _ => None,
        }
    }

    pub fn press(&mut self, window: &Window, over_widget: bool) -> Option<Press> {
        if over_widget {
            self.click = None;
            return None;
        }
        if let Some(edge) = self.edge {
            return Some(Press::Resize(edge));
        }
        if !title_band(window).contains(self.pointer) {
            self.click = None;
            return None;
        }
        let now = Instant::now();
        let point = screen_pointer(window, self.pointer);
        let doubled = self.click.is_some_and(|(at, last)| {
            now.saturating_duration_since(at) < DOUBLE_CLICK_INTERVAL
                && (point - last).length() <= DOUBLE_CLICK_SLOP
        });
        self.click = (!doubled).then_some((now, point));
        Some(if doubled {
            Press::Maximize
        } else {
            Press::Drag
        })
    }

    pub fn menu(&self, window: &Window, over_widget: bool) -> bool {
        !over_widget && title_band(window).contains(self.pointer)
    }

    pub fn refresh(&mut self, window: &Window) {
        self.edge = resize_edge(window, self.pointer);
    }
}

impl Default for Chrome {
    fn default() -> Self {
        Self::new()
    }
}

fn title_band(window: &Window) -> Rect {
    grana_editor::title_band(logical_size(window))
}

fn logical_size(window: &Window) -> Vec2 {
    let size = window.inner_size();
    let scale = window.scale_factor() as f32;
    Vec2::new(size.width as f32, size.height as f32) / scale
}

fn screen_pointer(window: &Window, pointer: Vec2) -> Vec2 {
    let scale = window.scale_factor() as f32;
    window.outer_position().map_or(pointer, |position| {
        Vec2::new(position.x as f32, position.y as f32) / scale + pointer
    })
}

fn resize_edge(window: &Window, point: Vec2) -> Option<ResizeDirection> {
    if !custom_controls() || window.is_maximized() {
        return None;
    }
    let size = logical_size(window);
    let west = point.x <= RESIZE_BORDER;
    let east = point.x >= size.x - RESIZE_BORDER;
    let north = point.y <= RESIZE_BORDER;
    let south = point.y >= size.y - RESIZE_BORDER;
    let direction = match (west, east, north, south) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::West,
        (_, true, _, _) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return None,
    };
    Some(direction)
}

fn custom_controls() -> bool {
    cfg!(target_os = "windows")
}

fn leading() -> f32 {
    if cfg!(target_os = "macos") {
        TRAFFIC_LIGHTS
    } else {
        0.0
    }
}

#[cfg(target_os = "windows")]
pub fn attributes(attributes: WindowAttributes) -> WindowAttributes {
    use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
    attributes
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_corner_preference(CornerPreference::Round)
}

#[cfg(target_os = "macos")]
pub fn attributes(attributes: WindowAttributes) -> WindowAttributes {
    use winit::platform::macos::WindowAttributesExtMacOS;
    attributes
        .with_titlebar_transparent(true)
        .with_fullsize_content_view(true)
        .with_title_hidden(true)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes
}
