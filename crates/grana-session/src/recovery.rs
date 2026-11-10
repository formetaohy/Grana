use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use grana_project::Project;

use crate::task::{Poll, Task};

const FILE: &str = "recovery.json";

const SNAPSHOT: &str = "snapshot.grana";

const STALE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    name: String,
    target: String,
    project: Option<String>,
    saved: u64,
}

impl Entry {
    pub fn new(name: &str, target: &str, project: Option<&Path>) -> Self {
        assert!(
            !name.trim().is_empty(),
            "a recovery names the dataset it holds"
        );
        assert!(
            !target.trim().is_empty(),
            "a recovery names where its dataset lives"
        );
        Self {
            name: name.trim().to_string(),
            target: target.to_string(),
            project: project.map(|path| path.to_string_lossy().into_owned()),
            saved: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn project(&self) -> Option<&Path> {
        self.project.as_deref().map(Path::new)
    }

    pub fn age(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        now.saturating_sub(self.saved)
    }
}

enum Event {
    Done(Box<Result<Entry, String>>),
}

pub struct Recovery {
    root: Option<PathBuf>,
    entry: Option<Entry>,
    task: Option<Task<Event>>,
    refused: bool,
}

impl Default for Recovery {
    fn default() -> Self {
        Self::idle()
    }
}

impl Recovery {
    pub fn persistent() -> Self {
        let root = grana_dir::cache().map(|root| root.join("recovery"));
        Self::at(root)
    }

    pub fn at(root: Option<PathBuf>) -> Self {
        let mut store = Self::idle();
        let Some(root) = root else {
            return store;
        };
        let entry = read(&root.join(FILE));
        if entry
            .as_ref()
            .is_some_and(|entry| entry.age() > STALE.as_secs() || !root.join(SNAPSHOT).is_file())
        {
            discard(&root);
        } else {
            store.entry = entry;
        }
        store.root = Some(root);
        store
    }

    fn idle() -> Self {
        Self {
            root: None,
            entry: None,
            task: None,
            refused: false,
        }
    }

    pub fn entry(&self) -> Option<&Entry> {
        self.entry.as_ref()
    }

    pub fn snapshot(&self) -> Option<PathBuf> {
        let root = self.root.as_ref()?;
        let entry = self.entry.as_ref()?;
        let path = root.join(SNAPSHOT);
        (entry.age() <= STALE.as_secs() && path.is_file()).then_some(path)
    }

    pub fn busy(&self) -> bool {
        self.task.is_some()
    }

    pub fn refused(&self) -> bool {
        self.refused
    }

    pub fn refuse(&mut self) {
        self.refused = true;
    }

    pub fn start(&mut self, project: Project, entry: Entry) -> bool {
        let Some(root) = self.root.clone() else {
            return false;
        };
        if self.task.is_some() {
            return false;
        }
        let target = root.join(SNAPSHOT);
        let label = entry.clone();
        self.task = Some(Task::spawn(move |cancel, sink| {
            let written = std::fs::create_dir_all(&root)
                .map_err(|error| format!("failed to create {}: {error}", root.display()))
                .and_then(|()| {
                    project
                        .write(&target, cancel, &mut |_, _| {})
                        .map_err(|error| error.to_string())
                })
                .and_then(|()| {
                    let text = serde_json::to_string_pretty(&label)
                        .map_err(|error| format!("the recovery does not serialize: {error}"))?;
                    grana_file::write(&root.join(FILE), text.as_bytes())
                        .map_err(|error| format!("failed to write the recovery record: {error}"))
                });
            sink(Event::Done(Box::new(
                written
                    .map(|()| label)
                    .map_err(|error| format!("autosave failed: {error}")),
            )));
        }));
        true
    }

    pub fn poll(&mut self) -> Option<Result<Entry, String>> {
        let task = self.task.as_ref()?;
        match task.poll() {
            Poll::Event(Event::Done(outcome)) => {
                self.task = None;
                match *outcome {
                    Ok(entry) => {
                        self.refused = false;
                        self.entry = Some(entry.clone());
                        Some(Ok(entry))
                    }
                    Err(detail) => Some(Err(detail)),
                }
            }
            Poll::Empty => None,
            Poll::Closed => {
                self.task = None;
                Some(Err("the autosave worker stopped".to_string()))
            }
        }
    }

    pub fn discard(&mut self) {
        self.task = None;
        self.entry = None;
        if let Some(root) = self.root.as_ref() {
            discard(root);
        }
    }
}

fn discard(root: &Path) {
    let _ = std::fs::remove_file(root.join(SNAPSHOT));
    let _ = std::fs::remove_file(root.join(FILE));
}

fn read(path: &Path) -> Option<Entry> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Entry>(&text) {
        Ok(entry) => Some(entry),
        Err(_) => {
            let _ = std::fs::remove_file(path);
            None
        }
    }
}
