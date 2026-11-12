use std::sync::mpsc;

use super::*;

use crate::view::StitchView;

pub struct Stitch {
    folder: PathBuf,
    tiles: usize,
    channels: Vec<String>,
    channel: usize,
    series: usize,
    series_count: usize,
    rows: usize,
    columns: usize,
    output: PathBuf,
    running: Option<mpsc::Receiver<Result<PathBuf, String>>>,
}

impl Stitch {
    pub fn view(&self) -> StitchView {
        StitchView {
            folder: self.folder.display().to_string(),
            tiles: self.tiles,
            channels: self.channels.clone(),
            channel: self.channel,
            series: self.series,
            series_count: self.series_count,
            rows: self.rows,
            columns: self.columns,
            running: self.running.is_some(),
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

    pub fn begin_stitch(&mut self, folder: PathBuf) -> bool {
        if self.scene.is_some() {
            return false;
        }
        let tiles = match grana_stitch::tiles(&folder) {
            Ok(tiles) if tiles.len() >= 2 => tiles,
            Ok(_) => {
                self.message = Some(Message::error(
                    "a tile folder holds at least two images".to_string(),
                ));
                return false;
            }
            Err(error) => {
                self.message = Some(Message::error(error.to_string()));
                return false;
            }
        };
        let first = &tiles[0];
        let channels = grana_image::Image::open(first)
            .map(|image| {
                image
                    .pyramid()
                    .channels()
                    .iter()
                    .map(|channel| channel.name().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let series_count = grana_stitch::series_count(&tiles).unwrap_or(1);
        let name = folder.file_name().map_or_else(
            || "stitched".to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let parent = folder.parent().unwrap_or(&folder).to_path_buf();
        let output = parent.join(format!("{name}-stitched.ome.zarr"));
        if grana_image::is_image(&output) && fresh(&output, &tiles) {
            self.begin_load(output);
            return true;
        }
        let (rows, columns) = grana_stitch::default_grid(tiles.len());
        self.stitch = Some(Stitch {
            folder,
            tiles: tiles.len(),
            channels,
            channel: 0,
            series: 0,
            series_count,
            rows,
            columns,
            output,
            running: None,
        });
        self.message = None;
        true
    }

    pub(super) fn set_stitch_grid(&mut self, rows: usize, columns: usize) -> bool {
        let Some(stitch) = self.stitch.as_mut() else {
            return false;
        };
        if stitch.running.is_some() || rows == 0 || columns == 0 || rows * columns != stitch.tiles {
            return false;
        }
        stitch.rows = rows;
        stitch.columns = columns;
        true
    }

    pub(super) fn set_stitch_channel(&mut self, channel: usize) -> bool {
        let Some(stitch) = self.stitch.as_mut() else {
            return false;
        };
        if stitch.running.is_some() || channel >= stitch.channels.len() {
            return false;
        }
        stitch.channel = channel;
        true
    }

    pub(super) fn set_stitch_series(&mut self, series: usize) -> bool {
        let Some(stitch) = self.stitch.as_mut() else {
            return false;
        };
        if stitch.running.is_some() || series >= stitch.series_count {
            return false;
        }
        stitch.series = series;
        true
    }

    pub(super) fn confirm_stitch(&mut self) -> bool {
        let Some(stitch) = self.stitch.as_mut() else {
            return false;
        };
        if stitch.running.is_some() {
            return false;
        }
        let tiles = match grana_stitch::tiles(&stitch.folder) {
            Ok(tiles) => tiles,
            Err(error) => {
                self.message = Some(Message::error(error.to_string()));
                return false;
            }
        };
        let request = grana_stitch::Request {
            tiles,
            output: stitch.output.clone(),
            rows: stitch.rows,
            columns: stitch.columns,
            channel: stitch.channel,
            series: stitch.series,
        };
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("grana-stitch".to_string())
            .spawn(move || {
                let outcome = grana_stitch::stitch(&request).map_err(|error| error.to_string());
                let _ = sender.send(outcome.map(|_| request.output));
            });
        match spawned {
            Ok(_) => {
                stitch.running = Some(receiver);
                true
            }
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "the stitcher did not start: {error}"
                )));
                false
            }
        }
    }

    pub(super) fn cancel_stitch(&mut self) -> bool {
        self.stitch = None;
        true
    }

    pub(super) fn poll_stitch(&mut self) -> bool {
        let outcome = match self
            .stitch
            .as_ref()
            .and_then(|stitch| stitch.running.as_ref())
        {
            Some(receiver) => receiver.try_recv(),
            None => return false,
        };
        match outcome {
            Ok(Ok(path)) => {
                self.stitch = None;
                self.begin_load(path);
                true
            }
            Ok(Err(detail)) => {
                if let Some(stitch) = self.stitch.as_mut() {
                    stitch.running = None;
                }
                self.message = Some(Message::error(detail));
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
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
