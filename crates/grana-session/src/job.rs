use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use grana_analysis::TrackParams;
use grana_image::{Image, Source};
use grana_scene::{Job, ObjectFrame};

use crate::event::Event;
use crate::run::{self, ColocRequest, DetectRequest};
use crate::task::{Poll, Task};

pub struct Analysis {
    job: Job,
    task: Task<Event>,
}

impl Analysis {
    pub fn job(&self) -> Job {
        self.job
    }

    pub fn poll(&self) -> Poll<Event> {
        self.task.poll()
    }
}

pub fn detect(job: Job, source: Source, request: DetectRequest) -> Analysis {
    analyse(job, move |cancel, sink| {
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
    analyse(job, move |cancel, sink| {
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
    analyse(job, move |cancel, sink| {
        run::relation(source, target, workers, cancel, sink);
        Ok(())
    })
}

pub fn coloc(job: Job, source: Source, request: ColocRequest) -> Analysis {
    analyse(job, move |cancel, sink| {
        let reader = Image::open(&source)?;
        run::coloc(&reader, request, cancel, sink)
    })
}

fn analyse(
    job: Job,
    work: impl FnOnce(&AtomicBool, &mut dyn FnMut(Event)) -> Result<(), String> + Send + 'static,
) -> Analysis {
    let task = Task::spawn(move |cancel, sink| {
        if let Err(detail) = work(cancel, sink) {
            sink(Event::Failed(detail));
        }
    });
    Analysis { job, task }
}
