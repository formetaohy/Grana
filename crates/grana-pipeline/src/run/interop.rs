use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_geometry::{Bounds, GeometryStore, Mesh, MeshVertex};
use grana_interop::{Kind, Objects, Shape};
use grana_object::{
    FilamentSet, FilamentStatistics, FilamentTopology, IntensityTable, SampleStatistics, Spot,
    SpotSet, SurfaceSet, SurfaceStatistics,
};
use grana_scene::{Drift, ObjectFrame};
use grana_volume::{Level, VoxelData};

use crate::channels::Channels;
use crate::event::Event;

pub struct InteropRequest {
    file: String,
    drift: Drift,
    store: Arc<GeometryStore>,
}

impl InteropRequest {
    pub fn new(file: &str, drift: Drift, store: Arc<GeometryStore>) -> Self {
        assert!(!file.trim().is_empty(), "an import names a track file");
        Self {
            file: file.to_string(),
            drift,
            store,
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
    let (lineage, objects) = grana_interop::geff::read_objects(Path::new(&request.file), radius)
        .map_err(|error| error.to_string())?;
    let tracks = lineage
        .tracks(reader.pyramid().frame_interval())
        .map_err(|error| error.to_string())?;
    let frames = group(&lineage);
    let total = frames.len();
    sink(Event::Progress {
        time: 0,
        done: 0,
        total,
    });
    let samples = samples(&lineage, &objects, reader.pyramid().channel_count());
    for (index, (time, slots)) in frames.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let detections = lineage.detections();
        let frame = match objects.kind() {
            Kind::Points => {
                let spots = slots
                    .iter()
                    .map(|slot| {
                        let detection = &detections[*slot];
                        Spot::new(
                            detection.position(),
                            detection.radius(),
                            detection.quality(),
                        )
                    })
                    .collect::<Vec<_>>();
                let table = intensities(
                    reader, &request, *time, slots, detections, &objects, &samples,
                )?;
                ObjectFrame::Spots(SpotSet::new(0, spots, table))
            }
            Kind::Surfaces => {
                let (mesh, statistics) = surfaces(slots, detections, &objects)?;
                let table = intensities(
                    reader, &request, *time, slots, detections, &objects, &samples,
                )?;
                ObjectFrame::Surfaces(
                    SurfaceSet::paged(0, &request.store, mesh, statistics, table)
                        .map_err(|error| error.to_string())?,
                )
            }
            Kind::Filaments => {
                let (mesh, statistics) = filaments(slots, detections, &objects)?;
                let table = intensities(
                    reader, &request, *time, slots, detections, &objects, &samples,
                )?;
                ObjectFrame::Filaments(
                    FilamentSet::paged(0, &request.store, mesh, statistics, table)
                        .map_err(|error| error.to_string())?,
                )
            }
        };
        sink(Event::Frame { time: *time, frame });
        sink(Event::Progress {
            time: *time,
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

fn group(lineage: &grana_interop::Lineage) -> Vec<(u32, Vec<usize>)> {
    let mut frames: Vec<(u32, Vec<usize>)> = Vec::new();
    for (slot, detection) in lineage.detections().iter().enumerate() {
        match frames.last_mut() {
            Some((time, slots)) if *time == detection.time() => slots.push(slot),
            _ => frames.push((detection.time(), vec![slot])),
        }
    }
    frames
}

fn samples(
    lineage: &grana_interop::Lineage,
    objects: &Objects,
    channels: usize,
) -> Option<HashMap<u64, Vec<SampleStatistics>>> {
    let values = objects.intensities()?;
    if values.len() != lineage.len() || objects.channels() == 0 || objects.channels() != channels {
        return None;
    }
    Some(
        lineage
            .detections()
            .iter()
            .zip(values)
            .map(|(detection, samples)| (detection.id(), samples.clone()))
            .collect(),
    )
}

fn surfaces(
    slots: &[usize],
    detections: &[grana_interop::Detection],
    objects: &Objects,
) -> Result<(Mesh, Vec<SurfaceStatistics>), String> {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut statistics = Vec::with_capacity(slots.len());
    for (object, slot) in slots.iter().enumerate() {
        let shape = &objects.shapes()[*slot];
        let Shape::Surface {
            mesh,
            voxels,
            volume,
            area,
        } = shape
        else {
            return Err("a surface graph holds a shape that is not a surface".to_string());
        };
        let base = vertices.len() as u32;
        for (position, normal) in mesh.positions().iter().zip(mesh.normals()) {
            vertices.push(MeshVertex::new(*position, *normal, object as u32));
        }
        for index in mesh.indices() {
            indices.push(base + index);
        }
        let bounds = bounds(mesh.positions());
        statistics.push(SurfaceStatistics::new(
            *voxels,
            *volume,
            *area,
            detections[*slot].position(),
            bounds,
            None,
        ));
    }
    Ok((Mesh::new(vertices, indices), statistics))
}

fn filaments(
    slots: &[usize],
    detections: &[grana_interop::Detection],
    objects: &Objects,
) -> Result<(Mesh, Vec<FilamentStatistics>), String> {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut statistics = Vec::with_capacity(slots.len());
    for (object, slot) in slots.iter().enumerate() {
        let shape = &objects.shapes()[*slot];
        let Shape::Filament {
            mesh,
            length,
            diameter,
            volume,
            branches,
            terminals,
            junctions,
            nodes,
        } = shape
        else {
            return Err("a filament graph holds a shape that is not a filament".to_string());
        };
        let base = vertices.len() as u32;
        for (position, normal) in mesh.positions().iter().zip(mesh.normals()) {
            vertices.push(MeshVertex::new(*position, *normal, object as u32));
        }
        for index in mesh.indices() {
            indices.push(base + index);
        }
        statistics.push(FilamentStatistics::new(
            *length,
            *diameter,
            *volume,
            FilamentTopology::new(*branches, *terminals, *junctions, *nodes),
            detections[*slot].position(),
            bounds(mesh.positions()),
        ));
    }
    Ok((Mesh::new(vertices, indices), statistics))
}

fn bounds(positions: &[[f32; 3]]) -> Bounds {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for position in positions {
        for (axis, value) in position.iter().enumerate() {
            low[axis] = low[axis].min(*value);
            high[axis] = high[axis].max(*value);
        }
    }
    Bounds::new(low, high)
}

fn intensities(
    reader: &Channels,
    request: &InteropRequest,
    time: u32,
    slots: &[usize],
    detections: &[grana_interop::Detection],
    objects: &Objects,
    samples: &Option<HashMap<u64, Vec<SampleStatistics>>>,
) -> Result<IntensityTable, String> {
    if let Some(samples) = samples {
        let channels = objects.channels();
        let mut values = Vec::with_capacity(slots.len() * channels);
        for slot in slots {
            let detection = &detections[*slot];
            let measured = samples.get(&detection.id()).ok_or_else(|| {
                format!(
                    "the graph carries no intensities for node {}",
                    detection.id()
                )
            })?;
            values.extend_from_slice(measured);
        }
        return Ok(IntensityTable::new(channels, values));
    }
    let spots = slots
        .iter()
        .map(|slot| {
            let detection = &detections[*slot];
            Spot::new(
                detection.position(),
                detection.radius(),
                detection.quality(),
            )
        })
        .collect::<Vec<_>>();
    measure(
        reader,
        reader.pyramid().level(0),
        time,
        &spots,
        request.drift.warp(time),
    )
}

fn measure(
    reader: &Channels,
    info: Level,
    time: u32,
    spots: &[Spot],
    warp: &grana_warp::Warp,
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
                .read_region(time, 0, channel, &ranges, warp)
                .map_err(|error| error.to_string())?;
            let mut samples = vec![0.0f32; data.voxel_count()];
            data.write_values(&mut samples);
            let size = [
                (ranges[0].end - ranges[0].start) as usize,
                (ranges[1].end - ranges[1].start) as usize,
                (ranges[2].end - ranges[2].start) as usize,
            ];
            let squared_radius = radius * radius;
            let mut accumulator = grana_object::SampleAccumulator::new();
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
