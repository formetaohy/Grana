use std::time::{Duration, Instant};

use grana_editor::Panel;
use grana_project::ViewState;

use super::{AUTOSAVE_IDLE, AUTOSAVE_INTERVAL, App};
use crate::preference::Preference;
impl App {
    pub(super) fn sync_preference(&mut self) {
        if self.workspace.language == self.language {
            return;
        }
        self.language = self.workspace.language;
        let preference = Preference {
            language: self.language,
        };
        if let Err(detail) = preference.save() {
            self.output = Some(detail);
        }
        self.request_redraw();
    }

    pub(super) fn sync_plate_panel(&mut self) {
        let identity = self
            .session
            .plate()
            .map(|view| (view.plate.name().to_string(), view.plate.len()));
        if identity == self.plate {
            return;
        }
        self.plate = identity;
        if self.plate.is_some() && self.workspace.panel.is_none() {
            self.workspace.panel = Some(Panel::Plate);
            self.workspace.last_panel = Panel::Plate;
            self.workspace.dock_height = self.workspace.dock_height.max(600.0);
        }
    }

    pub(super) fn view_state(&self) -> ViewState {
        ViewState {
            camera: self.camera.state(),
            timeline: self.session.timeline().clone(),
            export: self.session.export(),
        }
    }

    pub(super) fn apply_view(&mut self, view: &ViewState) {
        self.camera.restore(view.camera);
        self.session.restore_timeline(view.timeline.clone());
        self.workspace.playback.selected = None;
        self.workspace.playback.position = 0.0;
        self.needs_fit = false;
    }

    pub(super) fn dirty(&self) -> bool {
        self.session.dirty()
    }

    pub(super) fn track_changes(&mut self) {
        let revision = self.session.revision();
        if revision != self.revision {
            self.revision = revision;
            self.changed_at = Some(Instant::now());
        }
    }

    pub(super) fn autosave(&mut self) {
        if self.capture.is_some() || self.session.access().viewer() {
            return;
        }
        let now = Instant::now();
        let idle = self
            .changed_at
            .is_none_or(|at| now.duration_since(at) >= AUTOSAVE_IDLE);
        let due = self
            .autosave_at
            .is_none_or(|at| now.duration_since(at) >= AUTOSAVE_INTERVAL);
        if !idle || !due || !self.session.dirty() || self.session.analysing() {
            return;
        }
        self.autosave_at = Some(now);
        let view = self.view_state();
        self.session.autosave(view);
    }

    pub(super) fn autosave_wait(&self) -> Duration {
        let held = self
            .autosave_at
            .map_or(Duration::ZERO, |at| Instant::now().duration_since(at));
        AUTOSAVE_INTERVAL.saturating_sub(held).max(AUTOSAVE_IDLE)
    }

    pub(super) fn sync_title(&mut self) {
        let name = self
            .session
            .project_name()
            .unwrap_or_else(|| grana_i18n::common::untitled(self.workspace.language).to_string());
        let title = if self.session.access().viewer() {
            format!(
                "Grana — {name} · {}",
                grana_i18n::app::viewer(self.workspace.language)
            )
        } else {
            let marker = if self.dirty() { " •" } else { "" };
            format!("Grana — {name}{marker}")
        };
        if self.title.as_deref() == Some(title.as_str()) {
            return;
        }
        self.title = Some(title.clone());
        if let Some(surface) = &self.surface {
            surface.window.set_title(&title);
        }
    }
}
