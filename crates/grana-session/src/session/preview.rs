use std::collections::HashMap;
use std::sync::atomic::Ordering;

use grana_image::{Plate, Source};
use grana_raster::Bitmap;

use crate::task::{Poll, Task};

pub(super) enum Event {
    Thumb { series: usize, bitmap: Bitmap },
}

pub(super) struct Previews {
    task: Task<Event>,
    thumbs: HashMap<usize, Bitmap>,
    closed: bool,
}

impl Previews {
    pub(super) fn start(plate: &Plate, base: &Source) -> Self {
        let size = grana_preview::size(plate.len());
        let sources = plate
            .fields()
            .iter()
            .map(|field| {
                (
                    field.series(),
                    field_source(base, field.series(), field.path()),
                )
            })
            .collect::<Vec<_>>();
        let task = Task::spawn(move |cancel, sink| {
            for (series, source) in sources {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(bitmap) = grana_preview::field(&source, size) else {
                    continue;
                };
                sink(Event::Thumb { series, bitmap });
            }
        });
        Self {
            task,
            thumbs: HashMap::new(),
            closed: false,
        }
    }

    pub(super) fn thumbs(&self) -> &HashMap<usize, Bitmap> {
        &self.thumbs
    }

    pub(super) fn done(&self) -> usize {
        self.thumbs.len()
    }

    pub(super) fn closed(&self) -> bool {
        self.closed
    }

    pub(super) fn poll(&mut self) -> bool {
        if self.closed {
            return false;
        }
        let mut repaint = false;
        loop {
            match self.task.poll() {
                Poll::Event(Event::Thumb { series, bitmap }) => {
                    self.thumbs.insert(series, bitmap);
                    repaint = true;
                }
                Poll::Empty => break,
                Poll::Closed => {
                    self.closed = true;
                    break;
                }
            }
        }
        repaint
    }
}

fn field_source(base: &Source, series: usize, group: &str) -> Source {
    let group = group.to_string();
    match base {
        Source::File { path, .. } => Source::file_group(path.clone(), series, group),
        Source::Remote { url, .. } => Source::remote_group(url.clone(), series, group),
        Source::Memory(_) => unreachable!("a plate is read from a file or a remote store"),
    }
}
