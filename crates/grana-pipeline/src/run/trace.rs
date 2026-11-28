use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::{TraceBand, Tracer};
use grana_geometry::PackedMesh;
use grana_scene::Drifts;
use grana_volume::{ChannelInfo, Level, VoxelData};
use grana_warp::Warp;

use crate::channels::Channels;

pub enum SurfaceEvent {
    Progress {
        done: usize,
        total: usize,
    },
    Surface {
        mesh: Arc<PackedMesh>,
        colors: Arc<Vec<[f32; 3]>>,
    },
}

const BAND: u32 = 48;
const BAND_BYTES: u64 = 256 << 20;

pub struct TraceRequest {
    channel: usize,
    time: u32,
    level: Level,
    drifts: Drifts,
}

impl TraceRequest {
    pub fn new(channel: usize, time: u32, level: Level, drifts: Drifts) -> Self {
        Self {
            channel,
            time,
            level,
            drifts,
        }
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn time(&self) -> u32 {
        self.time
    }
}

pub fn trace(
    reader: &Channels,
    request: TraceRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(SurfaceEvent),
) -> Result<(), String> {
    let pyramid = reader.pyramid();
    let level = request.level;
    let info = pyramid
        .channels()
        .get(request.channel)
        .ok_or_else(|| format!("the dataset holds no channel {}", request.channel + 1))?;
    let extent = level.extent();
    let dims = [extent.x(), extent.y(), extent.z()];
    if dims.contains(&0) {
        return Err("the dataset holds no voxel to trace a surface from".to_string());
    }
    let plane = u64::from(dims[0]) * u64::from(dims[1]) * info.format().bytes() as u64;
    let band = ((BAND_BYTES / plane.max(1)).max(1) as u32).min(BAND);
    let bands = dims[2].div_ceil(band) as usize;
    sink(SurfaceEvent::Progress {
        done: 0,
        total: bands,
    });
    let warp: &Warp = request.drifts.warp(request.channel, request.time);
    let background = info.statistics().background();
    let read = |z: u32| -> Result<VoxelData, String> {
        let ranges: [Range<u32>; 3] = [
            0..dims[0],
            0..dims[1],
            z.saturating_sub(1)..(z + band + 1).min(dims[2]),
        ];
        reader
            .read_region(request.time, 0, request.channel, &ranges, warp)
            .map_err(|error| error.to_string())
    };
    let mut tracer = Tracer::new();
    let mut done = 0usize;
    let mut z = 0u32;
    let mut data = read(z)?;
    while z < dims[2] {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let top = (z + band).min(dims[2]);
        let low = z.saturating_sub(1);
        let span = [dims[0], dims[1], (top + 1).min(dims[2]) - low];
        let first = z - low;
        let last = (top - low).min(span[2] - 1);
        let cells = [0..span[0] - 1, 0..span[1] - 1, first..last];
        let ((), fetched) = rayon::join(
            || {
                tracer.push(TraceBand::new(
                    level,
                    [0, 0, low],
                    span,
                    cells,
                    &data,
                    background,
                ))
            },
            || match top < dims[2] {
                true => read(top).map(Some),
                false => Ok(None),
            },
        );
        let fetched = fetched?;
        done += 1;
        sink(SurfaceEvent::Progress { done, total: bands });
        match fetched {
            Some(next) => {
                z = top;
                data = next;
            }
            None => break,
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    let Some(traced) = tracer.finish() else {
        return Ok(());
    };
    let colors = Arc::new(colors(&traced.labels, info));
    let mesh = Arc::new(PackedMesh::new(traced.mesh));
    sink(SurfaceEvent::Surface { mesh, colors });
    Ok(())
}

fn colors(labels: &[u32], info: &ChannelInfo) -> Vec<[f32; 3]> {
    let table = info.color_table().map(|table| table.colors());
    labels
        .iter()
        .map(|label| match table {
            Some(colors) => *colors
                .get(*label as usize)
                .unwrap_or_else(|| colors.last().expect("a colour table holds colours")),
            None => info.tint(),
        })
        .collect()
}
