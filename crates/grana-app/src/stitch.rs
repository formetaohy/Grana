use std::path::PathBuf;
use std::process::ExitCode;

use grana_stitch::Request;

pub fn command(arguments: &[String]) -> Option<ExitCode> {
    let mut paths = Vec::new();
    let mut channel = 0usize;
    let mut series = 0usize;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--channel" => {
                channel = arguments.get(index + 1)?.parse().ok()?;
                index += 2;
            }
            "--series" => {
                series = arguments.get(index + 1)?.parse().ok()?;
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
    let tiles = match grana_stitch::tiles(std::path::Path::new(folder)) {
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
        series,
    };
    match grana_stitch::stitch(&request) {
        Ok(summary) => {
            println!(
                "{} × {} × {} · {} channels · {} frames · {} tiles · series {} · {} levels · {:.1} MiB",
                summary.width(),
                summary.height(),
                summary.depth(),
                summary.channels(),
                summary.frames(),
                summary.offsets().len(),
                summary.series() + 1,
                summary.levels(),
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
