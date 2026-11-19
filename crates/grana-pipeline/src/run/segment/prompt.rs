use std::sync::atomic::{AtomicBool, Ordering};

use grana_scene::ObjectFrame;
use grana_volume::Level;

use crate::event::Event;

use super::Slicer;

pub(super) fn segment(
    slicer: &Slicer<'_>,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let seeds = &slicer.request.seeds;
    let frames = slicer.reader.pyramid().frame_count();
    if seeds.len() != frames as usize {
        return Err("the seed points cover every frame of the dataset".to_string());
    }
    let depth = slicer.depth() as usize;
    let mut found = 0usize;
    for span in spans(seeds, frames) {
        let mut prompts = seeds[span.start as usize].clone();
        for time in order(span) {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            if prompts.is_empty() {
                break;
            }
            let Some(mut assembly) = slicer.volume(time, cancel, &mut |done| {
                sink(Event::Progress {
                    time,
                    done,
                    total: depth,
                });
            })?
            else {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(());
                }
                break;
            };
            let origin = [
                assembly.ranges[0].start,
                assembly.ranges[1].start,
                assembly.ranges[2].start,
            ];
            let voxels = voxels(&prompts, slicer.info, origin);
            let components = assembly.volume.retain_containing(&voxels);
            if components.is_empty() {
                break;
            }
            found += components.len();
            if let Some(set) = slicer.surfaces(time, &assembly)? {
                sink(Event::Frame {
                    time,
                    frame: ObjectFrame::Surfaces(set),
                });
            }
            prompts = components
                .iter()
                .map(|component| {
                    let center = component.center();
                    slicer.info.voxel_center([
                        center[0] + origin[0] as f32,
                        center[1] + origin[1] as f32,
                        center[2] + origin[2] as f32,
                    ])
                })
                .collect();
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    if found == 0 {
        return Err("the seed points mark no object".to_string());
    }
    sink(Event::Done);
    Ok(())
}

fn voxels(prompts: &[[f32; 3]], info: Level, origin: [u32; 3]) -> Vec<[u32; 3]> {
    prompts
        .iter()
        .filter_map(|prompt| {
            let voxel = info.voxel_of(*prompt);
            if voxel.iter().any(|value| !value.is_finite()) {
                return None;
            }
            let mut local = [0u32; 3];
            for (axis, value) in voxel.iter().enumerate() {
                let index = (value.floor() as i64).checked_sub(i64::from(origin[axis]))?;
                local[axis] = u32::try_from(index).ok()?;
            }
            Some(local)
        })
        .collect()
}

struct Span {
    start: u32,
    end: u32,
    forward: bool,
}

fn spans(seeds: &[Vec<[f32; 3]>], frames: u32) -> Vec<Span> {
    let seeded = (0..frames)
        .filter(|time| !seeds[*time as usize].is_empty())
        .collect::<Vec<_>>();
    let mut spans = Vec::new();
    if let Some(first) = seeded.first().copied()
        && first > 0
    {
        spans.push(Span {
            start: first,
            end: 0,
            forward: false,
        });
    }
    for (index, &start) in seeded.iter().enumerate() {
        spans.push(Span {
            start,
            end: seeded.get(index + 1).copied().unwrap_or(frames),
            forward: true,
        });
    }
    spans
}

fn order(span: Span) -> Vec<u32> {
    let mut order = vec![span.start];
    if span.forward {
        order.extend(span.start + 1..span.end);
    } else {
        let mut time = span.start;
        while time > 0 {
            time -= 1;
            order.push(time);
        }
    }
    order
}
