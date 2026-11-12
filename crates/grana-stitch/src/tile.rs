use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use grana_image::{Image, Source};

use crate::Error;

pub fn tiles(path: &Path) -> Result<Vec<Source>, Error> {
    if grana_image::is_image(path) {
        return Ok(vec![Source::file(path.to_path_buf())]);
    }
    let entries = std::fs::read_dir(path)
        .map_err(|error| Error::new(format!("failed to read {}: {error}", path.display())))?;
    let mut files = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| grana_image::is_image(path))
        .collect::<Vec<PathBuf>>();
    files.sort_by(|left, right| natural(left, right));
    if files.is_empty() {
        return Err(Error::new(format!(
            "{} holds no image tile",
            path.display()
        )));
    }
    Ok(files.into_iter().map(Source::file).collect())
}

pub fn channel_of(tile: &Source, name: &str) -> Result<usize, Error> {
    let image = Image::open_with_cache(tile, None)
        .map_err(|error| Error::new(format!("failed to open {}: {error}", tile.label())))?;
    let channels = image.pyramid().channels();
    channels
        .iter()
        .position(|channel| channel.name() == name)
        .ok_or_else(|| {
            let names = channels
                .iter()
                .map(|channel| channel.name().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Error::new(format!(
                "{} names no channel {name}; it holds {names}",
                tile.label()
            ))
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
