use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_geometry::GeometryStore;
use grana_project::{Error, Project};

use crate::task::{Poll, Task};

enum Event {
    Progress(usize, usize),
    Done(Outcome),
}

pub enum Outcome {
    Saved,
    Opened(Box<Project>),
    Failed(String),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Transfer {
    Save,
    Open,
}

pub struct Document {
    task: Task<Event>,
    path: PathBuf,
    hash: Option<u64>,
    label: String,
    progress: (usize, usize),
}

impl Document {
    pub fn save(project: Project, path: PathBuf, hash: Option<u64>) -> Self {
        let headline = label(Transfer::Save, &path);
        let target = path.clone();
        let task = Task::spawn(move |cancel, sink| {
            let outcome = {
                let mut progress = |done: usize, total: usize| sink(Event::Progress(done, total));
                project.write(&target, cancel, &mut progress)
            };
            sink(saved(outcome));
        });
        Self {
            task,
            path,
            hash,
            label: headline,
            progress: (0, 0),
        }
    }

    pub fn open(path: PathBuf, store: Arc<GeometryStore>) -> Self {
        let headline = label(Transfer::Open, &path);
        let target = path.clone();
        let task = Task::spawn(move |cancel, sink| {
            let outcome = {
                let mut progress = |done: usize, total: usize| sink(Event::Progress(done, total));
                Project::read(&target, store, cancel, &mut progress)
            };
            sink(opened(outcome));
        });
        Self {
            task,
            path,
            hash: None,
            label: headline,
            progress: (0, 0),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn hash(&self) -> Option<u64> {
        self.hash
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn progress(&self) -> (usize, usize) {
        self.progress
    }

    pub fn poll(&mut self) -> Poll<Outcome> {
        loop {
            match self.task.poll() {
                Poll::Event(Event::Progress(done, total)) => self.progress = (done, total),
                Poll::Event(Event::Done(outcome)) => return Poll::Event(outcome),
                Poll::Empty => return Poll::Empty,
                Poll::Closed => return Poll::Closed,
            }
        }
    }
}

fn label(transfer: Transfer, path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    match transfer {
        Transfer::Save => format!("Saving {name}"),
        Transfer::Open => format!("Opening {name}"),
    }
}

fn saved(outcome: Result<(), Error>) -> Event {
    Event::Done(match outcome {
        Ok(()) => Outcome::Saved,
        Err(Error::Cancelled) => Outcome::Cancelled,
        Err(error) => Outcome::Failed(error.to_string()),
    })
}

fn opened(outcome: Result<Project, Error>) -> Event {
    Event::Done(match outcome {
        Ok(project) => Outcome::Opened(Box::new(project)),
        Err(Error::Cancelled) => Outcome::Cancelled,
        Err(error) => Outcome::Failed(error.to_string()),
    })
}
