use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_exec::Role;
use grana_project::{Error, Project};
use grana_store::SpillStore;

use crate::task::{Poll, Progress};

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
    task: Progress<Outcome>,
    path: PathBuf,
    hash: Option<u64>,
    label: String,
}

impl Document {
    pub fn save(project: Project, path: PathBuf, hash: Option<u64>) -> Self {
        let headline = label(Transfer::Save, &path);
        let target = path.clone();
        let task = Progress::spawn(Role::Write, move |cancel, progress| {
            saved(project.write(&target, cancel, progress))
        });
        Self {
            task,
            path,
            hash,
            label: headline,
        }
    }

    pub fn open(path: PathBuf, store: Arc<SpillStore>) -> Self {
        let headline = label(Transfer::Open, &path);
        let target = path.clone();
        let task = Progress::spawn(Role::Read, move |cancel, progress| {
            opened(Project::read(&target, store, cancel, progress))
        });
        Self {
            task,
            path,
            hash: None,
            label: headline,
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
        self.task.progress()
    }

    pub fn stepped(&mut self) -> bool {
        self.task.stepped()
    }

    pub fn poll(&mut self) -> Poll<Outcome> {
        self.task.poll()
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

fn saved(outcome: Result<(), Error>) -> Outcome {
    match outcome {
        Ok(()) => Outcome::Saved,
        Err(Error::Cancelled) => Outcome::Cancelled,
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

fn opened(outcome: Result<Project, Error>) -> Outcome {
    match outcome {
        Ok(project) => Outcome::Opened(Box::new(project)),
        Err(Error::Cancelled) => Outcome::Cancelled,
        Err(error) => Outcome::Failed(error.to_string()),
    }
}
