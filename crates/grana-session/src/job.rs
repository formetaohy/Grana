use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use grana_analysis::{Roi, TrackParams};
use grana_image::{Image, Source};
use grana_measure::{ColocPair, ColocSettings};
use grana_scene::{Job, ObjectFrame};
use grana_volume::Region;

use crate::event::{Event, Poll};
use crate::run::{self, DetectRequest};

pub struct Analysis {
    job: Job,
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

impl Analysis {
    pub fn job(&self) -> Job {
        self.job
    }

    pub fn poll(&self) -> Poll {
        match self.events.try_recv() {
            Ok(event) => Poll::Event(event),
            Err(TryRecvError::Empty) => Poll::Empty,
            Err(TryRecvError::Disconnected) => Poll::Closed,
        }
    }
}

impl Drop for Analysis {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn detect(job: Job, source: Source, request: DetectRequest) -> Analysis {
    spawn(job, move |cancel, sink| {
        let reader = Image::open(&source)?;
        run::detect(&reader, request, cancel, sink)
    })
}

pub fn track(
    job: Job,
    frames: Vec<Vec<[f32; 3]>>,
    params: TrackParams,
    frame_interval: f32,
) -> Analysis {
    spawn(job, move |cancel, sink| {
        run::tracks(frames, params, frame_interval, cancel, sink);
        Ok(())
    })
}

pub fn relation(
    job: Job,
    source: Vec<Option<Arc<ObjectFrame>>>,
    target: Vec<Option<Arc<ObjectFrame>>>,
    workers: usize,
) -> Analysis {
    spawn(job, move |cancel, sink| {
        run::relation(source, target, workers, cancel, sink);
        Ok(())
    })
}

pub fn coloc(
    job: Job,
    source: Source,
    pair: ColocPair,
    region: Region,
    roi: Option<Roi>,
    mask: Option<u64>,
    settings: ColocSettings,
) -> Analysis {
    spawn(job, move |cancel, sink| {
        let reader = Image::open(&source)?;
        run::coloc(
            &reader,
            run::ColocRequest::new(pair, region, roi, mask, settings),
            cancel,
            sink,
        )
    })
}

fn spawn(
    job: Job,
    run: impl FnOnce(&AtomicBool, &mut dyn FnMut(Event)) -> Result<(), String> + Send + 'static,
) -> Analysis {
    let cancel = Arc::new(AtomicBool::new(false));
    let token = Arc::clone(&cancel);
    let (events, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut sink = |event: Event| {
            let _ = events.send(event);
        };
        if let Err(detail) = run(&token, &mut sink) {
            let _ = events.send(Event::Failed(detail));
        }
    });
    Analysis {
        job,
        events: receiver,
        cancel,
    }
}
