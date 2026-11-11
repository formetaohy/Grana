use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_object::{IntensityTable, SampleAccumulator, SampleStatistics, Spot, SpotSet};
use grana_scene::{Drift, ObjectFrame};
use grana_volume::{Level, VoxelData};

use crate::channels::Channels;
use crate::event::Event;

pub struct InteropRequest {
    file: String,
    drift: Drift,
}

impl InteropRequest {
    pub fn new(file: &str, drift: Drift) -> Self {
        assert!(!file.trim().is_empty(), "an import names a track file");
        Self {
            file: file.to_string(),
            drift,
        }
    }

    pub fn file(&self) -> &str {
        &self.file
    }
}

pub fn interop(
    reader: &Channels,
    request: InteropRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let spacing = reader.pyramid().level(0).spacing();
    let radius = spacing.x().min(spacing.y()).min(spacing.z());
    let lineage = grana_interop::geff::read(Path::new(&request.file), radius)
        .map_err(|error| error.to_string())?;
    let tracks = lineage
        .tracks(reader.pyramid().frame_interval())
        .map_err(|error| error.to_string())?;
    let frames = lineage.spots();
    let total = frames.len();
    sink(Event::Progress {
        time: 0,
        done: 0,
        total,
    });
    let info = reader.pyramid().level(0);
    for (index, (time, spots)) in frames.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let intensities = measure(reader, info, time, &spots, request.drift.offset(time))?;
        sink(Event::Frame {
            time,
            frame: ObjectFrame::Spots(SpotSet::new(0, spots, intensities)),
        });
        sink(Event::Progress {
            time,
            done: index + 1,
            total,
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    sink(Event::Tracks(tracks));
    sink(Event::Done);
    Ok(())
}

fn measure(
    reader: &Channels,
    info: Level,
    time: u32,
    spots: &[Spot],
    shift: [f32; 3],
) -> Result<IntensityTable, String> {
    let channels = reader.pyramid().channel_count();
    let extent = info.extent();
    let dims = [extent.x(), extent.y(), extent.z()];
    let span = info.spacing();
    let step = [span.x(), span.y(), span.z()];
    let mut values = vec![SampleStatistics::new(0.0, 0.0, 0.0, 0.0, 0.0); spots.len() * channels];
    for channel in 0..channels {
        for (slot, spot) in spots.iter().enumerate() {
            let center = spot.center();
            let radius = spot.radius();
            let mut voxel = [0.0f32; 3];
            let mut low = [0u32; 3];
            let mut high = [0u32; 3];
            for axis in 0..3 {
                voxel[axis] = info.voxel_of(center)[axis] - 0.5;
                let reach = radius / step[axis];
                low[axis] = (voxel[axis] - reach)
                    .floor()
                    .clamp(0.0, (dims[axis] - 1) as f32) as u32;
                high[axis] = (voxel[axis] + reach)
                    .ceil()
                    .clamp(0.0, (dims[axis] - 1) as f32) as u32;
            }
            let ranges = [
                low[0]..high[0] + 1,
                low[1]..high[1] + 1,
                low[2]..high[2] + 1,
            ];
            let data: VoxelData = reader
                .read_region(time, 0, channel, &ranges, shift)
                .map_err(|error| error.to_string())?;
            let mut samples = vec![0.0f32; data.voxel_count()];
            data.write_values(&mut samples);
            let size = [
                (ranges[0].end - ranges[0].start) as usize,
                (ranges[1].end - ranges[1].start) as usize,
                (ranges[2].end - ranges[2].start) as usize,
            ];
            let squared_radius = radius * radius;
            let mut accumulator = SampleAccumulator::new();
            let mut sampled = false;
            for z in 0..size[2] {
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        let squared = (0..3)
                            .map(|axis| {
                                let position = info.voxel_center([
                                    (low[0] + x as u32) as f32,
                                    (low[1] + y as u32) as f32,
                                    (low[2] + z as u32) as f32,
                                ])[axis];
                                (position - center[axis]).powi(2)
                            })
                            .sum::<f32>();
                        if squared <= squared_radius {
                            accumulator.push(samples[x + size[0] * (y + size[1] * z)]);
                            sampled = true;
                        }
                    }
                }
            }
            if !sampled {
                let nearest = [
                    (voxel[0].round() as u32).clamp(low[0], high[0]) - low[0],
                    (voxel[1].round() as u32).clamp(low[1], high[1]) - low[1],
                    (voxel[2].round() as u32).clamp(low[2], high[2]) - low[2],
                ];
                let index = nearest[0] as usize
                    + size[0] * (nearest[1] as usize + size[1] * nearest[2] as usize);
                accumulator.push(samples[index]);
            }
            values[slot * channels + channel] = accumulator.statistics();
        }
    }
    Ok(IntensityTable::new(channels, values))
}
