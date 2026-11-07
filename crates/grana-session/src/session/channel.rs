use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use grana_pipeline::Channels;
use grana_volume::{Intensity, Level};
use grana_zarr::{ImageSpec, ImageWritten, WriteChannel};

use crate::loader::Loader;
use crate::task::{Poll, Task};

pub(super) enum Event {
    Progress(usize, usize),
    Done(Result<ImageWritten, String>),
}

pub(super) struct ChannelExport {
    task: Task<Event>,
    label: String,
    progress: (usize, usize),
}

struct Request {
    path: PathBuf,
    name: String,
    channel: usize,
    channel_name: String,
    tint: [f32; 3],
    range: Intensity,
    levels: Vec<Level>,
    frames: u32,
    interval: f32,
    channels: Arc<Channels>,
}

impl ChannelExport {
    fn start(request: Request) -> Self {
        let Request {
            path,
            name,
            channel,
            channel_name,
            tint,
            range,
            levels,
            frames,
            interval,
            channels,
        } = request;
        let task = Task::spawn(move |cancel, sink| {
            let mut fill = |level: usize,
                            time: u32,
                            _: usize,
                            origin: [u32; 3],
                            extent: [u32; 3],
                            data: &mut [f32]| {
                if cancel.load(Ordering::Relaxed) {
                    return Err("the channel export was cancelled".to_string());
                }
                let ranges = [
                    origin[0]..origin[0] + extent[0],
                    origin[1]..origin[1] + extent[1],
                    origin[2]..origin[2] + extent[2],
                ];
                let values = channels
                    .read_region(time, level, channel, &ranges, [0.0; 3])
                    .map_err(|error| error.to_string())?;
                values.write_values(data);
                Ok(())
            };
            let channels = [WriteChannel {
                name: &channel_name,
                tint,
                range,
            }];
            let mut progress = |done: usize, total: usize| sink(Event::Progress(done, total));
            let outcome = grana_zarr::write_image(
                &path,
                ImageSpec {
                    name: &name,
                    channels: &channels,
                    levels: &levels,
                    frames,
                    interval,
                    fill: &mut fill,
                    progress: &mut progress,
                },
            )
            .map_err(|error| error.to_string());
            sink(Event::Done(outcome));
        });
        Self {
            task,
            label: "Exporting a channel".to_string(),
            progress: (0, 0),
        }
    }

    pub(super) fn label(&self) -> &str {
        &self.label
    }

    pub(super) fn progress(&self) -> (usize, usize) {
        self.progress
    }

    pub(super) fn poll(&mut self) -> Poll<Event> {
        loop {
            match self.task.poll() {
                Poll::Event(Event::Progress(done, total)) => self.progress = (done, total),
                other => return other,
            }
        }
    }
}

impl super::Session {
    pub(super) fn export_channel(&mut self, channel: usize) {
        if self.channel_export.is_some() || self.labels.is_some() {
            self.message = Some(crate::Message::error("an export is already running"));
            return;
        }
        if self.loading.is_some() {
            self.message = Some(crate::Message::error(
                "the dataset is still opening before it can export a channel",
            ));
            return;
        }
        if !self.analysis.is_empty() {
            self.message = Some(crate::Message::error(
                "wait for the running analysis before exporting a channel",
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
        let Some(info) = scene.dataset.channels().get(channel) else {
            self.message = Some(crate::Message::error("the dataset holds no such channel"));
            return;
        };
        let interval = scene.dataset.frame_interval();
        if !(interval.is_finite() && interval > 0.0) {
            self.message = Some(crate::Message::error(
                "the dataset declares no positive frame interval to export",
            ));
            return;
        }
        let levels = scene.dataset.pyramid().levels()[..scene.dataset.stored_levels()].to_vec();
        if levels.is_empty() {
            self.message = Some(crate::Message::error(
                "the dataset holds no stored resolution level",
            ));
            return;
        }
        let dataset = scene.dataset.name().to_string();
        let frames = scene.frame_count();
        let channel_name = info.name().to_string();
        let tint = info.tint();
        let range = info.window();
        let suggested = format!(
            "{}-{}.ome.zarr",
            crate::slug(&dataset),
            crate::slug(&channel_name)
        );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export the channel as an OME-Zarr image")
            .set_file_name(&suggested)
            .add_filter("OME-Zarr", &["zarr"])
            .save_file()
        else {
            return;
        };
        let name = format!("{dataset} {channel_name}");
        self.message = None;
        self.channel_export = Some(ChannelExport::start(Request {
            path,
            name,
            channel,
            channel_name,
            tint,
            range,
            levels,
            frames,
            interval,
            channels,
        }));
    }

    pub(super) fn poll_channel(&mut self) -> bool {
        let Some(mut export) = self.channel_export.take() else {
            return false;
        };
        let mut repaint = false;
        let mut keep = true;
        loop {
            match export.poll() {
                Poll::Event(Event::Progress(_, _)) => repaint = true,
                Poll::Event(Event::Done(outcome)) => {
                    keep = false;
                    repaint = true;
                    self.message = Some(match outcome {
                        Ok(written) => crate::Message::info(format!(
                            "wrote {} levels of {} frames to {}",
                            written.levels(),
                            written.frames(),
                            written.path().display()
                        )),
                        Err(detail) => crate::Message::error(detail),
                    });
                    break;
                }
                Poll::Empty => break,
                Poll::Closed => {
                    keep = false;
                    repaint = true;
                    self.message = Some(crate::Message::error(
                        "the channel export stopped before the image was written",
                    ));
                    break;
                }
            }
        }
        if keep {
            self.channel_export = Some(export);
        }
        repaint
    }
}
