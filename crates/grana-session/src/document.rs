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
pub enum Target {
    Document,
    Field(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Save,
    Open,
}

pub struct Document {
    task: Progress<Outcome>,
    path: PathBuf,
    hash: Option<u64>,
    target: Target,
    label: String,
}

impl Document {
    pub fn save(project: Project, path: PathBuf, hash: Option<u64>) -> Self {
        Self::save_as(project, path, hash, Target::Document)
    }

    pub fn save_field(project: Project, path: PathBuf, hash: Option<u64>, series: usize) -> Self {
        Self::save_as(project, path, hash, Target::Field(series))
    }

    fn save_as(project: Project, path: PathBuf, hash: Option<u64>, target: Target) -> Self {
        let headline = label(Direction::Save, &path);
        let destination = path.clone();
        let task = Progress::spawn(Role::Write, move |cancel, progress| {
            saved(project.write(&destination, cancel, progress))
        });
        Self {
            task,
            path,
            hash,
            target,
            label: headline,
        }
    }

    pub fn open(path: PathBuf, store: Arc<SpillStore>) -> Self {
        let headline = label(Direction::Open, &path);
        let target = path.clone();
        let task = Progress::spawn(Role::Read, move |cancel, progress| {
            opened(Project::read(&target, store, cancel, progress))
        });
        Self {
            task,
            path,
            hash: None,
            target: Target::Document,
            label: headline,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn hash(&self) -> Option<u64> {
        self.hash
    }

    pub fn target(&self) -> Target {
        self.target
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

fn label(direction: Direction, path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    match direction {
        Direction::Save => format!("Saving {name}"),
        Direction::Open => format!("Opening {name}"),
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
