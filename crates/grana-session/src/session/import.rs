use super::*;

use grana_pipeline::run::{ImportRequest, InteropRequest};
use grana_scene::Provenance;

impl Session {
    pub(super) fn import_labels(&mut self, label: &str) {
        if self.loading.is_some() {
            return;
        }
        if !self.analysable() {
            return;
        }
        if !self.label_images.iter().any(|name| name == label) {
            self.message = Some(Message::error(format!(
                "the dataset holds no label image named {label}"
            )));
            return;
        }
        let set = match self.scene.as_mut() {
            Some(scene) => scene.import_object(label),
            None => return,
        };
        self.start_import(set);
    }

    pub(super) fn start_import(&mut self, set: ObjectSetId) {
        if self.loading.is_some() {
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job().installs(set))
        {
            return;
        }
        let Some(channels) = self.source.as_ref().and_then(Loader::channels) else {
            self.message = Some(Message::error("the dataset is still opening"));
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let provenance = object.provenance().clone();
        let drift = scene.drift().clone();
        self.message = None;
        match provenance {
            Provenance::Imported { label } => {
                self.record_progress(Job::Import(set), 0, 0, 0);
                self.analysis.push(crate::import(
                    Job::Import(set),
                    channels,
                    ImportRequest::new(&label, drift),
                ));
            }
            Provenance::Interop { file } => {
                self.record_progress(Job::Import(set), 0, 0, 0);
                self.analysis.push(crate::interop(
                    Job::Import(set),
                    channels,
                    InteropRequest::new(&file, drift),
                ));
            }
            Provenance::Detection
            | Provenance::Segmented { .. }
            | Provenance::Filtered { .. }
            | Provenance::Copied { .. }
            | Provenance::Edited { .. }
            | Provenance::Merged { .. } => {
                self.message = Some(Message::error(
                    "only an imported object set reads a label image or a track file",
                ));
            }
        }
    }
}
