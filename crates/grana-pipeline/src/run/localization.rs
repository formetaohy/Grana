use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_object::{IntensityTable, SampleAccumulator, SampleStatistics, Spot, SpotSet};
use grana_readout::localization::{self, Localization};
use grana_scene::{LocalizationAlignment, ObjectFrame};
use grana_warp::Warp;

use crate::channels::Channels;
use crate::event::Event;

const TILE: u32 = 64;
const SAMPLE_LEVEL: usize = 0;

pub struct LocalizationRequest {
    file: String,
    alignment: LocalizationAlignment,
}

impl LocalizationRequest {
    pub fn new(file: &str, alignment: LocalizationAlignment) -> Self {
        assert!(
            !file.trim().is_empty(),
            "an import names a localization table"
        );
        Self {
            file: file.to_string(),
            alignment,
        }
    }

    pub fn file(&self) -> &str {
        &self.file
    }
}

pub fn localization(
    reader: &Channels,
    request: LocalizationRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let pyramid = reader.pyramid();
    let level = pyramid.level(SAMPLE_LEVEL);
    let frames = pyramid.frame_count();
    let path = Path::new(&request.file);
    let mut carry_on = |_done: u64, _total: u64| !cancel.load(Ordering::Relaxed);
    let Some(mut table) = localization::open(path, level, frames, &mut carry_on)
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    sink(Event::Localization {
        pixel: table.pixel(),
        width: table.width(),
        height: table.height(),
        volume: table.volume(),
        photons: table.photons(),
        alignment: request.alignment.clone(),
    });
    let total = table.rows();
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let Some(frame) = table
            .next(&mut carry_on)
            .map_err(|error| error.to_string())?
        else {
            break;
        };
        let time = frame.time();
        let rows = frame.rows();
        done += rows.len() as u64;
        if let Some(set) = spots(reader, time, rows, &request.alignment.warp(time))? {
            sink(Event::Frame {
                time,
                frame: ObjectFrame::Spots(set),
            });
        }
        sink(Event::Progress {
            time,
            done: done.min(usize::MAX as u64) as usize,
            total: total.min(usize::MAX as u64) as usize,
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    sink(Event::Done);
    Ok(())
}

fn spots(
    reader: &Channels,
    time: u32,
    entries: &[Localization],
    warp: &Warp,
) -> Result<Option<SpotSet>, String> {
    if entries.is_empty() {
        return Ok(None);
    }
    let spots = entries
        .iter()
        .map(|entry| {
            let center = entry.center();
            let offset = warp.sample(center);
            Spot::grouped(
                [0, 1, 2].map(|axis| center[axis] - offset[axis]),
                entry.sigma(),
                entry.quality(),
                entry.group(),
            )
        })
        .collect::<Vec<_>>();
    let intensities = intensities(reader, time, &spots, warp)?;
    Ok(Some(SpotSet::new(0, spots, intensities)))
}

fn intensities(
    reader: &Channels,
    time: u32,
    spots: &[Spot],
    warp: &Warp,
) -> Result<IntensityTable, String> {
    let channels = reader.pyramid().channel_count();
    let level = reader.pyramid().level(SAMPLE_LEVEL);
    let extent = level.extent();
    let dims = [extent.x(), extent.y(), extent.z()];
    let mut voxels = Vec::with_capacity(spots.len());
    let mut tiles = BTreeMap::<[u32; 3], Vec<usize>>::new();
    for (index, spot) in spots.iter().enumerate() {
        let centre = level.voxel_of(spot.center());
        let voxel = [0, 1, 2].map(|axis| {
            (centre[axis] - 0.5)
                .round()
                .clamp(0.0, (dims[axis] - 1) as f32) as u32
        });
        voxels.push(voxel);
        tiles
            .entry([0, 1, 2].map(|axis| voxel[axis] / TILE))
            .or_default()
            .push(index);
    }
    let blank = SampleStatistics::new(0.0, 0.0, 0.0, 0.0, 0.0);
    let mut values = vec![blank; spots.len() * channels];
    for (tile, members) in tiles {
        let low = [0, 1, 2].map(|axis| tile[axis] * TILE);
        let high = [0, 1, 2].map(|axis| (low[axis] + TILE).min(dims[axis]));
        let ranges = [low[0]..high[0], low[1]..high[1], low[2]..high[2]];
        let size = [0, 1, 2].map(|axis| (high[axis] - low[axis]) as usize);
        for channel in 0..channels {
            let data = reader
                .read_region(time, SAMPLE_LEVEL, channel, &ranges, warp)
                .map_err(|error| error.to_string())?;
            if data.voxel_count() != size.iter().product::<usize>() {
                return Err("the sampled tile disagrees with its extent".to_string());
            }
            let mut samples = vec![0.0f32; data.voxel_count()];
            data.write_values(&mut samples);
            for object in &members {
                let voxel = voxels[*object];
                let local = [0, 1, 2].map(|axis| (voxel[axis] - low[axis]) as usize);
                let index = local[0] + size[0] * (local[1] + size[1] * local[2]);
                let mut accumulator = SampleAccumulator::new();
                accumulator.push(samples[index]);
                values[*object * channels + channel] = accumulator.statistics();
            }
        }
    }
    Ok(IntensityTable::new(channels, values))
}
