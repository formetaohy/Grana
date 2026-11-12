use std::path::Path;

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
) -> Result<(), String> {
    let tiles = grana_stitch::tiles(dataset.path()).map_err(|error| error.to_string())?;
    if tiles.len() != spec.tiles() {
        return Err(format!(
            "{} holds {} tiles and the grid names {}",
            dataset.path().display(),
            tiles.len(),
            spec.tiles()
        ));
    }
    let channel =
        grana_stitch::channel_of(&tiles[0], &spec.channel).map_err(|error| error.to_string())?;
    let folder = output.join(STITCHED).join(dataset.key());
    let path = folder.join(format!("{}.ome.zarr", grana_scene::slug(&dataset.title())));
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
    grana_stitch::stitch(&request).map_err(|error| error.to_string())?;
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
