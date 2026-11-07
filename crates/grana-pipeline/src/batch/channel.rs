use std::path::Path;
use std::sync::atomic::Ordering;

use grana_scene::{Scene, slug};
use grana_zarr::{ImageSpec, WriteChannel};

use super::{Event, Task};
use crate::channels::Channels;

pub(super) fn write(
    task: &Task<'_>,
    scene: &Scene,
    channels: &Channels,
    folder: &Path,
) -> Result<(), String> {
    let derived = scene.derived();
    if derived.is_empty() {
        return Ok(());
    }
    let base = scene.base_channels();
    let levels = &scene.dataset.pyramid().levels()[..scene.dataset.stored_levels()];
    if levels.is_empty() {
        return Err("the dataset holds no stored resolution level".to_string());
    }
    let frames = scene.frame_count();
    let interval = scene.dataset.frame_interval();
    for offset in 0..derived.len() {
        let channel = base + offset;
        let info = scene
            .dataset
            .channels()
            .get(channel)
            .ok_or_else(|| format!("the derived channel {channel} lies outside the dataset"))?;
        let name = info.name().to_string();
        let _ = task.events.send(Event::Step {
            index: task.index,
            label: format!("exporting the channel {name}"),
        });
        let path = folder.join(format!("{}.ome.zarr", slug(&name)));
        let mut fill = |level: usize,
                        time: u32,
                        _: usize,
                        origin: [u32; 3],
                        extent: [u32; 3],
                        data: &mut [f32]| {
            if task.cancel.load(Ordering::Relaxed) {
                return Err("the channel export was cancelled".to_string());
            }
            let ranges = [
                origin[0]..origin[0] + extent[0],
                origin[1]..origin[1] + extent[1],
                origin[2]..origin[2] + extent[2],
            ];
            let shift = scene.drift().offset(time);
            let values = channels
                .read_region(time, level, channel, &ranges, shift)
                .map_err(|error| error.to_string())?;
            values.write_values(data);
            Ok(())
        };
        let mut progress = |done: usize, total: usize| {
            let _ = task.events.send(Event::Progress {
                index: task.index,
                done,
                total,
            });
        };
        let spec = [WriteChannel {
            name: &name,
            tint: info.tint(),
            range: info.window(),
        }];
        let title = format!("{} {name}", scene.dataset.name());
        grana_zarr::write_image(
            &path,
            ImageSpec {
                name: &title,
                channels: &spec,
                levels,
                frames,
                interval,
                fill: &mut fill,
                progress: &mut progress,
            },
        )
        .map_err(|error| error.to_string())?;
        if !task.gate.hold(task.cancel) {
            return Err("the channel export was cancelled".to_string());
        }
    }
    Ok(())
}
