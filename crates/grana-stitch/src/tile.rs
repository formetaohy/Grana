use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use grana_image::{Format, Image, Source};

use crate::Error;

const CACHE_LIMIT: usize = 8;

type Entry = (PathBuf, Option<SystemTime>, Arc<Folder>);

pub struct Folder {
    tiles: Vec<Source>,
    channels: Vec<String>,
    series: usize,
    rows: usize,
    columns: usize,
}

impl Folder {
    pub fn tiles(&self) -> &[Source] {
        &self.tiles
    }

    pub fn channels(&self) -> &[String] {
        &self.channels
    }

    pub fn channel(&self, name: &str) -> Result<usize, Error> {
        self.channels
            .iter()
            .position(|channel| channel == name)
            .ok_or_else(|| {
                Error::new(format!(
                    "the tiles name no channel {name}; they hold {}",
                    self.channels.join(", ")
                ))
            })
    }

    pub fn series(&self) -> usize {
        self.series
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn count(&self) -> usize {
        self.tiles.len()
    }
}

pub fn folder(path: &Path) -> Result<Arc<Folder>, Error> {
    let stamp = stamp(path);
    if let Some(found) = cached(path, stamp) {
        return Ok(found);
    }
    let scanned = Arc::new(unfold(path)?);
    store(path, stamp, Arc::clone(&scanned));
    Ok(scanned)
}

pub fn unfold(path: &Path) -> Result<Folder, Error> {
    let files = if grana_image::is_image(path) {
        vec![path.to_path_buf()]
    } else {
        let entries = std::fs::read_dir(path)
            .map_err(|error| Error::new(format!("failed to read {}: {error}", path.display())))?;
        let mut files = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| grana_image::is_image(path))
            .collect::<Vec<PathBuf>>();
        files.sort_by(|left, right| natural(left, right));
        files
    };
    let first = files
        .first()
        .ok_or_else(|| Error::new(format!("{} holds no image tile", path.display())))?;
    let image = Image::open(&Source::file(first.clone()))
        .map_err(|error| Error::new(format!("failed to open {}: {error}", first.display())))?;
    let channels = image
        .pyramid()
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let series = match image.format() {
        Some(Format::Tiff) | None => 1,
        Some(_) => series_of(&files)?,
    };
    let (rows, columns) = default_grid(files.len());
    Ok(Folder {
        tiles: files.into_iter().map(Source::file).collect(),
        channels,
        series,
        rows,
        columns,
    })
}

pub fn default_grid(count: usize) -> (usize, usize) {
    assert!(count > 0, "a tile grid holds at least one tile");
    let mut rows = (count as f64).sqrt().round().max(1.0) as usize;
    while !count.is_multiple_of(rows) && rows > 1 {
        rows -= 1;
    }
    (rows, count / rows)
}

fn series_of(files: &[PathBuf]) -> Result<usize, Error> {
    let mut count = None;
    for file in files {
        let image = Image::open(&Source::file(file.clone()))
            .map_err(|error| Error::new(format!("failed to open {}: {error}", file.display())))?;
        let scenes = image.series().len();
        match count {
            None => count = Some(scenes),
            Some(known) if known == scenes => {}
            Some(known) => {
                return Err(Error::new(format!(
                    "{} holds {scenes} series and the first tile holds {known}",
                    file.display()
                )));
            }
        }
    }
    count.ok_or_else(|| Error::new("a mosaic holds at least one tile"))
}

fn stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn cached(path: &Path, stamp: Option<SystemTime>) -> Option<Arc<Folder>> {
    let cache = cache().lock().ok()?;
    cache
        .iter()
        .find(|entry| entry.0 == path && entry.1 == stamp)
        .map(|entry| Arc::clone(&entry.2))
}

fn store(path: &Path, stamp: Option<SystemTime>, folder: Arc<Folder>) {
    let Ok(mut cache) = cache().lock() else {
        return;
    };
    cache.retain(|entry| entry.0 != path);
    cache.push((path.to_path_buf(), stamp, folder));
    while cache.len() > CACHE_LIMIT {
        cache.remove(0);
    }
}

fn cache() -> &'static Mutex<Vec<Entry>> {
    static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
    &CACHE
}

fn natural(left: &Path, right: &Path) -> Ordering {
    let left = left
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let right = right
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let mut left = left.chars().peekable();
    let mut right = right.chars().peekable();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let (x, y) = (number(a, &mut left), number(b, &mut right));
                if x != y {
                    return x.cmp(&y);
                }
            }
            (Some(a), Some(b)) if a != b => return a.cmp(&b),
            _ => {}
        }
    }
}

fn number(first: char, rest: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u64 {
    let mut value = digit(first);
    while let Some(next) = rest.peek().copied().filter(|digit| digit.is_ascii_digit()) {
        value = value * 10 + digit(next);
        rest.next();
    }
    value
}

fn digit(value: char) -> u64 {
    u64::from(value as u32 - '0' as u32)
}
