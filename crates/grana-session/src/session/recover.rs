use super::*;

use crate::recovery::{Entry as RecoveryEntry, Recovery};
use crate::view::RecoveryView;

const RECOVERY_LIMIT: u64 = 1 << 30;

#[derive(Default)]
pub(super) struct Restore {
    pub(super) project: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Restored {
    Fresh,
    Field,
    Mismatch,
}

impl Session {
    pub fn attach_recovery_store(&mut self) {
        self.recovery = Recovery::persistent();
    }

    pub fn attach_recovery_at(&mut self, root: PathBuf) {
        self.recovery = Recovery::at(Some(root));
    }

    pub fn recovery(&self) -> Option<RecoveryView<'_>> {
        let entry = self.recovery.entry()?;
        self.recovery.snapshot()?;
        Some(RecoveryView {
            name: entry.name(),
            detail: entry.project().map_or_else(
                || entry.target().to_string(),
                |path| path.display().to_string(),
            ),
            age: entry.age(),
        })
    }

    pub fn autosave(&mut self, view: ViewState) -> bool {
        if self.access().viewer() {
            return false;
        }
        if self.tasks.autosaving()
            || self.loading.is_some()
            || self.tasks.transferring()
            || !self.dirty()
        {
            return false;
        }
        let stats = self.store.stats();
        if stats.resident().saturating_add(stats.spilled()) > RECOVERY_LIMIT {
            if !self.recovery.refused() {
                self.recovery.refuse();
                grana_log::warn!(target: "grana::session", "the analysis is too large for an automatic recovery snapshot");
                self.message = Some(Message::info(
                    "the analysis is too large for an automatic snapshot, so save the project before closing",
                ));
            }
            return false;
        }
        let Some((project, entry)) = self.recovery_document(view) else {
            return false;
        };
        let Some(task) = self.recovery.start(project, entry) else {
            return false;
        };
        self.tasks.start_autosave(task);
        true
    }

    pub fn autosave_now(&mut self, view: ViewState) -> Result<(), String> {
        if self.access().viewer() || !self.dirty() {
            return Ok(());
        }
        let Some((project, entry)) = self.recovery_document(view) else {
            return Err("the analysis holds no recoverable document".to_string());
        };
        self.tasks.cancel_autosave();
        match self.recovery.write(project, entry) {
            Ok(()) => Ok(()),
            Err(detail) => {
                grana_log::error!(target: "grana::session", %detail, "the recovery snapshot could not be written");
                Err(detail)
            }
        }
    }

    fn recovery_document(&self, view: ViewState) -> Option<(Project, RecoveryEntry)> {
        if !self.dirty() {
            return None;
        }
        let fingerprint = self.fingerprint?;
        let source = self.source_ref()?;
        let target = match &source {
            ProjectSource::File { path, .. } => path.display().to_string(),
            ProjectSource::Remote { url, .. } => url.clone(),
            ProjectSource::Synthetic { name } => name.clone(),
        };
        let scene = self.scene.as_ref()?;
        let name = scene.dataset.name().to_string();
        let project = Project::capture(
            scene,
            &self.measure,
            source,
            view,
            self.views.clone(),
            fingerprint,
        );
        let entry = RecoveryEntry::new(&name, &target, self.project.as_deref());
        Some((project, entry))
    }

    pub fn recover(&mut self) {
        if self.transferring() {
            grana_log::warn!(target: "grana::session", "a recovery was requested while a transfer is running");
            self.message = Some(Message::error(
                "wait for the running transfer before recovering",
            ));
            return;
        }
        let Some(snapshot) = self.recovery.snapshot() else {
            grana_log::error!(target: "grana::session", "the recovery snapshot is gone");
            self.message = Some(Message::error("the recovery snapshot is gone"));
            return;
        };
        let Some(entry) = self.recovery.entry().cloned() else {
            grana_log::error!(target: "grana::session", "the recovery record is gone");
            self.message = Some(Message::error("the recovery record is gone"));
            return;
        };
        grana_log::info!(target: "grana::session", dataset = entry.name(), "recovering an automatic snapshot");
        self.restore = Some(Restore {
            project: entry.project().map(Path::to_path_buf),
        });
        self.begin_project(snapshot);
    }

    pub fn discard_recovery(&mut self) {
        self.restore = None;
        self.tasks.cancel_autosave();
        self.recovery.discard();
    }

    pub(super) fn finish_autosave(&mut self, outcome: Result<RecoveryEntry, String>) {
        match outcome {
            Ok(entry) => self.recovery.accept(entry),
            Err(detail) => {
                grana_log::error!(target: "grana::session", %detail, "the recovery snapshot failed");
                self.message = Some(Message::error(detail));
            }
        }
    }

    pub(super) fn accept_open(&mut self, restored: Restored) {
        if restored == Restored::Mismatch {
            self.restore = None;
            self.baseline = self.document_hash();
            return;
        }
        if let Some(restore) = self.restore.take() {
            self.project = restore.project;
            self.baseline = None;
            self.deferred = false;
        } else if restored == Restored::Fresh {
            self.baseline = self.document_hash();
        }
        self.recovery.discard();
        self.tasks.cancel_autosave();
    }

    pub(super) fn clear_recovery(&mut self) {
        self.recovery.discard();
        self.tasks.cancel_autosave();
    }
}
