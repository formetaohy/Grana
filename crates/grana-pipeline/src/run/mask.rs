use std::sync::Arc;

use grana_analysis::{LabelBuilder, LabelSlab};
use grana_object::{
    Mask, MaskEdit, MaskMode, MaskTool, ObjectAnchor, Roi, Shape, SurfaceSet, assign,
};
use grana_scene::{Drift, ObjectFrame};
use grana_volume::{Level, VoxelData};

use crate::channels::Channels;

const SLAB: u32 = 32;
const WAND_MARGIN: u32 = 16;
const WAND_GROWTH: u32 = 4;
const BLOCK_LIMIT: u64 = 512 << 20;

pub struct MaskRequest<'a> {
    pub channel: usize,
    pub level: usize,
    pub strokes: &'a [MaskEdit],
    pub drift: &'a Drift,
}

pub fn mask(
    reader: &Channels,
    frames: &[Option<Arc<ObjectFrame>>],
    voxel: f32,
    request: &MaskRequest<'_>,
) -> Result<Vec<Option<Arc<ObjectFrame>>>, String> {
    let mut edited = frames.to_vec();
    for (frame, strokes) in grouped(request.strokes) {
        let next = {
            let Some(entry) = edited.get(frame as usize) else {
                return Err(format!(
                    "the mask edits name frame {frame}, which the object set does not hold"
                ));
            };
            let Some(ObjectFrame::Surfaces(current)) = entry.as_deref() else {
                return Err("only surfaces carry an editable mask".to_string());
            };
            let targets = targets(current, &strokes, voxel, frame)?;
            let mut next = current.clone();
            let mut removed = Vec::new();
            for (object, strokes) in targets {
                match edit_object(reader, current, object, &strokes, frame, request)? {
                    Some(replacement) => {
                        next = next
                            .replaced(object, &replacement)
                            .map_err(|error| error.to_string())?
                            .ok_or_else(|| {
                                "the edited surface does not fit its object set".to_string()
                            })?;
                    }
                    None => removed.push(object),
                }
            }
            for object in removed.into_iter().rev() {
                let Some(pruned) = next.without(object).map_err(|error| error.to_string())? else {
                    break;
                };
                next = pruned;
            }
            next
        };
        edited[frame as usize] = (!next.is_empty()).then(|| Arc::new(ObjectFrame::Surfaces(next)));
    }
    Ok(edited)
}

fn grouped(strokes: &[MaskEdit]) -> Vec<(u32, Vec<&MaskEdit>)> {
    let mut frames: Vec<(u32, Vec<&MaskEdit>)> = Vec::new();
    for stroke in strokes {
        match frames
            .iter_mut()
            .find(|(frame, _)| *frame == stroke.frame())
        {
            Some((_, edits)) => edits.push(stroke),
            None => frames.push((stroke.frame(), vec![stroke])),
        }
    }
    frames
}

fn targets<'a>(
    current: &SurfaceSet,
    strokes: &[&'a MaskEdit],
    voxel: f32,
    frame: u32,
) -> Result<Vec<(usize, Vec<&'a MaskEdit>)>, String> {
    let anchors = current
        .objects()
        .iter()
        .map(|statistics| ObjectAnchor::new(statistics.centroid(), statistics.bounds()))
        .collect::<Vec<_>>();
    let recorded = strokes
        .iter()
        .map(|stroke| stroke.anchor())
        .collect::<Vec<_>>();
    let resolved = assign(&recorded, &anchors, voxel);
    let mut targets: Vec<(usize, Vec<&MaskEdit>)> = Vec::new();
    for (index, target) in resolved.into_iter().enumerate() {
        let Some(target) = target else {
            return Err(format!(
                "the mask edits of frame {frame} find no surface for stroke {index}"
            ));
        };
        match targets.iter_mut().find(|(object, _)| *object == target) {
            Some((_, edits)) => edits.push(strokes[index]),
            None => targets.push((target, vec![strokes[index]])),
        }
    }
    Ok(targets)
}

fn edit_object(
    reader: &Channels,
    current: &SurfaceSet,
    object: usize,
    strokes: &[&MaskEdit],
    frame: u32,
    request: &MaskRequest<'_>,
) -> Result<Option<SurfaceSet>, String> {
    let level = level_of(reader, request.level)?;
    let roi = Roi::new(vec![Some(single_shape(current, object)?)]);
    let base = roi.mask(level, 0)?;
    if base.is_empty() {
        return Ok(None);
    }
    let block = block(level, &base, strokes);
    let mut mask = base.extended(block.origin, block.extent);
    for stroke in strokes {
        match (stroke.tool(), stroke.mode()) {
            (MaskTool::Brush, MaskMode::Add) => {
                for point in stroke.points() {
                    mask.filled(*point, stroke.radius());
                }
            }
            (MaskTool::Brush, MaskMode::Remove) => {
                for point in stroke.points() {
                    mask.cleared(*point, stroke.radius());
                }
            }
            (MaskTool::Wand, mode) => {
                let region = flood(reader, request, stroke, &mask, frame)?;
                match mode {
                    MaskMode::Add => mask.united(&region),
                    MaskMode::Remove => mask.subtracting(&region),
                }
            }
        }
    }
    if mask.count() == 0 {
        return Ok(None);
    }
    rebuild(reader, level, &mask, frame, request).map(Some)
}

fn single_shape(set: &SurfaceSet, object: usize) -> Result<Shape, String> {
    let packed = set.geometry().map_err(|error| error.to_string())?;
    let keep = (0..packed.object_count())
        .map(|index| index == object)
        .collect::<Vec<_>>();
    Ok(Shape::mesh(Arc::new(packed.retain(&keep))))
}

fn level_of(reader: &Channels, level: usize) -> Result<Level, String> {
    reader
        .pyramid()
        .levels()
        .get(level)
        .copied()
        .ok_or_else(|| format!("the dataset holds no resolution level {level}"))
}

struct Block {
    origin: [u32; 3],
    extent: [u32; 3],
}

fn block(level: Level, base: &Mask, strokes: &[&MaskEdit]) -> Block {
    let limits = level.extent();
    let limits = [limits.x(), limits.y(), limits.z()];
    let voxel = level.voxel();
    let brush = strokes
        .iter()
        .filter(|stroke| stroke.tool() == MaskTool::Brush)
        .map(|stroke| stroke.radius())
        .fold(0.0, f32::max);
    let origin = base.origin();
    let extent = base.extent();
    let mut low = [0u32; 3];
    let mut high = [0u32; 3];
    for axis in 0..3 {
        let reach = if brush > 0.0 {
            (brush / voxel[axis]).ceil() as u32 + 2
        } else {
            WAND_MARGIN
        };
        low[axis] = origin[axis].saturating_sub(reach);
        high[axis] = (origin[axis] + extent[axis] + reach).min(limits[axis]);
    }
    Block {
        origin: low,
        extent: [0, 1, 2].map(|axis| high[axis] - low[axis]),
    }
}

fn flood(
    reader: &Channels,
    request: &MaskRequest<'_>,
    stroke: &MaskEdit,
    block: &Mask,
    frame: u32,
) -> Result<Mask, String> {
    let level = level_of(reader, request.level)?;
    let limits = level.extent();
    let limits = [limits.x(), limits.y(), limits.z()];
    let span = channel_span(reader, request.channel)?;
    let mut origin = block.origin();
    let mut extent = block.extent();
    for _ in 0..=WAND_GROWTH {
        let ranges = [
            origin[0]..origin[0] + extent[0],
            origin[1]..origin[1] + extent[1],
            origin[2]..origin[2] + extent[2],
        ];
        let data = reader
            .read_region(
                frame,
                request.level,
                request.channel,
                &ranges,
                request.drift.warp(frame),
            )
            .map_err(|error| error.to_string())?;
        let region = grow(
            level,
            origin,
            extent,
            &values_of(&data),
            stroke.points()[0],
            stroke.tolerance(),
            span,
        );
        if !touches(&region, origin, extent) {
            return Ok(region);
        }
        let mut grown = [0u32; 3];
        for axis in 0..3 {
            let reach = (extent[axis] / 2).max(1);
            origin[axis] = origin[axis].saturating_sub(reach);
            grown[axis] = (extent[axis] + reach).min(limits[axis] - origin[axis]);
        }
        extent = grown;
    }
    Err("the magic wand reaches the volume before it closes".to_string())
}

fn values_of(data: &VoxelData) -> Vec<f32> {
    let mut values = vec![0.0f32; data.voxel_count()];
    data.write_values(&mut values);
    values
}

fn channel_span(reader: &Channels, channel: usize) -> Result<f32, String> {
    let intensity = reader
        .pyramid()
        .channels()
        .get(channel)
        .ok_or_else(|| format!("the dataset holds no channel {channel}"))?
        .intensity();
    Ok((intensity.max() - intensity.min()).max(1.0e-6))
}

fn grow(
    level: Level,
    origin: [u32; 3],
    extent: [u32; 3],
    values: &[f32],
    seed: [f32; 3],
    tolerance: f32,
    span: f32,
) -> Mask {
    let width = extent[0] as usize;
    let height = extent[1] as usize;
    let continuous = level.voxel_of(seed);
    let start = [0, 1, 2].map(|axis| {
        (continuous[axis].floor() as i64 - i64::from(origin[axis]))
            .clamp(0, i64::from(extent[axis]) - 1) as usize
    });
    let mut mask = Mask::block(level, origin, extent);
    let stamp = [0, 1, 2].map(|axis| start[axis] + origin[axis] as usize);
    let seed_value = values[start[0] + width * (start[1] + height * start[2])];
    let reach = tolerance * span;
    if !(seed_value.is_finite() && reach.is_finite()) {
        return mask;
    }
    let mut visited = vec![false; values.len()];
    let mut frontier = vec![stamp];
    while let Some(cell) = frontier.pop() {
        let local = [0, 1, 2].map(|axis| cell[axis] - origin[axis] as usize);
        let index = local[0] + width * (local[1] + height * local[2]);
        if visited[index] {
            continue;
        }
        visited[index] = true;
        let delta = (values[index] - seed_value).abs();
        if !delta.is_finite() || delta > reach {
            continue;
        }
        mask.set(cell.map(|value| value as u32));
        for axis in 0..3 {
            for step in [-1i64, 1] {
                let next = cell[axis] as i64 + step;
                if next < 0 || next >= i64::from(origin[axis]) + i64::from(extent[axis]) {
                    continue;
                }
                let mut neighbour = cell;
                neighbour[axis] = next as usize;
                frontier.push(neighbour);
            }
        }
    }
    mask
}

fn touches(region: &Mask, origin: [u32; 3], extent: [u32; 3]) -> bool {
    region.cells().any(|cell| {
        (0..3).any(|axis| {
            let value = cell[axis] - origin[axis];
            value == 0 || value + 1 == extent[axis]
        })
    })
}

fn rebuild(
    reader: &Channels,
    level: Level,
    mask: &Mask,
    frame: u32,
    request: &MaskRequest<'_>,
) -> Result<SurfaceSet, String> {
    let origin = mask.origin();
    let extent = mask.extent();
    let voxels = extent
        .iter()
        .map(|value| u64::from(*value))
        .product::<u64>();
    if voxels > BLOCK_LIMIT {
        return Err(format!(
            "an edited mask covers {voxels} voxels and cannot rebuild its surface"
        ));
    }
    let channels = reader.pyramid().channel_count();
    let mut builder = LabelBuilder::new(request.level, level, channels);
    let mut start = 0;
    while start < extent[2] {
        let core = SLAB.min(extent[2] - start);
        let span = [extent[0], extent[1], core];
        let ranges = [
            origin[0]..origin[0] + extent[0],
            origin[1]..origin[1] + extent[1],
            origin[2] + start..origin[2] + start + core,
        ];
        let labels = labels(mask, origin, span, start);
        let mut data = Vec::with_capacity(channels);
        for channel in 0..channels {
            data.push(
                reader
                    .read_region(
                        frame,
                        request.level,
                        channel,
                        &ranges,
                        request.drift.warp(frame),
                    )
                    .map_err(|error| error.to_string())?,
            );
        }
        builder.push(LabelSlab {
            origin: [origin[0], origin[1], origin[2] + start],
            core: span,
            extent: span,
            labels: &labels,
            channels: &data,
        });
        start += core;
    }
    let built = builder.finish();
    built.ok_or_else(|| "the edited mask closes no surface".to_string())
}

fn labels(mask: &Mask, origin: [u32; 3], extent: [u32; 3], start: u32) -> Vec<u32> {
    let width = extent[0] as usize;
    let height = extent[1] as usize;
    let mut labels = vec![0u32; width * height * extent[2] as usize];
    for cell in mask.cells() {
        let z = cell[2] - origin[2] - start;
        if z >= extent[2] {
            continue;
        }
        let x = (cell[0] - origin[0]) as usize;
        let y = (cell[1] - origin[1]) as usize;
        labels[x + width * (y + height * z as usize)] = 1;
    }
    labels
}
