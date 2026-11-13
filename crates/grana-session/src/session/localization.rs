use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use grana_analysis::localization::{
    Clusters, LocalizationFilter, LocalizationReport, LocalizationSpec,
};
use grana_object::ClusterSpec;
use grana_pipeline::Event;
use grana_pipeline::run::{
    CompositionSource, LocalizationClusterRequest, LocalizationCompositionRequest,
    LocalizationImageRequest, LocalizationReportRequest, LocalizationWindow,
};
use grana_raster::Bitmap;
use grana_scene::{Job, ObjectSetId, Revision, Scene};
use grana_volume::Region;

use super::export::Writer;
use super::*;

type Prepared = (
    Revision,
    LocalizationSpec,
    Option<Region>,
    LocalizationWindow,
    f32,
    bool,
);

type PreparedClusters = (
    Revision,
    ClusterSpec,
    LocalizationFilter,
    LocalizationWindow,
    bool,
);

#[derive(Default)]
pub struct Localizations {
    entries: HashMap<ObjectSetId, Entry>,
}

struct Entry {
    revision: Revision,
    report: Arc<LocalizationReport>,
    bitmaps: Vec<Bitmap>,
    layer: usize,
}

impl Localizations {
    pub fn report(&self, set: ObjectSetId) -> Option<&LocalizationReport> {
        self.entries.get(&set).map(|entry| entry.report.as_ref())
    }

    pub fn bitmap(&self, set: ObjectSetId) -> Option<&Bitmap> {
        self.entries
            .get(&set)
            .and_then(|entry| entry.bitmaps.get(entry.layer))
    }

    pub fn layer(&self, set: ObjectSetId) -> usize {
        self.entries.get(&set).map_or(0, |entry| entry.layer)
    }

    pub fn layers(&self, set: ObjectSetId) -> usize {
        self.entries
            .get(&set)
            .map_or(0, |entry| entry.bitmaps.len())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn rendered(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn set_layer(&mut self, set: ObjectSetId, layer: usize) -> bool {
        let Some(entry) = self.entries.get_mut(&set) else {
            return false;
        };
        let layer = layer.min(entry.bitmaps.len().saturating_sub(1));
        if entry.layer == layer {
            return false;
        }
        entry.layer = layer;
        true
    }

    pub(super) fn install(
        &mut self,
        set: ObjectSetId,
        revision: Revision,
        report: LocalizationReport,
    ) -> bool {
        let bitmaps = report
            .stack()
            .layers()
            .iter()
            .map(|image| Bitmap::new(image.size(), image.shared()))
            .collect::<Vec<_>>();
        let layer = bitmaps.len() / 2;
        self.entries.insert(
            set,
            Entry {
                revision,
                report: Arc::new(report),
                bitmaps,
                layer,
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
        let (revision, spec, region, window, pixel, grouped) = match prepared {
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
            LocalizationReportRequest::new(set, revision, window, spec, region, pixel, grouped),
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
        let window = LocalizationWindow::of(object, spec.frames());
        let pixel = source.pixel() * 0.001;
        if !(pixel.is_finite() && pixel > 0.0) {
            return Err("the localization table carries no camera pixel".to_string());
        }
        Ok((
            object.revision(),
            spec,
            scene.region_scope(),
            window,
            pixel,
            !source.clustered(),
        ))
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
        let (revision, clusters, filter, window, grouped) = match prepared {
            Ok(prepared) => prepared,
            Err(detail) => {
                self.message = Some(Message::error(detail));
                return;
            }
        };
        self.message = None;
        self.record_progress(Job::Clusters(set), 0, 0, window.len());
        self.analysis.push(crate::localization_cluster(
            Job::Clusters(set),
            LocalizationClusterRequest::new(set, revision, window, clusters, filter, grouped),
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
        let window = LocalizationWindow::of(object, [0, scene.frame_count() - 1]);
        Ok((
            object.revision(),
            source.clusters(),
            source.spec().filter(),
            window,
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
        let name = self
            .scene
            .as_ref()
            .and_then(|scene| scene.objects.get(set))
            .map_or_else(
                || "localizations".to_string(),
                |object| object.name().to_string(),
            );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export a super-resolution image")
            .set_file_name(format!("{}.ome.tif", crate::slug(&name)))
            .add_filter("OME-TIFF", &["tif", "tiff"])
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };
        if let Err(detail) = self.export_localization_image_to(set, &path) {
            self.message = Some(Message::error(detail));
        }
    }

    pub(super) fn export_localization_composite(&mut self) {
        if self.tasks.writing() {
            self.message = Some(Message::error("an export is already running"));
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export the composed super-resolution image")
            .set_file_name("superres.ome.tif")
            .add_filter("OME-TIFF", &["tif", "tiff"])
            .save_file()
        else {
            return;
        };
        if let Err(detail) = self.export_localization_composite_to(&path) {
            self.message = Some(Message::error(detail));
        }
    }

    pub fn export_localization_composite_to(&mut self, path: &Path) -> Result<(), String> {
        if self.tasks.writing() {
            return Err("an export is already running".to_string());
        }
        let scene = self.scene.as_ref().ok_or("open a dataset first")?;
        let rendered = self.localization.rendered();
        if rendered < 2 {
            return Err(
                "render at least two localization channels before composing their image"
                    .to_string(),
            );
        }
        let mut sources = Vec::with_capacity(rendered);
        for object in scene.objects.iter() {
            if self.localization.report(object.id()).is_none() {
                continue;
            }
            let Some(source) = object.localization() else {
                continue;
            };
            let spec = source.spec().fitted(scene.frame_count());
            sources.push(CompositionSource::new(
                LocalizationWindow::of(object, spec.frames()),
                spec,
                object.color(),
            ));
        }
        let name = scene.dataset.name().to_string();
        let request = LocalizationCompositionRequest::new(sources, &name, path)?;
        self.message = None;
        self.tasks.superres = Some(Writer::start(
            "Composing the super-resolution image".to_string(),
            path.to_path_buf(),
            move |cancel, progress, _| {
                grana_pipeline::run::localization_composition(request, cancel, &mut |event| {
                    if let Event::Progress { done, total, .. } = event {
                        progress(done, total);
                    }
                })
            },
        ));
        Ok(())
    }

    pub fn export_localization_image_to(
        &mut self,
        set: ObjectSetId,
        path: &Path,
    ) -> Result<(), String> {
        if self.tasks.writing() {
            return Err("an export is already running".to_string());
        }
        let (window, spec, pixel, name, selected) = {
            let scene = self.scene.as_ref().ok_or("open a dataset first")?;
            let object = scene.objects.get(set).ok_or_else(|| {
                "the object set carries no localization table to export".to_string()
            })?;
            let source = object.localization().ok_or_else(|| {
                "the object set carries no localization table to export".to_string()
            })?;
            let spec = source.spec().fitted(scene.frame_count());
            (
                LocalizationWindow::of(object, spec.frames()),
                spec,
                source.pixel() * 0.001,
                object.name().to_string(),
                self.localization.layer(set),
            )
        };
        let request = LocalizationImageRequest::new(window, spec, pixel, &name, selected, path)?;
        self.message = None;
        self.tasks.superres = Some(Writer::start(
            "Rendering the super-resolution image".to_string(),
            path.to_path_buf(),
            move |cancel, progress, _| {
                grana_pipeline::run::localization_image(request, cancel, &mut |event| {
                    if let Event::Progress { done, total, .. } = event {
                        progress(done, total);
                    }
                })
            },
        ));
        Ok(())
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
