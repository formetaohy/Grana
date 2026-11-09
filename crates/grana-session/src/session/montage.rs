use std::path::PathBuf;
use std::sync::atomic::Ordering;

use grana_image::{Plate, Source};
use grana_raster::{Cell, Group, Raster, Sheet};
use grana_text::Fonts;

use crate::task::{Poll, Task};

const TILE: [u32; 2] = [192, 144];
const SAMPLE: u32 = 256;
const BACKGROUND: [u8; 4] = [10, 12, 16, 255];

pub(super) enum Event {
    Progress(usize, usize),
    Done(Result<PathBuf, String>),
}

pub(super) struct Montage {
    task: Task<Event>,
    progress: (usize, usize),
    closed: bool,
}

impl Montage {
    pub(super) fn start(plate: &Plate, base: &Source, path: PathBuf) -> Self {
        let plate = plate.clone();
        let base = base.clone();
        let target = path;
        let task = Task::spawn(move |cancel, sink| {
            let outcome = compose(&plate, &base, &target, cancel, |done, total| {
                sink(Event::Progress(done, total))
            });
            sink(Event::Done(outcome));
        });
        Self {
            task,
            progress: (0, 0),
            closed: false,
        }
    }

    pub(super) fn progress(&self) -> (usize, usize) {
        self.progress
    }

    pub(super) fn poll(&mut self) -> Poll<Event> {
        if self.closed {
            return Poll::Closed;
        }
        loop {
            match self.task.poll() {
                Poll::Event(Event::Progress(done, total)) => self.progress = (done, total),
                other => {
                    if matches!(other, Poll::Closed) {
                        self.closed = true;
                    }
                    return other;
                }
            }
        }
    }
}

fn compose(
    plate: &Plate,
    base: &Source,
    path: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<PathBuf, String> {
    let columns = plate.columns();
    let rows = plate.rows();
    let total = plate.len();
    let mut images = Vec::with_capacity(rows * columns);
    let mut titles = Vec::with_capacity(rows * columns);
    let mut details = Vec::with_capacity(rows * columns);
    for row in 0..rows {
        for column in 0..columns {
            let label = format!(
                "{}{}",
                plate.row_name(row).unwrap_or_default(),
                plate.column_name(column).unwrap_or_default()
            );
            let well = plate.well(row, column);
            let fields = well.map_or(0, |well| well.fields().len());
            titles.push(label);
            details.push(match fields {
                0 => "empty".to_string(),
                1 => "1 field".to_string(),
                count => format!("{count} fields"),
            });
            let Some(field) = well.and_then(|well| well.fields().first()) else {
                images.push(Raster::solid(TILE[0], TILE[1], BACKGROUND));
                continue;
            };
            if cancel.load(Ordering::Relaxed) {
                return Err("the plate montage was cancelled".to_string());
            }
            let source = field_source(base, field.series(), field.path());
            let image = grana_preview::field(&source, SAMPLE)?;
            images.push(image.raster());
            progress(images.len(), total);
        }
    }
    let cells = images
        .iter()
        .zip(&titles)
        .zip(&details)
        .map(|((image, title), detail)| Cell {
            image,
            title,
            detail,
            accent: None,
        })
        .collect::<Vec<_>>();
    let groups = [Group {
        name: format!(
            "{} · {rows} × {columns} wells · {total} fields",
            plate.name()
        ),
        table: None,
        cells,
    }];
    let raster = Sheet::grid(TILE, columns as u32).compose(&mut Fonts::system(), &groups);
    grana_raster::write(path, &raster).map_err(String::from)?;
    Ok(path.to_path_buf())
}

fn field_source(base: &Source, series: usize, group: &str) -> Source {
    let group = group.to_string();
    match base {
        Source::File { path, .. } => Source::file_group(path.clone(), series, group),
        Source::Remote { url, .. } => Source::remote_group(url.clone(), series, group),
        Source::Memory(_) => unreachable!("a plate is read from a file or a remote store"),
    }
}
