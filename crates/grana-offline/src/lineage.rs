use std::path::{Path, PathBuf};

use grana_scene::{ObjectSetId, Scene};
use grana_ui::Ui;
use wgpu::{Device, Queue, TextureFormat};

use crate::vector;

pub(crate) fn write(
    device: &Device,
    queue: &Queue,
    device_format: TextureFormat,
    scene: &Scene,
    set: ObjectSetId,
    path: &Path,
    size: [u32; 2],
) -> Result<PathBuf, String> {
    let Some(object) = scene.objects.get(set) else {
        return Err("the lineage names an object set the scene does not hold".to_string());
    };
    let Some(tracks) = object.tracks().map(|layer| layer.set()) else {
        return Err(format!("{} holds no tracked lineage", object.name()));
    };
    let frames = scene.frame_count();
    if tracks
        .tracks()
        .iter()
        .flat_map(|track| track.nodes())
        .any(|node| node.time() >= frames)
    {
        return Err(format!(
            "the lineage of {} names a frame outside the {frames} frames",
            object.name()
        ));
    }
    let page = grana_figure::lineage::page_size(tracks.len(), size[0])?;
    if page != size {
        return Err(format!(
            "a lineage of {} tracks spans {} × {} points, not {} × {}",
            tracks.len(),
            page[0],
            page[1],
            size[0],
            size[1]
        ));
    }
    let format = vector::format(path).ok_or_else(|| {
        format!(
            "a lineage page is saved as SVG or PDF, not `{}`",
            path.display()
        )
    })?;
    let colors = (0..tracks.len())
        .map(|track| object.track_color(track))
        .collect::<Vec<_>>();
    let mut ui: Ui<()> = Ui::new(device, queue, device_format);
    ui.resize(size);
    let marks = grana_figure::lineage::marks(
        &mut ui,
        size,
        &grana_figure::lineage::Lineage {
            title: object.name(),
            set: tracks,
            colors: &colors,
            frames: scene.frame_count(),
            time: scene.time(),
        },
    );
    vector::write_marks(path, format, size, &marks)?;
    Ok(path.to_path_buf())
}
