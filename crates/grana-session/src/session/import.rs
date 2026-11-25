use super::*;

use std::path::Path;

use grana_pipeline::run::{ImportRequest, InteropRequest, LocalizationRequest};
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
        let channel = object.channel();
        let ids = object.id_source();
        let drifts = scene.drifts().clone();
        let alignment = grana_scene::LocalizationAlignment::new(
            channel,
            scene.drift(channel).cloned().unwrap_or_default(),
            scene.registration().clone(),
        );
        let Some(input) = self.import_input(set) else {
            self.message = Some(Message::error(
                "only an imported object set reads a label image or a track file",
            ));
            return;
        };
        let fallback = self
            .scene
            .as_ref()
            .and_then(|scene| scene.object_state(set));
        self.message = None;
        match provenance {
            Provenance::Imported { label } => {
                self.record_progress(Job::Import(set), 0, 0, 0);
                self.analysis.push(
                    crate::import(
                        Job::Import(set),
                        channels,
                        ImportRequest::new(channel, &label, drifts, ids.clone()),
                    )
                    .keyed(input)
                    .with_fallback(fallback),
                );
            }
            Provenance::Interop { file } => {
                self.record_progress(Job::Import(set), 0, 0, 0);
                if localization_table(&file) {
                    self.analysis.push(
                        crate::localize(
                            Job::Import(set),
                            channels,
                            LocalizationRequest::new(&file, alignment, ids.clone()),
                        )
                        .keyed(input)
                        .with_fallback(fallback),
                    );
                } else {
                    self.analysis.push(
                        crate::interop(
                            Job::Import(set),
                            channels,
                            InteropRequest::new(&file, drifts, Arc::clone(&self.store)),
                        )
                        .keyed(input)
                        .with_fallback(fallback),
                    );
                }
            }
            Provenance::Detection
            | Provenance::Segmented { .. }
            | Provenance::Attached { .. }
            | Provenance::Filtered { .. }
            | Provenance::Copied { .. }
            | Provenance::Edited { .. }
            | Provenance::Masked { .. }
            | Provenance::Merged { .. } => {
                self.message = Some(Message::error(
                    "only an imported object set reads a label image or a track file",
                ));
            }
        }
    }
}

pub(super) fn localization_table(file: &str) -> bool {
    let path = Path::new(file);
    path.is_file() && grana_readout::localization::has_extension(path)
}
