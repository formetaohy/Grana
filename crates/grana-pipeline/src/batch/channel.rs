use std::path::Path;

use grana_scene::{Scene, slug};

use super::{Event, Task};
use crate::channels::Channels;
use crate::export::{ChannelLayer, DatasetSpec, LabelLayer, write_dataset};

pub(super) fn write(
    task: &Task<'_>,
    scene: &Scene,
    channels: &Channels,
    folder: &Path,
) -> Result<(), String> {
    let levels = &scene.dataset.pyramid().levels()[..scene.dataset.stored_levels()];
    if levels.is_empty() {
        return Err("the dataset holds no stored resolution level".to_string());
    }
    let name = scene.dataset.name().to_string();
    let _ = task.events.send(Event::Step {
        index: task.index,
        label: format!("exporting the dataset {name}"),
    });
    let layers = scene
        .dataset
        .channels()
        .iter()
        .enumerate()
        .map(|(index, info)| ChannelLayer {
            index,
            name: info.name().to_string(),
            tint: info.tint(),
            range: info.window(),
        })
        .collect::<Vec<_>>();
    let labels = scene
        .objects
        .iter()
        .filter_map(LabelLayer::of)
        .collect::<Vec<_>>();
    let frames = scene.frame_count();
    let drift = (0..frames)
        .map(|time| scene.drift().warp(time).clone())
        .collect::<Vec<_>>();
    let path = folder.join(format!("{}.ome.zarr", slug(&name)));
    let mut progress = |done: usize, total: usize| {
        let _ = task.events.send(Event::Progress {
            index: task.index,
            done,
            total,
        });
    };
    write_dataset(
        DatasetSpec {
            path,
            name,
            channels: layers,
            levels: levels.to_vec(),
            placement: scene.dataset.pyramid().placement(),
            frames,
            interval: scene.dataset.frame_interval(),
            drift,
            labels,
        },
        channels,
        task.cancel,
        &mut progress,
    )
    .map_err(|error| error.to_string())?;
    if !task.gate.hold(task.cancel) {
        return Err("the dataset export was cancelled".to_string());
    }
    Ok(())
}
