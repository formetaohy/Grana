use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use grana_project::Project;

use crate::task::Progress;

const FILE: &str = "recovery.toml";

const SNAPSHOT: &str = "snapshot.grana";

const STALE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    name: String,
    target: String,
    project: Option<String>,
    fingerprint: String,
    snapshot: u64,
    saved: u64,
}

impl Entry {
    pub fn new(name: &str, target: &str, project: Option<&Path>, fingerprint: &str) -> Self {
        assert!(
            !name.trim().is_empty(),
            "a recovery names the dataset it holds"
        );
        assert!(
            !target.trim().is_empty(),
            "a recovery names where its dataset lives"
        );
        assert!(
            fingerprint.len() == 64 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "a recovery names the fingerprint of its dataset"
        );
        Self {
            name: name.trim().to_string(),
            target: target.to_string(),
            project: project.map(|path| path.to_string_lossy().into_owned()),
            fingerprint: fingerprint.to_string(),
            snapshot: 0,
            saved: now(),
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

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn age(&self) -> u64 {
        now().saturating_sub(self.saved)
    }
}

pub struct Recovery {
    root: Option<PathBuf>,
    entry: Option<Entry>,
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
        if let Err(error) = grana_file::recover(&root) {
            grana_log::warn!(target: "grana::session", path = %root.display(), %error, "the recovery unit left an unclearable artifact");
        }
        match read(&root.join(FILE)) {
            Record::Absent => discard(&root),
            Record::Unreadable(detail) => {
                grana_log::warn!(
                    target: "grana::session",
                    path = %root.display(),
                    %detail,
                    "the recovery record cannot be read, so the snapshot beside it was kept"
                );
            }
            Record::Held(entry) => {
                if entry.age() <= STALE.as_secs() && holds(&root, &entry) {
                    store.entry = Some(entry);
                } else {
                    grana_log::warn!(
                        target: "grana::session",
                        path = %root.display(),
                        name = entry.name(),
                        "the recovery record does not describe the snapshot beside it, so the unit was discarded"
                    );
                    discard(&root);
                }
            }
        }
        store.root = Some(root);
        store
    }

    fn idle() -> Self {
        Self {
            root: None,
            entry: None,
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
        holds(root, entry).then_some(path)
    }

    pub fn refused(&self) -> bool {
        self.refused
    }

    pub fn refuse(&mut self) {
        self.refused = true;
    }

    pub fn start(&self, project: Project, entry: Entry) -> Option<Progress<Result<Entry, String>>> {
        let root = self.root.clone()?;
        Some(Progress::spawn(
            grana_exec::Role::Write,
            move |cancel, _| store(&root, project, entry, cancel),
        ))
    }

    pub fn write(&mut self, project: Project, entry: Entry) -> Result<(), String> {
        let Some(root) = self.root.clone() else {
            return Err("the recovery store is not available".to_string());
        };
        let held = store(&root, project, entry, &AtomicBool::new(false))?;
        self.refused = false;
        self.entry = Some(held);
        Ok(())
    }

    pub fn accept(&mut self, entry: Entry) {
        self.refused = false;
        self.entry = Some(entry);
    }

    pub fn discard(&mut self) {
        self.entry = None;
        if let Some(root) = self.root.as_ref() {
            discard(root);
        }
    }
}

fn store(
    root: &Path,
    project: Project,
    mut entry: Entry,
    cancel: &AtomicBool,
) -> Result<Entry, String> {
    let atomic = grana_file::Atomic::directory(root)
        .map_err(|error| format!("failed to prepare {}: {error}", root.display()))?;
    let snapshot = atomic.staging().join(SNAPSHOT);
    project
        .write(&snapshot, cancel, &mut |_, _| {})
        .map_err(|error| format!("autosave failed: {error}"))?;
    if cancel.load(Ordering::Relaxed) {
        return Err("autosave cancelled".to_string());
    }
    entry.snapshot = std::fs::metadata(&snapshot)
        .map_err(|error| format!("failed to measure the autosave: {error}"))?
        .len();
    let text = toml::to_string_pretty(&entry)
        .map_err(|error| format!("the recovery does not serialize: {error}"))?;
    grana_file::write(&atomic.staging().join(FILE), text.as_bytes())
        .map_err(|error| format!("failed to write the recovery record: {error}"))?;
    atomic
        .commit()
        .map_err(|error| format!("autosave failed: {error}"))?;
    Ok(entry)
}

fn holds(root: &Path, entry: &Entry) -> bool {
    if grana_volume::Fingerprint::from_hex(&entry.fingerprint).is_none() {
        return false;
    }
    std::fs::metadata(root.join(SNAPSHOT))
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == entry.snapshot)
}

fn discard(root: &Path) {
    for name in [SNAPSHOT, FILE] {
        if let Err(error) = std::fs::remove_file(root.join(name))
            && error.kind() != std::io::ErrorKind::NotFound
        {
            grana_log::warn!(target: "grana::session", path = %root.join(name).display(), %error, "the recovery file cannot be removed");
        }
    }
}

enum Record {
    Absent,
    Held(Entry),
    Unreadable(String),
}

fn read(path: &Path) -> Record {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Record::Absent,
        Err(error) => return Record::Unreadable(error.to_string()),
    };
    match toml::from_str::<Entry>(&text) {
        Ok(entry) => Record::Held(entry),
        Err(error) => Record::Unreadable(error.to_string()),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}
