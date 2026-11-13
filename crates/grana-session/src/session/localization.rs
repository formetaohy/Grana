use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use grana_analysis::localization::{
    Clusters, LocalizationDepth, LocalizationFilter, LocalizationGeometry, LocalizationRenderer,
    LocalizationReport, LocalizationSpec,
};
use grana_object::ClusterSpec;
use grana_pipeline::run::{LocalizationClusterRequest, LocalizationReportRequest};
use grana_raster::Bitmap;
use grana_scene::{Job, ObjectSetId, Revision, Scene};
use grana_volume::Region;

use super::export::Writer;
use super::*;

type Prepared = (
    Revision,
    LocalizationSpec,
    Option<Region>,
    Vec<Option<Arc<ObjectFrame>>>,
    Vec<Option<Vec<bool>>>,
    f32,
    bool,
);

type PreparedClusters = (
    Revision,
    ClusterSpec,
    LocalizationFilter,
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
        let (revision, spec, region, frames, visible, pixel, grouped) = match prepared {
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
            LocalizationReportRequest::new(
                set,
                revision,
                grana_pipeline::run::LocalizationWindow::new(frames, visible),
                spec,
                region,
                pixel,
                grouped,
            ),
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
            scene.region_scope(),
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
        let (revision, clusters, filter, frames, visible, grouped) = match prepared {
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
            LocalizationClusterRequest::new(
                set,
                revision,
                grana_pipeline::run::LocalizationWindow::new(frames, visible),
                clusters,
                filter,
                grouped,
            ),
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
            source.spec().filter(),
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

    pub fn export_localization_image_to(
        &mut self,
        set: ObjectSetId,
        path: &Path,
    ) -> Result<(), String> {
        if self.tasks.writing() {
            return Err("an export is already running".to_string());
        }
        let (geometry, depth, density, filter, groups, spec, selected) = {
            let report = self
                .localization
                .report(set)
                .ok_or_else(|| "render a super-resolution image before exporting it".to_string())?;
            (
                report.geometry(),
                report.depth(),
                report.density(),
                report.filter(),
                report.groups().clone(),
                report.spec(),
                self.localization.layer(set),
            )
        };
        if !geometry.renderable() {
            let [width, height] = geometry.size();
            return Err(format!(
                "the super-resolution image would span {width}x{height} pixels; raise the pixel size"
            ));
        }
        let (frames, visible) = self
            .localization_window(set, spec)
            .map_err(|_| "the object set carries no localization table to export".to_string())?;
        let raster = ImageFile::of(path)?;
        let name = self
            .scene
            .as_ref()
            .and_then(|scene| scene.objects.get(set))
            .map_or_else(
                || "localizations".to_string(),
                |object| object.name().to_string(),
            );
        self.message = None;
        self.tasks.superres = Some(start_superres(Request {
            frames,
            visible,
            geometry,
            depth,
            density,
            filter,
            groups,
            raster,
            selected,
            name,
            path: path.to_path_buf(),
        }));
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

#[derive(Clone, Copy)]
enum ImageFile {
    Png,
    Tiff,
}

impl ImageFile {
    fn of(path: &Path) -> Result<Self, String> {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("png") => Ok(Self::Png),
            Some("tif" | "tiff") => Ok(Self::Tiff),
            Some(other) => Err(format!(
                "a super-resolution image writes a PNG or an OME-TIFF, not `.{other}`"
            )),
            None => Err("a super-resolution image names a file extension to write".to_string()),
        }
    }
}

pub(super) struct Request {
    frames: Vec<Option<Arc<ObjectFrame>>>,
    visible: Vec<Option<Vec<bool>>>,
    geometry: LocalizationGeometry,
    depth: LocalizationDepth,
    density: f32,
    filter: LocalizationFilter,
    groups: grana_analysis::localization::Groups,
    raster: ImageFile,
    selected: usize,
    name: String,
    path: PathBuf,
}

pub(super) fn start_superres(request: Request) -> Writer {
    let Request {
        frames,
        visible,
        geometry,
        depth,
        density,
        filter,
        groups,
        raster,
        selected,
        name,
        path,
    } = request;
    Writer::start(
        "Rendering the super-resolution image".to_string(),
        path,
        move |cancel, progress, path| {
            let held = frames
                .iter()
                .zip(&visible)
                .map(|(frame, mask)| {
                    let spots = frame
                        .as_deref()
                        .and_then(ObjectFrame::spots)
                        .map_or(&[][..], grana_object::SpotSet::spots);
                    grana_analysis::localization::LocalizationFrame::new(
                        spots,
                        mask.as_deref(),
                        filter,
                    )
                })
                .collect::<Vec<_>>();
            let mut report = |done: usize, total: usize| {
                progress(done, total);
                !cancel.load(Ordering::Relaxed)
            };
            let mut renderer =
                LocalizationRenderer::new(&held, geometry, depth, groups, &mut report)?;
            renderer.at_density(density);
            match raster {
                ImageFile::Png => {
                    renderer.begin((selected.min(depth.layers() as usize - 1)) as u32)?;
                    grana_raster::write_stream(path, geometry.size(), &mut |_, row| {
                        renderer.row(row)
                    })
                    .map_err(|error| error.to_string())?
                }
                ImageFile::Tiff => {
                    let pixel = geometry.pixel();
                    let slice = if depth.step() > 0.0 {
                        depth.step()
                    } else {
                        pixel
                    };
                    let stack = grana_tiff::Stack {
                        size: geometry.size(),
                        samples: 3,
                        name: &name,
                        physical: Some([pixel, pixel, slice]),
                        interval: None,
                        depth: depth.layers(),
                    };
                    let mut current = u32::MAX;
                    grana_tiff::write_stack(path, &stack, &mut |layer, _, bytes| {
                        if layer != current {
                            renderer.begin(layer)?;
                            current = layer;
                        }
                        renderer.row_rgb(bytes)
                    })
                    .map_err(|error| error.to_string())?
                }
            }
            Ok(())
        },
    )
}
