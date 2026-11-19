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
    let folder = match grana_stitch::folder(std::path::Path::new(folder)) {
        Ok(folder) => folder,
        Err(detail) => {
            grana_log::error!(target: "grana::stitch", %detail, "failed to read the tiles");
            eprintln!("{detail}");
            return Some(ExitCode::FAILURE);
        }
    };
    if rows * columns != folder.count() {
        grana_log::error!(
            target: "grana::stitch",
            rows,
            columns,
            tiles = folder.count(),
            "the grid does not cover the tiles"
        );
        eprintln!(
            "the {columns} × {rows} grid does not cover {} tiles",
            folder.count()
        );
        return Some(ExitCode::FAILURE);
    }
    if series >= folder.series() {
        grana_log::error!(
            target: "grana::stitch",
            series,
            series_count = folder.series(),
            "the series is not present in the tiles"
        );
        eprintln!(
            "the tiles hold {} series and series {series} was requested",
            folder.series()
        );
        return Some(ExitCode::FAILURE);
    }
    if channel >= folder.channels(series).len() {
        grana_log::error!(
            target: "grana::stitch",
            channel,
            channels = folder.channels(series).len(),
            "the channel is not present in the tiles"
        );
        eprintln!(
            "the tiles name {} channels in series {series} and channel {channel} was requested",
            folder.channels(series).len()
        );
        return Some(ExitCode::FAILURE);
    }
    let request = Request {
        tiles: folder.tiles().to_vec(),
        output: PathBuf::from(output),
        rows,
        columns,
        channel,
        series,
    };
    match grana_stitch::stitch(&request, &grana_exec::Cancel::new()) {
        Ok(summary) => {
            grana_log::info!(
                target: "grana::stitch",
                output = %request.output.display(),
                width = summary.width(),
                height = summary.height(),
                depth = summary.depth(),
                tiles = summary.offsets().len(),
                "the stitched volume was written"
            );
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
            grana_log::error!(target: "grana::stitch", %error, "the stitch failed");
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
