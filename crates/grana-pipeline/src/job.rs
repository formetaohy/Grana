use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use grana_analysis::TrackParams;
use grana_object::ColocPair;
use grana_scene::{Job, ObjectParams};
use grana_volume::Region;

use crate::event::{Event, Poll};
use crate::reader::Reader;
use crate::run;
use crate::source::Source;

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

pub fn detect(
    job: Job,
    source: Source,
    channel: usize,
    params: ObjectParams,
    region: Region,
) -> Analysis {
    spawn(job, move |cancel, sink| {
        let reader = Reader::open(&source)?;
        run::detect(&reader, channel, params, region, cancel, sink)
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

pub fn coloc(job: Job, source: Source, pair: ColocPair, region: Region) -> Analysis {
    spawn(job, move |cancel, sink| {
        let reader = Reader::open(&source)?;
        run::coloc(&reader, pair, region, cancel, sink)
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
