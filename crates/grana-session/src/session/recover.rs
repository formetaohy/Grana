use super::*;

use crate::recovery::{Entry as RecoveryEntry, Recovery};
use crate::view::RecoveryView;

const RECOVERY_LIMIT: u64 = 1 << 30;

#[derive(Default)]
pub(super) struct Restore {
    pub(super) project: Option<PathBuf>,
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
        let Some(fingerprint) = self.fingerprint else {
            return false;
        };
        let Some(source) = self.source_ref() else {
            return false;
        };
        let stats = self.store.stats();
        if stats.resident().saturating_add(stats.spilled()) > RECOVERY_LIMIT {
            if !self.recovery.refused() {
                self.recovery.refuse();
                self.message = Some(Message::info(
                    "the analysis is too large for an automatic snapshot, so save the project before closing",
                ));
            }
            return false;
        }
        let target = match &source {
            ProjectSource::File { path, .. } => path.display().to_string(),
            ProjectSource::Remote { url, .. } => url.clone(),
            ProjectSource::Synthetic { name } => name.clone(),
        };
        let Some(scene) = self.scene.as_ref() else {
            return false;
        };
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
        let Some(task) = self.recovery.start(project, entry) else {
            return false;
        };
        self.tasks.start_autosave(task);
        true
    }

    pub fn recover(&mut self) {
        if self.transferring() {
            self.message = Some(Message::error(
                "wait for the running transfer before recovering",
            ));
            return;
        }
        let Some(snapshot) = self.recovery.snapshot() else {
            self.message = Some(Message::error("the recovery snapshot is gone"));
            return;
        };
        let Some(entry) = self.recovery.entry().cloned() else {
            self.message = Some(Message::error("the recovery record is gone"));
            return;
        };
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
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    pub(super) fn accept_open(&mut self) {
        if let Some(restore) = self.restore.take() {
            self.project = restore.project;
            self.baseline = None;
            self.deferred = false;
        } else {
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
