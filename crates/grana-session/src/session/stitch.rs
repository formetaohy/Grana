use std::sync::Arc;
use std::sync::mpsc;

use grana_exec::{Executor, Job, Role};
use grana_stitch::Folder;

use super::*;

use crate::task::{Poll, Task};
use crate::view::StitchView;

pub struct Stitch {
    folder: PathBuf,
    scan: Option<Arc<Folder>>,
    scanning: Option<Task<Result<Arc<Folder>, String>>>,
    channel: usize,
    series: usize,
    rows: usize,
    columns: usize,
    output: PathBuf,
    running: Option<Running>,
}

struct Running {
    job: Job,
    outcome: mpsc::Receiver<Result<PathBuf, String>>,
}

impl Stitch {
    pub fn busy(&self) -> bool {
        self.scanning.is_some() || self.running.is_some()
    }

    pub fn view(&self) -> StitchView {
        let scan = self.scan.as_deref();
        StitchView {
            folder: self.folder.display().to_string(),
            tiles: scan.map_or(0, |folder| folder.count()),
            channels: scan.map_or_else(Vec::new, |folder| folder.channels(self.series).to_vec()),
            channel: self.channel,
            series: self.series,
            series_count: scan.map_or(0, |folder| folder.series()),
            rows: self.rows,
            columns: self.columns,
            running: self.running.is_some(),
            scanning: self.scanning.is_some(),
        }
    }
}

impl Session {
    pub fn stitch(&self) -> Option<&Stitch> {
        self.stitch.as_ref()
    }

    pub fn stitching(&self) -> bool {
        self.stitch
            .as_ref()
            .is_some_and(|stitch| stitch.running.is_some())
    }

    fn open_mosaic(&mut self, path: PathBuf) {
        match self.begin_load(path) {
            Transfer::Started | Transfer::Refused => {}
            Transfer::Held(held) => {
                self.message = Some(Message::info(format!(
                    "the stitched mosaic is ready, and opening it waits for the unsaved analysis of {}",
                    held[0].name
                )));
            }
        }
    }

    pub fn begin_stitch(&mut self, folder: PathBuf) -> bool {
        if self.scene.is_some() {
            return false;
        }
        let name = folder.file_name().map_or_else(
            || "stitched".to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let parent = folder.parent().unwrap_or(&folder).to_path_buf();
        let output = parent.join(format!("{name}-stitched.ome.zarr"));
        let scanning = folder.clone();
        let task = Task::spawn(Role::Read, move |_, sink| {
            sink(grana_stitch::folder(&scanning).map_err(|error| error.to_string()));
        });
        self.stitch = Some(Stitch {
            folder,
            scan: None,
            scanning: Some(task),
            channel: 0,
            series: 0,
            rows: 0,
            columns: 0,
            output,
            running: None,
        });
        self.message = None;
        true
    }

    pub(super) fn set_stitch_grid(&mut self, rows: usize, columns: usize) -> bool {
        let Some(stitch) = self
            .stitch
            .as_mut()
            .filter(|stitch| stitch.running.is_none())
        else {
            return false;
        };
        if rows == 0
            || columns == 0
            || rows * columns != stitch.scan.as_ref().map_or(0, |folder| folder.count())
        {
            return false;
        }
        stitch.rows = rows;
        stitch.columns = columns;
        true
    }

    pub(super) fn set_stitch_channel(&mut self, channel: usize) -> bool {
        let Some(stitch) = self
            .stitch
            .as_mut()
            .filter(|stitch| stitch.running.is_none())
        else {
            return false;
        };
        if channel
            >= stitch
                .scan
                .as_ref()
                .map_or(0, |folder| folder.channels(stitch.series).len())
        {
            return false;
        }
        stitch.channel = channel;
        true
    }

    pub(super) fn set_stitch_series(&mut self, series: usize) -> bool {
        let Some(stitch) = self
            .stitch
            .as_mut()
            .filter(|stitch| stitch.running.is_none())
        else {
            return false;
        };
        if series >= stitch.scan.as_ref().map_or(0, |folder| folder.series()) {
            return false;
        }
        stitch.series = series;
        let channels = stitch
            .scan
            .as_ref()
            .map_or(0, |folder| folder.channels(series).len());
        stitch.channel = stitch.channel.min(channels.saturating_sub(1));
        true
    }

    pub(super) fn confirm_stitch(&mut self) -> bool {
        let Some(stitch) = self.stitch.as_mut() else {
            return false;
        };
        if stitch.running.is_some() || stitch.scan.is_none() {
            return false;
        }
        let request = (
            stitch.folder.clone(),
            stitch.output.clone(),
            stitch.rows,
            stitch.columns,
            stitch.channel,
            stitch.series,
        );
        let (sender, outcome) = mpsc::channel();
        let job = Executor::lane(Role::Compute).submit(move |cancel| {
            let (folder, output, rows, columns, channel, series) = request;
            let written = grana_stitch::folder(&folder)
                .and_then(|folder| {
                    if rows * columns != folder.count() {
                        return Err(grana_stitch::Error::new(format!(
                            "the {} × {rows} grid does not cover {} tiles",
                            columns,
                            folder.count()
                        )));
                    }
                    let request = grana_stitch::Request {
                        tiles: folder.tiles().to_vec(),
                        output: output.clone(),
                        rows,
                        columns,
                        channel,
                        series,
                    };
                    grana_stitch::stitch(&request, cancel).map(|_| output)
                })
                .map_err(|error| error.to_string());
            let _ = sender.send(written);
        });
        stitch.running = Some(Running { job, outcome });
        true
    }

    pub(super) fn cancel_stitch(&mut self) -> bool {
        if let Some(running) = self
            .stitch
            .as_mut()
            .and_then(|stitch| stitch.running.take())
        {
            running.job.cancel();
        }
        self.stitch = None;
        true
    }

    pub(super) fn poll_stitch(&mut self) -> bool {
        if self.poll_stitch_scan() {
            return true;
        }
        let outcome = match self
            .stitch
            .as_ref()
            .and_then(|stitch| stitch.running.as_ref())
        {
            Some(running) => running.outcome.try_recv(),
            None => return false,
        };
        match outcome {
            Ok(Ok(path)) => {
                grana_log::info!(target: "grana::session", path = %path.display(), "the stitched mosaic was written");
                self.stitch = None;
                if self.dirty() {
                    self.message = Some(Message::info(
                        "the stitched mosaic is ready, and the unsaved analysis stays open until it is saved",
                    ));
                } else {
                    self.open_mosaic(path);
                }
                true
            }
            Ok(Err(detail)) => {
                grana_log::error!(target: "grana::session", %detail, "the stitch failed");
                if let Some(stitch) = self.stitch.as_mut() {
                    stitch.running = None;
                }
                self.message = Some(Message::error(detail));
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                grana_log::error!(target: "grana::session", "the stitcher stopped before it wrote the mosaic");
                if let Some(stitch) = self.stitch.as_mut() {
                    stitch.running = None;
                }
                self.message = Some(Message::error(
                    "the stitcher stopped before it wrote the mosaic".to_string(),
                ));
                true
            }
        }
    }

    fn poll_stitch_scan(&mut self) -> bool {
        let Some(stitch) = self.stitch.as_mut() else {
            return false;
        };
        let Some(task) = stitch.scanning.as_ref() else {
            return false;
        };
        match task.poll() {
            Poll::Event(Ok(folder)) => {
                stitch.scanning = None;
                if folder.count() < 2 {
                    self.message = Some(Message::error(
                        "a tile folder holds at least two images".to_string(),
                    ));
                    self.stitch = None;
                    return true;
                }
                if grana_image::is_image(&stitch.output) && fresh(&stitch.output, folder.tiles()) {
                    let output = stitch.output.clone();
                    self.stitch = None;
                    if self.dirty() {
                        self.message = Some(Message::info(
                            "the stitched mosaic is ready, and the unsaved analysis stays open until it is saved",
                        ));
                    } else {
                        self.open_mosaic(output);
                    }
                    return true;
                }
                stitch.rows = folder.rows();
                stitch.columns = folder.columns();
                stitch.channel = stitch
                    .channel
                    .min(folder.channels(stitch.series).len().saturating_sub(1));
                stitch.series = stitch.series.min(folder.series().saturating_sub(1));
                stitch.scan = Some(folder);
                self.message = None;
                true
            }
            Poll::Event(Err(detail)) => {
                grana_log::error!(target: "grana::session", %detail, "the tile folder could not be scanned");
                self.message = Some(Message::error(detail));
                self.stitch = None;
                true
            }
            Poll::Empty => false,
            Poll::Closed => {
                grana_log::error!(target: "grana::session", "the tile scanner stopped before it finished");
                self.message = Some(Message::error(
                    "the tile folder could not be scanned".to_string(),
                ));
                self.stitch = None;
                true
            }
        }
    }
}

fn fresh(output: &std::path::Path, tiles: &[grana_image::Source]) -> bool {
    let Ok(stitched) = std::fs::metadata(output).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    tiles.iter().all(|tile| {
        tile.path()
            .and_then(|path| std::fs::metadata(path).ok())
            .and_then(|metadata| metadata.modified().ok())
            .is_none_or(|modified| modified <= stitched)
    })
}
