use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const LIMIT: usize = 12;
const FILE: &str = "recent.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Project,
    Remote,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    kind: Kind,
    name: String,
    target: String,
    series: usize,
}

impl Entry {
    fn new(kind: Kind, name: &str, target: &str, series: usize) -> Self {
        assert!(
            !name.trim().is_empty(),
            "a recent entry names the dataset it holds"
        );
        assert!(
            !target.trim().is_empty(),
            "a recent entry names where its dataset lives"
        );
        Self {
            kind,
            name: name.trim().to_string(),
            target: target.to_string(),
            series,
        }
    }

    pub fn image(path: &Path, name: &str, series: usize) -> Self {
        Self::new(Kind::Image, name, &path.to_string_lossy(), series)
    }

    pub fn project(path: &Path) -> Self {
        let name = path.file_stem().map_or_else(
            || path.to_string_lossy().into_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        Self::new(Kind::Project, &name, &path.to_string_lossy(), 0)
    }

    pub fn remote(url: &str, name: &str, series: usize) -> Self {
        Self::new(Kind::Remote, name, url, series)
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn series(&self) -> usize {
        self.series
    }

    pub fn path(&self) -> Option<PathBuf> {
        match self.kind {
            Kind::Image | Kind::Project => Some(PathBuf::from(&self.target)),
            Kind::Remote => None,
        }
    }

    fn present(&self) -> bool {
        self.path().is_none_or(|path| path.exists())
    }
}

#[derive(Default)]
pub struct Recent {
    entries: Vec<Entry>,
    root: Option<PathBuf>,
}

impl Recent {
    pub fn persistent() -> Self {
        let Some(root) = storage() else {
            return Self::default();
        };
        let entries = std::fs::read_to_string(&root)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<Entry>>(&text).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(Entry::present)
            .take(LIMIT)
            .collect();
        Self {
            entries,
            root: Some(root),
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    pub fn record(&mut self, entry: Entry) -> std::io::Result<()> {
        let duplicate = self.entries.iter().position(|held| {
            held.kind == entry.kind && held.target == entry.target && held.series == entry.series
        });
        if let Some(index) = duplicate {
            let held = self.entries.remove(index);
            self.entries.insert(0, held);
            return Ok(());
        }
        self.entries.insert(0, entry);
        self.entries.truncate(LIMIT);
        self.store()
    }

    fn store(&self) -> std::io::Result<()> {
        let Some(root) = self.root.as_ref() else {
            return Ok(());
        };
        if let Some(parent) = root.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.entries)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        std::fs::write(root, text)
    }
}

fn storage() -> Option<PathBuf> {
    grana_dir::data().map(|root| root.join(FILE))
}
