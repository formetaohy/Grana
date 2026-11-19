use std::path::Path;

use grana_exec::Cancel;
use grana_image::Source;
use grana_protocol::StitchSpec;
use grana_stitch::Request;

use super::{DatasetEntry, Event, Progress};

pub(super) const STITCHED: &str = ".stitch";

pub(super) fn place(
    output: &Path,
    dataset: &mut DatasetEntry,
    spec: &StitchSpec,
    progress: &Progress<'_>,
    cancel: &Cancel,
) -> Result<(), String> {
    let folder = grana_stitch::folder(dataset.path()).map_err(|error| error.to_string())?;
    if folder.count() != spec.tiles() {
        return Err(format!(
            "{} holds {} tiles and the grid names {}",
            dataset.path().display(),
            folder.count(),
            spec.tiles()
        ));
    }
    let channel = folder
        .channel(&spec.channel)
        .map_err(|error| error.to_string())?;
    let tiles = folder.tiles().to_vec();
    let folder = output.join(STITCHED).join(dataset.key());
    let path = folder.join(format!(
        "{}-{}x{}-c{}-s{}.ome.zarr",
        grana_scene::slug(&dataset.title()),
        spec.rows,
        spec.columns,
        spec.channel,
        dataset.scene()
    ));
    if current(&path, &tiles) {
        dataset.set_stitched(path);
        return Ok(());
    }
    let _ = progress.events.send(Event::Step {
        index: progress.index,
        label: format!("stitching {} tiles of {}", tiles.len(), dataset.title()),
    });
    std::fs::create_dir_all(&folder)
        .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;
    let request = Request {
        tiles,
        output: path.clone(),
        rows: spec.rows,
        columns: spec.columns,
        channel,
        series: dataset.scene(),
    };
    match grana_stitch::stitch(&request, cancel) {
        Ok(_) => {}
        Err(_) if cancel.cancelled() => return Ok(()),
        Err(error) => return Err(error.to_string()),
    }
    dataset.set_stitched(path);
    Ok(())
}

fn current(path: &Path, tiles: &[Source]) -> bool {
    let Ok(stitched) = std::fs::metadata(path).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    tiles.iter().all(|tile| {
        tile.path()
            .and_then(|path| std::fs::metadata(path).ok())
            .and_then(|metadata| metadata.modified().ok())
            .is_none_or(|modified| modified <= stitched)
    })
}
