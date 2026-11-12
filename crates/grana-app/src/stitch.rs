use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use grana_image::Source;
use grana_stitch::Request;

pub fn command(arguments: &[String]) -> Option<ExitCode> {
    let mut paths = Vec::new();
    let mut channel = 0usize;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--channel" => {
                channel = arguments.get(index + 1)?.parse().ok()?;
                index += 2;
            }
            option if option.starts_with("--") => return None,
            value => {
                paths.push(value.to_string());
                index += 1;
            }
        }
    }
    let [folder, output, grid] = paths.as_slice() else {
        return None;
    };
    let (rows, columns) = grid_size(grid)?;
    let tiles = match collect(Path::new(folder)) {
        Ok(tiles) => tiles,
        Err(detail) => {
            eprintln!("{detail}");
            return Some(ExitCode::FAILURE);
        }
    };
    let request = Request {
        tiles,
        output: PathBuf::from(output),
        rows,
        columns,
        channel,
    };
    match grana_stitch::stitch(&request) {
        Ok(summary) => {
            println!(
                "{} × {} × {} · {} channels · {} frames · {} tiles · {:.1} MiB",
                summary.width(),
                summary.height(),
                summary.depth(),
                summary.channels(),
                summary.frames(),
                summary.offsets().len(),
                summary.bytes() as f64 / (1024.0 * 1024.0)
            );
            for (index, offset) in summary.offsets().iter().enumerate() {
                println!("  tile {} at {},{}", index + 1, offset[0], offset[1]);
            }
            println!("{}", request.output.display());
            Some(ExitCode::SUCCESS)
        }
        Err(error) => {
            eprintln!("{error}");
            Some(ExitCode::FAILURE)
        }
    }
}

fn grid_size(text: &str) -> Option<(usize, usize)> {
    let (rows, columns) = text.split_once(['x', 'X'])?;
    let rows = rows.parse().ok()?;
    let columns = columns.parse().ok()?;
    (rows > 0 && columns > 0).then_some((rows, columns))
}

fn collect(path: &Path) -> Result<Vec<Source>, String> {
    if grana_image::is_image(path) {
        return Ok(vec![Source::file(path.to_path_buf())]);
    }
    let entries = std::fs::read_dir(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut tiles = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| grana_image::is_image(path))
        .collect::<Vec<_>>();
    tiles.sort_by(|left, right| natural(left, right));
    if tiles.is_empty() {
        return Err(format!("{} holds no image tile", path.display()));
    }
    Ok(tiles.into_iter().map(Source::file).collect())
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
    while let Some(digit) = rest.peek().copied().filter(|digit| digit.is_ascii_digit()) {
        value = value * 10 + self::digit(digit);
        rest.next();
    }
    value
}

fn digit(value: char) -> u64 {
    u64::from(value as u32 - '0' as u32)
}
