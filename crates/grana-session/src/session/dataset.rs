use std::path::PathBuf;
use std::sync::Arc;

use grana_pipeline::Channels;
use grana_pipeline::export::{ChannelLayer, DatasetSpec, LabelLayer, Summary, cropped_levels};
use grana_volume::Level;

use crate::Inquiry;
use crate::loader::Loader;
use crate::task::{Poll, Progress};

pub(super) struct DatasetExport {
    task: Progress<Result<Summary, String>>,
}

struct Request {
    path: PathBuf,
    name: String,
    channels: Arc<Channels>,
    layers: Vec<ChannelLayer>,
    labels: Vec<LabelLayer>,
    levels: Vec<Level>,
    origins: Vec<[u32; 3]>,
    placement: [f32; 3],
    frames: u32,
    interval: f32,
    calibration: grana_volume::Calibration,
    drift: Vec<grana_warp::Warps>,
}

impl DatasetExport {
    fn start(request: Request) -> Self {
        let Request {
            path,
            name,
            channels,
            layers,
            labels,
            levels,
            origins,
            placement,
            frames,
            interval,
            calibration,
            drift,
        } = request;
        let task = Progress::spawn(grana_exec::Role::Write, move |cancel, progress| {
            grana_pipeline::write_dataset(
                DatasetSpec {
                    path,
                    name,
                    channels: layers,
                    levels,
                    origins,
                    placement,
                    frames,
                    interval,
                    calibration,
                    drift,
                    labels,
                },
                &channels,
                cancel,
                progress,
            )
        });
        Self { task }
    }

    pub(super) fn label(&self) -> &str {
        "Exporting the dataset"
    }

    pub(super) fn progress(&self) -> (usize, usize) {
        self.task.progress()
    }

    pub(super) fn stepped(&mut self) -> bool {
        self.task.stepped()
    }

    pub(super) fn poll(&mut self) -> Poll<Result<Summary, String>> {
        let outcome = self.task.poll();
        match &outcome {
            Poll::Event(Ok(summary)) => {
                grana_log::info!(
                    target: "grana::session",
                    path = %summary.path().display(),
                    channels = summary.channels(),
                    labels = summary.labels(),
                    frames = summary.frames(),
                    "the dataset export finished"
                );
            }
            Poll::Event(Err(detail)) => {
                grana_log::error!(target: "grana::session", %detail, "the dataset export failed");
            }
            Poll::Empty | Poll::Closed => {}
        }
        outcome
    }
}

impl super::Session {
    pub(super) fn export_dataset(&mut self) {
        if self.tasks.writing() {
            self.message = Some(crate::Message::error("an export is already running"));
            return;
        }
        if self.loading.is_some() {
            self.message = Some(crate::Message::error(
                "the dataset is still opening before it can export",
            ));
            return;
        }
        if !self.analysis.is_empty() {
            self.message = Some(crate::Message::error(
                "wait for the running analysis before exporting the dataset",
            ));
            return;
        }
        if let Some(scene) = self.scene.as_mut() {
            scene.sync_masks();
        }
        let Some(channels) = self.source.as_ref().and_then(Loader::channels) else {
            self.message = Some(crate::Message::error("no dataset is open"));
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let interval = scene.dataset.frame_interval();
        if !(interval.is_finite() && interval > 0.0) {
            self.message = Some(crate::Message::error(
                "the dataset declares no positive frame interval to export",
            ));
            return;
        }
        let stored = scene.dataset.pyramid().levels()[..scene.dataset.stored_levels()].to_vec();
        if stored.is_empty() {
            self.message = Some(crate::Message::error(
                "the dataset holds no stored resolution level",
            ));
            return;
        }
        let cropped = match cropped_levels(scene.region, &stored) {
            Ok(cropped) => cropped,
            Err(detail) => {
                self.message = Some(crate::Message::error(detail));
                return;
            }
        };
        let layers = scene
            .dataset
            .channels()
            .iter()
            .enumerate()
            .map(|(index, info)| ChannelLayer {
                index,
                name: info.name().to_string(),
                tint: info.tint(),
                data: info.intensity(),
                window: info.display(),
                color_table: info.color_table().cloned(),
            })
            .collect::<Vec<_>>();
        let labels = match scene
            .objects
            .iter()
            .map(|set| LabelLayer::of(set, &cropped.levels))
            .collect::<Result<Vec<_>, String>>()
        {
            Ok(labels) => labels.into_iter().flatten().collect::<Vec<_>>(),
            Err(detail) => {
                self.message = Some(crate::Message::error(detail));
                return;
            }
        };
        let frames = scene.frame_count();
        let drift = (0..frames).map(|time| scene.drift_warps(time)).collect();
        let placement = scene.dataset.pyramid().placement();
        let calibration = scene.dataset.pyramid().calibration();
        let name = scene.dataset.name().to_string();
        let suggested = format!("{}.ome.zarr", crate::slug(&name));
        self.ask(
            Inquiry::save(
                "Export the dataset as an OME-Zarr or OME-TIFF archive",
                suggested,
            )
            .filter("OME-Zarr", &["zarr"])
            .filter("OME-TIFF pyramid", &["tif", "tiff"]),
            move |session, path| {
                let Some(path) = path else {
                    return;
                };
                session.write_dataset(Request {
                    path,
                    name,
                    channels,
                    layers,
                    labels,
                    levels: cropped.levels,
                    origins: cropped.origins,
                    placement,
                    frames,
                    interval,
                    calibration,
                    drift,
                });
            },
        );
    }

    fn write_dataset(&mut self, request: Request) {
        if self.tasks.writing() {
            self.message = Some(crate::Message::error("an export is already running"));
            return;
        }
        self.message = None;
        self.tasks.dataset = Some(DatasetExport::start(request));
    }

    pub(super) fn finish_dataset(&mut self, outcome: Result<Summary, String>) {
        self.message = Some(match outcome {
            Ok(written) => {
                let scope = if written.cropped() {
                    " inside the region"
                } else {
                    ""
                };
                let archive = if grana_archive::Format::of(written.path())
                    == Some(grana_archive::Format::Tiff)
                {
                    "an OME-TIFF archive"
                } else {
                    "an OME-Zarr image"
                };
                crate::Message::info(format!(
                    "wrote {} channels and {} labels over {} frames at {} resolutions{scope} to {archive} {}",
                    written.channels(),
                    written.labels(),
                    written.frames(),
                    written.levels(),
                    written.path().display()
                ))
            }
            Err(detail) => crate::Message::error(detail),
        });
    }
}
