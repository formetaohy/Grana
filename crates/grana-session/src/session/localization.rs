use std::collections::HashMap;
use std::sync::Arc;

use grana_analysis::localization::{LocalizationReport, LocalizationSpec};
use grana_pipeline::run::LocalizationReportRequest;
use grana_raster::Bitmap;
use grana_scene::{Job, ObjectSetId, Revision, Scene};

use super::*;

type Prepared = (
    Revision,
    LocalizationSpec,
    Vec<Option<Arc<ObjectFrame>>>,
    Vec<Option<Vec<bool>>>,
    f32,
);

#[derive(Default)]
pub struct Localizations {
    entries: HashMap<ObjectSetId, Entry>,
}

struct Entry {
    revision: Revision,
    report: Arc<LocalizationReport>,
    bitmap: Bitmap,
}

impl Localizations {
    pub fn report(&self, set: ObjectSetId) -> Option<&LocalizationReport> {
        self.entries.get(&set).map(|entry| entry.report.as_ref())
    }

    pub fn bitmap(&self, set: ObjectSetId) -> Option<&Bitmap> {
        self.entries.get(&set).map(|entry| &entry.bitmap)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn install(
        &mut self,
        set: ObjectSetId,
        revision: Revision,
        report: LocalizationReport,
    ) -> bool {
        let image = report.image();
        let bitmap = Bitmap::new(image.size(), image.shared());
        self.entries.insert(
            set,
            Entry {
                revision,
                report: Arc::new(report),
                bitmap,
            },
        );
        true
    }

    pub(super) fn remove(&mut self, set: ObjectSetId) -> bool {
        self.entries.remove(&set).is_some()
    }

    pub(super) fn clear(&mut self) -> bool {
        let held = !self.entries.is_empty();
        self.entries.clear();
        held
    }

    pub(super) fn retain(&mut self, scene: &Scene) -> bool {
        let held = self.entries.len();
        self.entries.retain(|set, entry| {
            scene
                .objects
                .get(*set)
                .is_some_and(|object| object.revision() == entry.revision)
        });
        held != self.entries.len()
    }
}

impl Session {
    pub fn localizations(&self) -> &Localizations {
        &self.localization
    }

    pub(super) fn start_localizations(&mut self, set: ObjectSetId, spec: LocalizationSpec) {
        if self.loading.is_some()
            || self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Localizations(set))
        {
            return;
        }
        if !self.analysable() {
            return;
        }
        let prepared = self.prepare_localizations(set, spec);
        let (revision, spec, frames, visible, pixel) = match prepared {
            Ok(prepared) => prepared,
            Err(detail) => {
                self.message = Some(Message::error(detail));
                return;
            }
        };
        self.message = None;
        self.record_progress(Job::Localizations(set), 0, 0, 0);
        self.analysis.push(crate::localization_report(
            Job::Localizations(set),
            LocalizationReportRequest::new(set, revision, frames, visible, spec, pixel),
        ));
    }

    fn prepare_localizations(
        &self,
        set: ObjectSetId,
        spec: LocalizationSpec,
    ) -> Result<Prepared, String> {
        let scene = self.scene.as_ref().ok_or("open a dataset first")?;
        let object = scene
            .objects
            .get(set)
            .ok_or("the object set left the scene")?;
        if object.kind() != grana_scene::ObjectKind::Spots {
            return Err("a super-resolution image renders a spot object set".to_string());
        }
        let source = object
            .localization()
            .ok_or("the object set carries no localization table to render")?;
        if scene.frame_count() < 2 {
            return Err("the dataset holds one frame to estimate a precision".to_string());
        }
        let spec = spec.fitted(scene.frame_count());
        let [start, end] = spec.frames();
        let frames = (start..=end)
            .map(|time| object.frame_shared(time))
            .collect::<Vec<_>>();
        let masked = object.filtered() || object.hidden_count() > 0;
        let visible = (start..=end)
            .map(|time| {
                if !masked {
                    return None;
                }
                object.frame(time).map(|frame| {
                    (0..frame.object_count())
                        .map(|index| object.visible_object(time, index))
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let pixel = source.pixel() * 0.001;
        if !(pixel.is_finite() && pixel > 0.0) {
            return Err("the localization table carries no camera pixel".to_string());
        }
        Ok((object.revision(), spec, frames, visible, pixel))
    }

    pub(super) fn cancel_localizations(&mut self, set: ObjectSetId) {
        self.cancel(Job::Localizations(set));
    }

    pub(super) fn clear_localizations(&mut self, set: ObjectSetId) {
        if self.localization.remove(set) {
            self.message = Some(Message::info("the super-resolution image is cleared"));
        }
    }

    pub(super) fn export_localization_image(&mut self, set: ObjectSetId) {
        let Some(bitmap) = self.localization.bitmap(set) else {
            self.message = Some(Message::error(
                "render a super-resolution image before exporting it",
            ));
            return;
        };
        let suggested = self
            .scene
            .as_ref()
            .and_then(|scene| scene.objects.get(set))
            .map_or_else(
                || "localizations.png".to_string(),
                |object| format!("{}.png", crate::slug(object.name())),
            );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export a super-resolution image")
            .set_file_name(suggested)
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };
        match grana_raster::write(&path, &bitmap.raster()) {
            Ok(()) => {
                self.message = Some(Message::info(format!(
                    "exported the super-resolution image to {}",
                    path.display()
                )));
            }
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export the super-resolution image to {}: {error}",
                    path.display()
                )));
            }
        }
    }
}
