use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use grana_analysis::localization::{
    Clusters, LocalizationGeometry, LocalizationRenderer, LocalizationReport, LocalizationSpec,
};
use grana_object::ClusterSpec;
use grana_pipeline::run::{LocalizationClusterRequest, LocalizationReportRequest};
use grana_raster::Bitmap;
use grana_scene::{Job, ObjectSetId, Revision, Scene};

use super::*;
use crate::task::Progress as ExportProgress;

type Prepared = (
    Revision,
    LocalizationSpec,
    Vec<Option<Arc<ObjectFrame>>>,
    Vec<Option<Vec<bool>>>,
    f32,
    bool,
);

type PreparedClusters = (
    Revision,
    ClusterSpec,
    Vec<Option<Arc<ObjectFrame>>>,
    Vec<Option<Vec<bool>>>,
    bool,
);

type Window = (Vec<Option<Arc<ObjectFrame>>>, Vec<Option<Vec<bool>>>);

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

    pub(super) fn start_localizations(&mut self, set: ObjectSetId) {
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
        let prepared = self.prepare_localizations(set);
        let (revision, spec, frames, visible, pixel, grouped) = match prepared {
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
            LocalizationReportRequest::new(set, revision, frames, visible, spec, pixel, grouped),
        ));
    }

    fn prepare_localizations(&self, set: ObjectSetId) -> Result<Prepared, String> {
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
        let spec = source.spec().fitted(scene.frame_count());
        let (frames, visible) = self.localization_window(set, spec)?;
        let pixel = source.pixel() * 0.001;
        if !(pixel.is_finite() && pixel > 0.0) {
            return Err("the localization table carries no camera pixel".to_string());
        }
        Ok((
            object.revision(),
            spec,
            frames,
            visible,
            pixel,
            !source.clustered(),
        ))
    }

    pub(super) fn localization_window(
        &self,
        set: ObjectSetId,
        spec: LocalizationSpec,
    ) -> Result<Window, String> {
        let scene = self.scene.as_ref().ok_or("open a dataset first")?;
        let object = scene
            .objects
            .get(set)
            .ok_or("the object set left the scene")?;
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
        Ok((frames, visible))
    }

    pub(super) fn cancel_localizations(&mut self, set: ObjectSetId) {
        self.cancel(Job::Localizations(set));
    }

    pub(super) fn start_clusters(&mut self, set: ObjectSetId) {
        if self.loading.is_some()
            || self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Clusters(set))
        {
            return;
        }
        if !self.analysable() {
            return;
        }
        let prepared = self.prepare_clusters(set);
        let (revision, clusters, frames, visible, grouped) = match prepared {
            Ok(prepared) => prepared,
            Err(detail) => {
                self.message = Some(Message::error(detail));
                return;
            }
        };
        self.message = None;
        self.record_progress(Job::Clusters(set), 0, 0, frames.len());
        self.analysis.push(crate::localization_cluster(
            Job::Clusters(set),
            LocalizationClusterRequest::new(set, revision, frames, visible, clusters, grouped),
        ));
    }

    fn prepare_clusters(&self, set: ObjectSetId) -> Result<PreparedClusters, String> {
        let scene = self.scene.as_ref().ok_or("open a dataset first")?;
        let object = scene
            .objects
            .get(set)
            .ok_or("the object set left the scene")?;
        if object.kind() != grana_scene::ObjectKind::Spots {
            return Err("molecules cluster a spot object set".to_string());
        }
        let source = object
            .localization()
            .ok_or("the object set carries no localization table to cluster")?;
        let frames = (0..scene.frame_count())
            .map(|time| object.frame_shared(time))
            .collect::<Vec<_>>();
        let masked = object.filtered() || object.hidden_count() > 0;
        let visible = (0..scene.frame_count())
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
        Ok((
            object.revision(),
            source.clusters(),
            frames,
            visible,
            !source.clustered(),
        ))
    }

    pub(super) fn install_clusters(&mut self, set: ObjectSetId, clusters: &Clusters) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        match scene.install_molecules(set, clusters.groups()) {
            Ok(_) => {
                self.message = Some(Message::info(format!(
                    "clustered {} localizations into {} molecules",
                    clusters.localizations(),
                    clusters.molecules()
                )));
            }
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    pub(super) fn clear_localizations(&mut self, set: ObjectSetId) {
        if self.localization.remove(set) {
            self.message = Some(Message::info("the super-resolution image is cleared"));
        }
    }

    pub(super) fn export_localization_image(&mut self, set: ObjectSetId) {
        if self.tasks.writing() {
            self.message = Some(Message::error("an export is already running"));
            return;
        }
        let Some(report) = self.localization.report(set) else {
            self.message = Some(Message::error(
                "render a super-resolution image before exporting it",
            ));
            return;
        };
        let geometry = report.geometry();
        if !geometry.renderable() {
            let [width, height] = geometry.size();
            self.message = Some(Message::error(format!(
                "the super-resolution image would span {width}x{height} pixels; raise the pixel size"
            )));
            return;
        }
        let spec = report.spec();
        let Ok((frames, visible)) = self.localization_window(set, spec) else {
            self.message = Some(Message::error(
                "the object set carries no localization table to export",
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
        self.message = None;
        self.tasks.superres = Some(Superres::start(
            frames,
            visible,
            geometry,
            report.density(),
            report.groups().clone(),
            path,
        ));
    }

    pub(super) fn estimate_clusters(&mut self, set: ObjectSetId) {
        let Some(report) = self.localization.report(set) else {
            self.message = Some(Message::error(
                "render a super-resolution image before estimating the cluster parameters",
            ));
            return;
        };
        let suggested = report.suggested_clusters();
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let Some(source) = object.localization() else {
            return;
        };
        let clusters = source
            .clusters()
            .at_radius(suggested.radius())
            .at_dark(suggested.dark());
        if scene.set_localization_clusters(set, clusters) {
            self.message = Some(Message::info(format!(
                "estimated a cluster radius of {:.1} nm over {} dark frames",
                clusters.radius() * 1000.0,
                clusters.dark()
            )));
        }
    }

    pub(super) fn finish_superres(&mut self, outcome: Result<PathBuf, String>) {
        self.message = Some(match outcome {
            Ok(path) => Message::info(format!(
                "exported the super-resolution image to {}",
                path.display()
            )),
            Err(detail) => Message::error(detail),
        });
    }
}

pub(super) struct Superres {
    task: ExportProgress<Result<PathBuf, String>>,
}

impl Superres {
    fn start(
        frames: Vec<Option<Arc<ObjectFrame>>>,
        visible: Vec<Option<Vec<bool>>>,
        geometry: LocalizationGeometry,
        density: f32,
        groups: grana_analysis::localization::Groups,
        path: PathBuf,
    ) -> Self {
        let task = ExportProgress::spawn(grana_exec::Role::Write, move |cancel, progress| {
            let held = frames
                .iter()
                .zip(&visible)
                .map(|(frame, mask)| {
                    let spots = frame
                        .as_deref()
                        .and_then(ObjectFrame::spots)
                        .map_or(&[][..], grana_object::SpotSet::spots);
                    grana_analysis::localization::LocalizationFrame::new(spots, mask.as_deref())
                })
                .collect::<Vec<_>>();
            let mut report = |done: usize, total: usize| {
                progress(done, total);
                !cancel.load(Ordering::Relaxed)
            };
            let mut renderer =
                LocalizationRenderer::new(&held, geometry, density, groups, &mut report)?;
            grana_raster::write_stream(&path, geometry.size(), &mut |_, row| renderer.row(row))
                .map_err(|error| error.to_string())?;
            Ok(path)
        });
        Self { task }
    }

    pub(super) fn stepped(&mut self) -> bool {
        self.task.stepped()
    }

    pub(super) fn progress(&self) -> (usize, usize) {
        self.task.progress()
    }

    pub(super) fn poll(&mut self) -> Poll<Result<PathBuf, String>> {
        self.task.poll()
    }
}
