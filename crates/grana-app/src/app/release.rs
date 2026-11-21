use std::collections::VecDeque;
use std::path::PathBuf;

use grana_editor::{Prompt, PromptChoice};
use grana_session::{Held, Inquiry};

use super::{App, Resume};

pub(super) struct Release {
    steps: VecDeque<Step>,
    next: Resume,
}

pub(super) enum Step {
    Save { series: usize, path: PathBuf },
    Discard { series: usize },
}

pub(super) enum Deferred {
    Unsaved {
        next: Resume,
    },
    Fields {
        held: Vec<Held>,
        index: usize,
        steps: VecDeque<Step>,
        next: Resume,
    },
}

impl App {
    pub(super) fn release_document(&mut self, next: Resume) -> bool {
        if self.session.access().viewer() {
            return true;
        }
        if self.resume.is_some()
            || self.release.is_some()
            || self.deferred.is_some()
            || self.workspace.prompt.is_some()
        {
            return false;
        }
        if self.session.transferring() {
            self.resume = Some(next);
            return false;
        }
        if !next.replaces_document() {
            return true;
        }
        if self.session.dirty() && !self.dirty_released {
            self.deferred = Some(Deferred::Unsaved { next });
            self.present_prompt(Prompt::Unsaved);
            return false;
        }
        self.release_held(next)
    }

    fn release_held(&mut self, next: Resume) -> bool {
        let held = if next.replaces_dataset() {
            self.session.held()
        } else {
            Vec::new()
        };
        if held.is_empty() {
            return true;
        }
        self.begin_fields(held, next);
        false
    }

    pub(super) fn begin_fields(&mut self, held: Vec<Held>, next: Resume) {
        self.deferred = Some(Deferred::Fields {
            held,
            index: 0,
            steps: VecDeque::new(),
            next,
        });
        self.ask_field();
    }

    fn ask_field(&mut self) {
        let field = match self.deferred.as_ref() {
            Some(Deferred::Fields { held, index, .. }) => held
                .get(*index)
                .map(|field| (field.name.clone(), field.savable)),
            _ => return,
        };
        match field {
            Some((name, savable)) => self.present_prompt(Prompt::UnsavedField { name, savable }),
            None => self.finish_fields(),
        }
    }

    fn finish_fields(&mut self) {
        let Some(Deferred::Fields {
            held: _,
            index: _,
            steps,
            next,
        }) = self.deferred.take()
        else {
            return;
        };
        self.release = Some(Release { steps, next });
        if let Ok(next) = self.drain_release() {
            self.resume = Some(next);
            self.follow_resume();
        }
    }

    pub(super) fn resolve_prompt(&mut self, choice: PromptChoice) {
        let Some(deferred) = self.deferred.take() else {
            return;
        };
        match deferred {
            Deferred::Unsaved { next } => match choice {
                PromptChoice::Save => self.save_document(next),
                PromptChoice::Discard => {
                    self.dirty_released = true;
                    if self.release_held(next.clone()) {
                        self.perform_resume(next);
                    }
                }
                PromptChoice::Cancel | PromptChoice::Acknowledge => self.dirty_released = false,
            },
            Deferred::Fields {
                held,
                index,
                mut steps,
                next,
            } => {
                if index >= held.len() {
                    self.deferred = Some(Deferred::Fields {
                        held,
                        index,
                        steps,
                        next,
                    });
                    self.finish_fields();
                    self.request_redraw();
                    return;
                }
                let field = &held[index];
                match choice {
                    PromptChoice::Save => {
                        let series = field.series;
                        let name = field.name.clone();
                        let project = field.project.clone();
                        self.deferred = Some(Deferred::Fields {
                            held,
                            index,
                            steps,
                            next,
                        });
                        self.ask_field_project(&name, project, move |app, path| {
                            let step = path.map(|path| Step::Save { series, path });
                            app.advance_field(step);
                        });
                        return;
                    }
                    PromptChoice::Discard => steps.push_back(Step::Discard {
                        series: field.series,
                    }),
                    PromptChoice::Cancel | PromptChoice::Acknowledge => {
                        self.dirty_released = false;
                        self.request_redraw();
                        return;
                    }
                }
                self.deferred = Some(Deferred::Fields {
                    held,
                    index: index + 1,
                    steps,
                    next,
                });
                self.ask_field();
            }
        }
        self.request_redraw();
    }

    fn advance_field(&mut self, step: Option<Step>) {
        let Some(Deferred::Fields {
            held,
            index,
            mut steps,
            next,
        }) = self.deferred.take()
        else {
            return;
        };
        match step {
            Some(step) => {
                steps.push_back(step);
                self.deferred = Some(Deferred::Fields {
                    held,
                    index: index + 1,
                    steps,
                    next,
                });
                self.ask_field();
            }
            None => {
                self.deferred = Some(Deferred::Fields {
                    held,
                    index,
                    steps,
                    next,
                });
                self.ask_field();
            }
        }
        self.request_redraw();
    }

    fn save_document(&mut self, next: Resume) {
        let view = self.view_state();
        self.ask_project_path(false, move |app, path| {
            let Some(path) = path else {
                app.deferred = Some(Deferred::Unsaved { next });
                app.present_prompt(Prompt::Unsaved);
                return;
            };
            app.session.save_project(path, view);
            app.resume = Some(next);
        });
    }

    fn ask_field_project(
        &mut self,
        name: &str,
        project: Option<PathBuf>,
        answer: impl FnOnce(&mut App, Option<PathBuf>) + 'static,
    ) {
        if let Some(project) = project {
            answer(self, Some(project));
            return;
        }
        let language = self.workspace.language;
        self.ask(
            Inquiry::save(
                grana_i18n::app::save_grana_project(language),
                format!(
                    "{}.{}",
                    grana_session::slug(name),
                    grana_session::PROJECT_EXTENSION
                ),
            )
            .filter(
                grana_i18n::app::grana_project(language),
                &[grana_session::PROJECT_EXTENSION],
            ),
            answer,
        );
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
        self.dirty_released = false;
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
        self.perform_resume(next)
    }

    fn perform_resume(&mut self, next: Resume) -> bool {
        self.dirty_released = false;
        match next {
            Resume::Exit => {
                self.exiting = true;
                true
            }
            Resume::Update => match self.update.apply() {
                Some(Ok(())) => {
                    self.exiting = true;
                    true
                }
                Some(Err(error)) => {
                    self.update.fail(&error);
                    self.sync_update();
                    self.request_redraw();
                    false
                }
                None => false,
            },
            Resume::Command(command) => {
                let view = self.view_state();
                self.session.observe_view(view);
                self.execute(command);
                self.validate_mask();
                false
            }
        }
    }

    pub(super) fn present_prompt(&mut self, prompt: Prompt) {
        if let Some(surface) = self.surface.as_mut() {
            surface.ui.close_popups();
        }
        self.workspace.settings.open = false;
        self.workspace.remote_open = false;
        self.workspace.prompt = Some(prompt);
        self.request_redraw();
    }

    pub(super) fn notice_export(&mut self) {
        self.present_prompt(Prompt::ExportRunning);
    }
}
