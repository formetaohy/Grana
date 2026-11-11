use std::path::PathBuf;

use grana_session::{Command, Effects};

use super::{App, Resume};
impl App {
    pub(super) fn apply(&mut self, commands: Vec<Command>) -> bool {
        let view = self.view_state();
        self.session.observe_view(view);
        for command in commands {
            if command.replaces_document()
                && !self.release_document(Resume::Command(command.clone()))
            {
                continue;
            }
            let effects = self.session.execute(command);
            if self.react(effects) {
                return true;
            }
        }
        self.validate_mask();
        false
    }

    pub(super) fn react(&mut self, effects: Effects) -> bool {
        if effects.repaint {
            self.request_redraw();
        }
        if effects.open_dataset
            && let Some(path) = Self::pick_folder()
        {
            self.session.begin_load(path);
            return true;
        }
        if effects.open_image
            && let Some(path) = Self::pick_image()
        {
            self.session.begin_load(path);
            return true;
        }
        if effects.open_project
            && let Some(path) = Self::pick_project()
        {
            self.session.begin_project(path);
            return true;
        }
        if let Some(choose) = effects.save_project {
            let view = self.view_state();
            if let Some(path) = self.project_path(choose) {
                self.session.save_project(path, view);
            }
        }
        if effects.snapshot {
            self.begin_snapshot();
        }
        if effects.figure {
            self.begin_figure();
        }
        if effects.chart {
            self.begin_chart();
        }
        if effects.comparison {
            self.begin_comparison();
        }
        if effects.lineage {
            self.begin_lineage();
        }
        if let Some(transparent) = effects.movie {
            self.begin_movie(transparent);
        }
        if effects.sequence {
            self.begin_sequence();
        }
        if effects.cancel_export {
            self.cancel_export();
        }
        if effects.keyframe {
            self.set_keyframe();
        }
        false
    }

    pub(super) fn validate_mask(&mut self) {
        let Some(brush) = self.workspace.mask else {
            return;
        };
        let held = self
            .session
            .scene()
            .is_some_and(|scene| scene.objects.get(brush.set).is_some());
        if !held {
            self.workspace.mask = None;
            self.mask_path.clear();
        }
    }

    pub(super) fn release_document(&mut self, next: Resume) -> bool {
        if self.resume.is_some() {
            return false;
        }
        if self.session.transferring() {
            self.resume = Some(next);
            return false;
        }
        if !self.session.dirty() {
            return true;
        }
        let answer = rfd::MessageDialog::new()
            .set_title("Unsaved analysis")
            .set_description("The analysis has unsaved changes. Save them before continuing?")
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        match answer {
            rfd::MessageDialogResult::No => {
                self.session.discard_recovery();
                true
            }
            rfd::MessageDialogResult::Yes => {
                let view = self.view_state();
                let Some(path) = self.project_path(false) else {
                    return false;
                };
                self.session.save_project(path, view);
                self.resume = Some(next);
                false
            }
            _ => false,
        }
    }

    pub(super) fn follow_resume(&mut self) -> bool {
        if self.resume.is_none() || self.session.transferring() {
            return false;
        }
        if self.session.dirty() {
            self.resume = None;
            return false;
        }
        match self.resume.take() {
            Some(Resume::Exit) => {
                self.exiting = true;
                true
            }
            Some(Resume::Command(command)) => {
                self.apply(vec![command]);
                false
            }
            None => false,
        }
    }

    pub(super) fn pick_folder() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open an OME-Zarr folder")
            .pick_folder()
    }

    pub(super) fn pick_image() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open a microscopy image")
            .add_filter(
                "Microscopy image",
                &[
                    "czi", "nd2", "lif", "ims", "ome.tif", "ome.tiff", "tif", "tiff",
                ],
            )
            .add_filter("ZEISS CZI", &["czi"])
            .add_filter("Nikon ND2", &["nd2"])
            .add_filter("Leica LIF", &["lif"])
            .add_filter("Imaris IMS", &["ims"])
            .add_filter("OME-TIFF", &["ome.tif", "ome.tiff", "tif", "tiff"])
            .pick_file()
    }

    pub(super) fn pick_project() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open a grana project")
            .add_filter("grana project", &["grana"])
            .pick_file()
    }

    pub(super) fn project_path(&self, choose: bool) -> Option<PathBuf> {
        if !choose && let Some(path) = self.session.project() {
            return Some(path.to_path_buf());
        }
        rfd::FileDialog::new()
            .set_title("Save a grana project")
            .set_file_name(self.session.suggested_project())
            .add_filter("grana project", &["grana"])
            .save_file()
    }
}
