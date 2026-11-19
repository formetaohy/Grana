use std::collections::VecDeque;
use std::path::PathBuf;

use grana_session::{Command, Effects, Held, Transfer};

use super::{App, Resume};

pub(super) struct Release {
    steps: VecDeque<Step>,
    next: Resume,
}

enum Step {
    Save { series: usize, path: PathBuf },
    Discard { series: usize },
}

impl App {
    pub(super) fn apply(&mut self, commands: Vec<Command>) -> bool {
        if commands.is_empty() {
            self.validate_mask();
            return false;
        }
        let view = self.view_state();
        self.session.observe_view(view);
        for command in commands {
            let resume = Resume::Command(command.clone());
            if command.replaces_document() && !self.release_document(resume.clone()) {
                continue;
            }
            let effects = self.session.execute(command);
            if !effects.held.is_empty() {
                if let Some(steps) = self.ask_fields(effects.held) {
                    self.release = Some(Release {
                        steps,
                        next: resume,
                    });
                    if let Ok(next) = self.drain_release() {
                        self.resume = Some(next);
                    }
                }
                self.request_redraw();
                return true;
            }
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
        if effects.open_any
            && let Some(path) = self.pick_any()
        {
            let transfer = self.session.open_path(path);
            self.absorb(transfer);
            return true;
        }
        if effects.open_dataset
            && let Some(path) = self.pick_folder()
        {
            let transfer = self.session.open_path(path);
            self.absorb(transfer);
            return true;
        }
        if effects.open_image
            && let Some(path) = self.pick_image()
        {
            let transfer = self.session.begin_load(path);
            self.absorb(transfer);
            return true;
        }
        if effects.open_tiles
            && let Some(path) = self.pick_tiles()
        {
            let transfer = self.session.open_path(path);
            self.absorb(transfer);
            return true;
        }
        if effects.open_project
            && let Some(path) = self.pick_project()
        {
            let transfer = self.session.begin_project(path);
            self.absorb(transfer);
            return true;
        }
        if effects.open_package
            && let Some(path) = self.pick_package()
        {
            let transfer = self.session.begin_package(path);
            self.absorb(transfer);
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

    fn absorb(&mut self, transfer: Transfer) {
        if let Transfer::Held(held) = transfer {
            self.report_held(held);
        }
    }

    fn report_held(&mut self, held: Vec<Held>) {
        let names = held
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        self.output = Some(format!(
            "save or discard the unsaved analysis of {names} before opening another document"
        ));
        self.request_redraw();
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
        if self.session.access().viewer() {
            return true;
        }
        if self.resume.is_some() || self.release.is_some() {
            return false;
        }
        if self.session.transferring() {
            self.resume = Some(next);
            return false;
        }
        if !next.replaces_document() {
            return true;
        }
        if self.session.dirty() {
            let language = self.workspace.language;
            let answer = rfd::MessageDialog::new()
                .set_title(grana_i18n::app::unsaved_analysis(language))
                .set_description(grana_i18n::app::unsaved_detail(language))
                .set_level(rfd::MessageLevel::Warning)
                .set_buttons(rfd::MessageButtons::YesNoCancel)
                .show();
            match answer {
                rfd::MessageDialogResult::No => {}
                rfd::MessageDialogResult::Yes => {
                    let view = self.view_state();
                    let Some(path) = self.project_path(false) else {
                        return false;
                    };
                    self.session.save_project(path, view);
                    self.resume = Some(next);
                    return false;
                }
                _ => return false,
            }
        }
        let held = if next.replaces_dataset() {
            self.session.held()
        } else {
            Vec::new()
        };
        if held.is_empty() {
            return true;
        }
        let Some(steps) = self.ask_fields(held) else {
            return false;
        };
        self.release = Some(Release { steps, next });
        self.drain_release().is_ok()
    }

    fn ask_fields(&mut self, held: Vec<Held>) -> Option<VecDeque<Step>> {
        let language = self.workspace.language;
        let mut steps = VecDeque::new();
        for field in held {
            if !field.savable {
                let answer = rfd::MessageDialog::new()
                    .set_title(grana_i18n::app::unsaved_analysis(language))
                    .set_description(grana_i18n::app::unsaved_unsavable(language, &field.name))
                    .set_level(rfd::MessageLevel::Warning)
                    .set_buttons(rfd::MessageButtons::OkCancel)
                    .show();
                if !matches!(answer, rfd::MessageDialogResult::Ok) {
                    return None;
                }
                steps.push_back(Step::Discard {
                    series: field.series,
                });
                continue;
            }
            let answer = rfd::MessageDialog::new()
                .set_title(grana_i18n::app::unsaved_analysis(language))
                .set_description(grana_i18n::app::unsaved_field(language, &field.name))
                .set_level(rfd::MessageLevel::Warning)
                .set_buttons(rfd::MessageButtons::YesNoCancel)
                .show();
            match answer {
                rfd::MessageDialogResult::No => steps.push_back(Step::Discard {
                    series: field.series,
                }),
                rfd::MessageDialogResult::Yes => {
                    let path = self.field_project(&field)?;
                    steps.push_back(Step::Save {
                        series: field.series,
                        path,
                    });
                }
                _ => return None,
            }
        }
        Some(steps)
    }

    fn field_project(&self, field: &Held) -> Option<PathBuf> {
        if let Some(path) = field.project.as_ref() {
            return Some(path.clone());
        }
        let language = self.workspace.language;
        rfd::FileDialog::new()
            .set_title(grana_i18n::app::save_grana_project(language))
            .set_file_name(format!(
                "{}.{}",
                grana_session::slug(&field.name),
                grana_session::PROJECT_EXTENSION
            ))
            .add_filter(
                grana_i18n::app::grana_project(language),
                &[grana_session::PROJECT_EXTENSION],
            )
            .save_file()
    }

    fn run_step(&mut self, step: Step) -> bool {
        match step {
            Step::Discard { series } => {
                if self.session.discard_field(series) {
                    return false;
                }
            }
            Step::Save { series, path } => {
                if self.session.save_field(series, path) {
                    return true;
                }
            }
        }
        self.release = None;
        true
    }

    fn drain_release(&mut self) -> Result<Resume, ()> {
        while let Some(step) = self
            .release
            .as_mut()
            .and_then(|release| release.steps.pop_front())
        {
            if self.run_step(step) {
                return Err(());
            }
        }
        let release = self
            .release
            .take()
            .expect("a drained release carries the transfer it defers");
        Ok(release.next)
    }

    pub(super) fn follow_resume(&mut self) -> bool {
        if self.session.transferring() {
            return false;
        }
        if self.release.is_some() {
            match self.drain_release() {
                Ok(next) => self.resume = Some(next),
                Err(()) => return false,
            }
        }
        let Some(next) = self.resume.take() else {
            return false;
        };
        if !self.release_document(next.clone()) {
            self.request_redraw();
            return false;
        }
        match next {
            Resume::Exit => {
                self.exiting = true;
                true
            }
            Resume::Command(command) => {
                self.apply(vec![command]);
                false
            }
        }
    }

    pub(super) fn pick_folder(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(grana_i18n::app::open_folder(self.workspace.language))
            .pick_folder()
    }

    pub(super) fn pick_any(&self) -> Option<PathBuf> {
        let language = self.workspace.language;
        rfd::FileDialog::new()
            .set_title(grana_i18n::app::open_anything(language))
            .add_filter(
                grana_i18n::app::supported_files(language),
                &[
                    "grana", "czi", "nd2", "lif", "ims", "ome.tif", "ome.tiff", "tif", "tiff",
                ],
            )
            .add_filter(grana_i18n::app::grana_project(language), &["grana"])
            .add_filter(
                grana_i18n::app::microscopy_image(language),
                &[
                    "czi", "nd2", "lif", "ims", "ome.tif", "ome.tiff", "tif", "tiff",
                ],
            )
            .pick_file()
    }

    pub(super) fn pick_tiles(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(grana_i18n::app::open_tile_folder(self.workspace.language))
            .pick_folder()
    }

    pub(super) fn pick_image(&self) -> Option<PathBuf> {
        let language = self.workspace.language;
        rfd::FileDialog::new()
            .set_title(grana_i18n::app::open_microscopy_image(language))
            .add_filter(
                grana_i18n::app::microscopy_image(language),
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

    pub(super) fn pick_project(&self) -> Option<PathBuf> {
        let language = self.workspace.language;
        rfd::FileDialog::new()
            .set_title(grana_i18n::app::open_grana_project(language))
            .add_filter(
                grana_i18n::app::grana_project(language),
                &[grana_session::PROJECT_EXTENSION],
            )
            .pick_file()
    }

    pub(super) fn pick_package(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(grana_i18n::app::open_grana_package(self.workspace.language))
            .pick_folder()
    }

    pub(super) fn project_path(&self, choose: bool) -> Option<PathBuf> {
        if !choose && let Some(path) = self.session.project() {
            return Some(path.to_path_buf());
        }
        let language = self.workspace.language;
        rfd::FileDialog::new()
            .set_title(grana_i18n::app::save_grana_project(language))
            .set_file_name(self.session.suggested_project())
            .add_filter(
                grana_i18n::app::grana_project(language),
                &[grana_session::PROJECT_EXTENSION],
            )
            .save_file()
    }
}
