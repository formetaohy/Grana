use std::time::{Duration, Instant};

use glam::Vec2;
use winit::dpi::LogicalSize;
use winit::window::Window;

use super::surface::Surface;
use super::{EDITOR_MINIMUM, EDITOR_SIZE};

const REVEAL_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fit {
    Startup,
    Editor,
}

#[derive(Clone, Copy)]
struct Pending {
    target: [u32; 2],
    since: Instant,
}

pub(super) struct Placement {
    fit: Fit,
    panel: Option<Vec2>,
    editor: bool,
    pending: Option<Pending>,
    visible: bool,
}

impl Placement {
    pub(super) fn new(fit: Fit) -> Self {
        Self {
            fit,
            panel: None,
            editor: false,
            pending: None,
            visible: false,
        }
    }

    pub(super) fn fit(&self) -> Fit {
        self.fit
    }

    pub(super) fn shown(&self) -> bool {
        self.visible
    }

    pub(super) fn settled(&self, surface: &Surface) -> bool {
        let inner = surface.window.inner_size();
        if inner.width == 0 || inner.height == 0 {
            return true;
        }
        if surface.size().physical() != [inner.width, inner.height] {
            return false;
        }
        let Some(pending) = self.pending else {
            return true;
        };
        if pending.since.elapsed() >= REVEAL_TIMEOUT {
            return true;
        }
        [inner.width, inner.height] == pending.target
    }

    pub(super) fn place(&mut self, fit: Fit, panel: Option<Vec2>, window: &Window) {
        if fit != self.fit {
            self.fit = fit;
            self.panel = None;
            self.editor = false;
            self.pending = None;
        }
        match fit {
            Fit::Startup => {
                let Some(panel) = panel else {
                    return;
                };
                if self.panel == Some(panel) {
                    return;
                }
                self.panel = Some(panel);
                let size = LogicalSize::new(panel.x as f64, panel.y as f64);
                window.set_min_inner_size(Some(size));
                self.request(window, size);
            }
            Fit::Editor => {
                if self.editor {
                    return;
                }
                self.editor = true;
                window.set_min_inner_size(Some(EDITOR_MINIMUM));
                let current = window.inner_size().to_logical::<f64>(window.scale_factor());
                let size = LogicalSize::new(
                    current.width.max(EDITOR_SIZE.width),
                    current.height.max(EDITOR_SIZE.height),
                );
                if size != current {
                    self.request(window, size);
                }
            }
        }
    }

    pub(super) fn reveal(&mut self, surface: &Surface, capture: bool) {
        if self.visible {
            self.pending = None;
            return;
        }
        if capture {
            self.pending = None;
            return;
        }
        let inner = surface.window.inner_size();
        if surface.size().physical() != [inner.width, inner.height] {
            return;
        }
        if let Some(pending) = self.pending {
            if [inner.width, inner.height] != pending.target
                && pending.since.elapsed() < REVEAL_TIMEOUT
            {
                return;
            }
            self.pending = None;
        }
        surface.window.set_visible(true);
        self.visible = true;
    }

    pub(super) fn wait(&self, surface: &Surface) -> Option<Duration> {
        if self.visible {
            return None;
        }
        let pending = self.pending?;
        let inner = surface.window.inner_size();
        if surface.size().physical() != [inner.width, inner.height] {
            return Some(Duration::ZERO);
        }
        Some(REVEAL_TIMEOUT.saturating_sub(pending.since.elapsed()))
    }

    fn request(&mut self, window: &Window, size: LogicalSize<f64>) {
        let target = size.to_physical::<u32>(window.scale_factor());
        let _ = window.request_inner_size(size);
        self.pending = Some(Pending {
            target: [target.width, target.height],
            since: Instant::now(),
        });
        window.request_redraw();
    }
}
