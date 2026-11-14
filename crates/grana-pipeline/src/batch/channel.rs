use std::path::Path;

use grana_scene::{Scene, slug};

use super::{Event, Task};
use crate::channels::Channels;
use crate::export::{ChannelLayer, DatasetSpec, LabelLayer, cropped_levels, write_dataset};

pub(super) fn write(
    task: &Task<'_>,
    scene: &Scene,
    channels: &Channels,
    folder: &Path,
) -> Result<(), String> {
    let stored = &scene.dataset.pyramid().levels()[..scene.dataset.stored_levels()];
    if stored.is_empty() {
        return Err("the dataset holds no stored resolution level".to_string());
    }
    let cropped = cropped_levels(scene.region, stored)?;
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
        .filter_map(|set| LabelLayer::of(set, &cropped.levels))
        .collect::<Vec<_>>();
    let frames = scene.frame_count();
    let drift = (0..frames)
        .map(|time| scene.drift_warps(time))
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
            levels: cropped.levels,
            origins: cropped.origins,
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
