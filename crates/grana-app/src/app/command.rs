use std::path::PathBuf;

use grana_session::{Command, Effects, Held, Inquiry, Transfer};

use super::{App, Resume};

impl App {
    pub(super) fn apply(&mut self, commands: Vec<Command>) -> bool {
        if commands.is_empty() {
            self.validate_mask();
            return false;
        }
        let view = self.view_state();
        self.session.observe_view(view);
        for command in commands {
            if command.replaces_document()
                && !self.release_document(Resume::Command(command.clone()))
            {
                return false;
            }
            if self.execute(command) {
                return true;
            }
            if self.session.inquiry_pending() {
                self.poll_inquiry();
                return true;
            }
        }
        self.validate_mask();
        false
    }

    pub(super) fn execute(&mut self, command: Command) -> bool {
        let resume = Resume::Command(command.clone());
        let effects = self.session.execute(command);
        if !effects.held.is_empty() {
            self.begin_fields(effects.held, resume);
            self.request_redraw();
            return true;
        }
        self.react(effects)
    }

    pub(super) fn react(&mut self, effects: Effects) -> bool {
        if effects.repaint {
            self.request_redraw();
        }
        if effects.open_any {
            self.ask_any();
            return true;
        }
        if effects.open_dataset {
            self.ask_dataset();
            return true;
        }
        if effects.open_image {
            self.ask_image();
            return true;
        }
        if effects.open_tiles {
            self.ask_tiles();
            return true;
        }
        if effects.open_project {
            self.ask_project();
            return true;
        }
        if effects.open_package {
            self.ask_package();
            return true;
        }
        if let Some(choose) = effects.save_project {
            self.ask_project_path(choose, |app, path| {
                let Some(path) = path else {
                    return;
                };
                let view = app.view_state();
                app.session.save_project(path, view);
            });
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

    fn ask_any(&mut self) {
        let language = self.workspace.language;
        self.ask(
            Inquiry::choose(grana_i18n::app::open_anything(language))
                .filter(
                    grana_i18n::app::supported_files(language),
                    &[
                        "grana", "czi", "nd2", "lif", "ims", "ome.tif", "ome.tiff", "tif", "tiff",
                    ],
                )
                .filter(grana_i18n::app::grana_project(language), &["grana"])
                .filter(
                    grana_i18n::app::microscopy_image(language),
                    &[
                        "czi", "nd2", "lif", "ims", "ome.tif", "ome.tiff", "tif", "tiff",
                    ],
                ),
            |app, path| {
                if let Some(path) = path {
                    let transfer = app.session.open_path(path);
                    app.absorb(transfer);
                }
            },
        );
    }

    fn ask_dataset(&mut self) {
        self.ask(
            Inquiry::folder(grana_i18n::app::open_folder(self.workspace.language)),
            |app, path| {
                if let Some(path) = path {
                    let transfer = app.session.open_path(path);
                    app.absorb(transfer);
                }
            },
        );
    }

    fn ask_tiles(&mut self) {
        self.ask(
            Inquiry::folder(grana_i18n::app::open_tile_folder(self.workspace.language)),
            |app, path| {
                if let Some(path) = path {
                    let transfer = app.session.open_path(path);
                    app.absorb(transfer);
                }
            },
        );
    }

    fn ask_image(&mut self) {
        let language = self.workspace.language;
        self.ask(
            Inquiry::choose(grana_i18n::app::open_microscopy_image(language))
                .filter(
                    grana_i18n::app::microscopy_image(language),
                    &[
                        "czi", "nd2", "lif", "ims", "ome.tif", "ome.tiff", "tif", "tiff",
                    ],
                )
                .filter("ZEISS CZI", &["czi"])
                .filter("Nikon ND2", &["nd2"])
                .filter("Leica LIF", &["lif"])
                .filter("Imaris IMS", &["ims"])
                .filter("OME-TIFF", &["ome.tif", "ome.tiff", "tif", "tiff"]),
            |app, path| {
                if let Some(path) = path {
                    let transfer = app.session.begin_load(path);
                    app.absorb(transfer);
                }
            },
        );
    }

    fn ask_project(&mut self) {
        let language = self.workspace.language;
        self.ask(
            Inquiry::choose(grana_i18n::app::open_grana_project(language)).filter(
                grana_i18n::app::grana_project(language),
                &[grana_session::PROJECT_EXTENSION],
            ),
            |app, path| {
                if let Some(path) = path {
                    let transfer = app.session.begin_project(path);
                    app.absorb(transfer);
                }
            },
        );
    }

    fn ask_package(&mut self) {
        self.ask(
            Inquiry::folder(grana_i18n::app::open_grana_package(self.workspace.language)),
            |app, path| {
                if let Some(path) = path {
                    let transfer = app.session.begin_package(path);
                    app.absorb(transfer);
                }
            },
        );
    }

    pub(super) fn ask_project_path(
        &mut self,
        choose: bool,
        answer: impl FnOnce(&mut App, Option<PathBuf>) + 'static,
    ) {
        if !choose && let Some(path) = self.session.project() {
            answer(self, Some(path.to_path_buf()));
            return;
        }
        let language = self.workspace.language;
        let suggested = self.session.suggested_project();
        self.ask(
            Inquiry::save(grana_i18n::app::save_grana_project(language), suggested).filter(
                grana_i18n::app::grana_project(language),
                &[grana_session::PROJECT_EXTENSION],
            ),
            answer,
        );
    }
}
