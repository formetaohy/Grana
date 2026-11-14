use glam::Vec3;
use grana_geometry::{Bounds, Mesh, MeshVertex, ends, push_graph};
use grana_object::{
    FilamentSet, FilamentStatistics, FilamentTopology, IntensityTable, SampleStatistics, Spot,
    SpotSet, SurfaceSet, SurfaceStatistics,
};
use grana_readout::{
    Imported, ImportedFilament, ImportedFrame, ImportedKind, ImportedObject, ImportedSample,
    ImportedSpot, ImportedSurface,
};
use grana_scene::{IntensitySource, ObjectKind};
use grana_store::SpillStore;

use super::intensity::Sampler;
use super::*;

#[derive(Default)]
struct Report {
    unmeasured: usize,
}

struct Sampling<'a, 'b> {
    sampler: &'a mut Option<Sampler<'b>>,
    report: &'a mut Report,
    sampled: &'a mut usize,
}

impl Sampling<'_, '_> {
    fn surface(
        &mut self,
        time: u32,
        surface: &ImportedSurface,
    ) -> Result<Vec<SampleStatistics>, String> {
        let Some(sampler) = self.sampler.as_mut() else {
            return Err("the Imaris statistics do not measure every channel".to_string());
        };
        let measured = sampler.surface(time, surface)?;
        *self.sampled += 1;
        if !measured.measured {
            self.report.unmeasured += 1;
        }
        Ok(measured.intensities)
    }

    fn spot(&mut self, time: u32, spot: &ImportedSpot) -> Result<Vec<SampleStatistics>, String> {
        let Some(sampler) = self.sampler.as_mut() else {
            return Err("the Imaris statistics do not measure every channel".to_string());
        };
        let measured = sampler.spot(time, spot)?;
        *self.sampled += 1;
        if !measured.measured {
            self.report.unmeasured += 1;
        }
        Ok(measured.intensities)
    }

    fn filament(
        &mut self,
        time: u32,
        filament: &ImportedFilament,
    ) -> Result<Vec<SampleStatistics>, String> {
        let Some(sampler) = self.sampler.as_mut() else {
            return Err("the Imaris statistics do not measure every channel".to_string());
        };
        let measured = sampler.filament(time, filament)?;
        *self.sampled += 1;
        if !measured.measured {
            self.report.unmeasured += 1;
        }
        Ok(measured.intensities)
    }
}

impl Session {
    pub(super) fn import_scene(&mut self, imported: Imported, file: &str) {
        let channels = self.channels.clone();
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        let count = scene.dataset.channels().len();
        let voxel = scene.dataset.voxel_size().min_element();
        let frames = scene.dataset.frame_count();
        let store = Arc::clone(&self.store);
        let mut sampler = channels.as_deref().map(Sampler::new);
        let mut report = Report::default();
        let mut sampled = 0usize;
        let mut failed = Vec::new();
        {
            let mut sampling = Sampling {
                sampler: &mut sampler,
                report: &mut report,
                sampled: &mut sampled,
            };
            for object in imported.objects() {
                let mut sampled_here = 0usize;
                let built = match object.kind() {
                    ImportedKind::Surfaces => surfaces(
                        object,
                        count,
                        voxel,
                        frames,
                        &store,
                        &mut sampling,
                        &mut sampled_here,
                    ),
                    ImportedKind::Spots => {
                        spots(object, count, frames, &mut sampling, &mut sampled_here)
                    }
                    ImportedKind::Filaments => filaments(
                        object,
                        count,
                        frames,
                        &store,
                        &mut sampling,
                        &mut sampled_here,
                    ),
                };
                let loaded = match built {
                    Ok(loaded) if !loaded.is_empty() => loaded,
                    Ok(_) => {
                        failed.push(ImportNote::new(
                            object.name(),
                            "the object holds no frame with geometry",
                        ));
                        continue;
                    }
                    Err(detail) => {
                        failed.push(ImportNote::new(object.name(), &detail));
                        continue;
                    }
                };
                let kind = match object.kind() {
                    ImportedKind::Surfaces => ObjectKind::Surfaces,
                    ImportedKind::Spots => ObjectKind::Spots,
                    ImportedKind::Filaments => ObjectKind::Filaments,
                };
                let intensities = if sampled_here > 0 {
                    IntensitySource::Sampled
                } else {
                    IntensitySource::Statistics
                };
                let set =
                    scene
                        .objects
                        .attached(kind, object.name(), file, intensities, &scene.dataset);
                let mut nodes = Vec::new();
                for (time, frame) in &loaded {
                    nodes.extend(
                        frame
                            .centroids()
                            .into_iter()
                            .enumerate()
                            .map(|(index, centroid)| (*time, index, centroid)),
                    );
                }
                for (time, frame) in loaded {
                    scene.objects.install(set, time, frame);
                }
                if !object.links().is_empty() {
                    match grana_object::TrackSet::linked(
                        scene.dataset.frame_interval(),
                        &nodes,
                        object.links(),
                    ) {
                        Ok(Some(tracks)) => {
                            scene.objects.install_tracks(set, tracks);
                        }
                        Ok(None) => {}
                        Err(detail) => failed.push(ImportNote::new(object.name(), &detail)),
                    }
                }
            }
        }
        let mut notes = Vec::new();
        if !failed.is_empty() {
            notes.push(format!(
                "the Imaris objects could not be read: {}",
                failed
                    .iter()
                    .map(|note| format!("{} ({})", note.name(), note.detail()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if sampled > 0 {
            let mut text = format!(
                "{sampled} Imaris objects held no statistics and were measured from the voxels inside them"
            );
            if report.unmeasured > 0 {
                text.push_str(&format!(
                    " · {} held no voxel inside and were sampled at their centre",
                    report.unmeasured
                ));
            }
            notes.push(text);
        }
        if !notes.is_empty() {
            self.message = Some(Message::info(notes.join(" · ")));
        }
        self.import = imported
            .diagnostics()
            .iter()
            .chain(failed.iter())
            .cloned()
            .collect();
        self.touch();
    }
}

type Loaded = Vec<(u32, ObjectFrame)>;

fn surfaces(
    object: &ImportedObject,
    channels: usize,
    voxel: f32,
    frames: u32,
    store: &Arc<SpillStore>,
    sampling: &mut Sampling<'_, '_>,
    sampled: &mut usize,
) -> Result<Loaded, String> {
    let mut loaded = Vec::new();
    for (time, frame) in object.frames() {
        if *time >= frames {
            continue;
        }
        let ImportedFrame::Surfaces(surfaces) = frame else {
            return Err("the object holds mixed frames".to_string());
        };
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut objects = Vec::new();
        let mut intensities = Vec::new();
        for (index, surface) in surfaces.iter().enumerate() {
            let statistics = statistics_of(surface, voxel)?;
            let samples = match surface.intensities() {
                Some(samples) if samples.len() == channels => samples
                    .iter()
                    .copied()
                    .map(statistics_sample)
                    .collect::<Vec<_>>(),
                _ => {
                    *sampled += 1;
                    sampling.surface(*time, surface)?
                }
            };
            let base = u32::try_from(vertices.len())
                .map_err(|_| "the surface holds more vertices than a mesh addresses".to_string())?;
            for (position, normal) in surface.vertices().iter().zip(surface.normals()) {
                vertices.push(MeshVertex::new(*position, *normal, index as u32));
            }
            for triangle in surface.triangles() {
                if triangle
                    .iter()
                    .any(|slot| *slot as usize >= surface.vertices().len())
                {
                    return Err(
                        "a surface triangle names a vertex the file does not hold".to_string()
                    );
                }
                indices.extend(triangle.map(|slot| base + slot));
            }
            objects.push(statistics);
            intensities.extend(samples);
        }
        if vertices.is_empty() || indices.is_empty() {
            continue;
        }
        let mesh = Mesh::new(vertices, indices);
        let table = IntensityTable::new(channels, intensities);
        let set =
            SurfaceSet::paged(0, store, mesh, objects, table).map_err(|error| error.to_string())?;
        loaded.push((*time, ObjectFrame::Surfaces(set)));
    }
    Ok(loaded)
}

fn spots(
    object: &ImportedObject,
    channels: usize,
    frames: u32,
    sampling: &mut Sampling<'_, '_>,
    sampled: &mut usize,
) -> Result<Loaded, String> {
    let mut loaded = Vec::new();
    for (time, frame) in object.frames() {
        if *time >= frames {
            continue;
        }
        let ImportedFrame::Spots(spots) = frame else {
            return Err("the object holds mixed frames".to_string());
        };
        let mut held = Vec::with_capacity(spots.len());
        let mut intensities = Vec::new();
        for spot in spots {
            let samples = match spot.intensities() {
                Some(samples) if samples.len() == channels => samples
                    .iter()
                    .copied()
                    .map(statistics_sample)
                    .collect::<Vec<_>>(),
                _ => {
                    *sampled += 1;
                    sampling.spot(*time, spot)?
                }
            };
            held.push(Spot::new(spot.center(), spot.radius(), 0.0));
            intensities.extend(samples);
        }
        if held.is_empty() {
            continue;
        }
        let table = IntensityTable::new(channels, intensities);
        loaded.push((
            *time,
            ObjectFrame::Spots(Arc::new(SpotSet::new(0, held, table))),
        ));
    }
    Ok(loaded)
}

fn filaments(
    object: &ImportedObject,
    channels: usize,
    frames: u32,
    store: &Arc<SpillStore>,
    sampling: &mut Sampling<'_, '_>,
    sampled: &mut usize,
) -> Result<Loaded, String> {
    let mut loaded = Vec::new();
    for (time, frame) in object.frames() {
        if *time >= frames {
            continue;
        }
        let ImportedFrame::Filaments(filaments) = frame else {
            return Err("the object holds mixed frames".to_string());
        };
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut objects = Vec::new();
        let mut intensities = Vec::new();
        for filament in filaments {
            let edges = filament
                .edges()
                .iter()
                .copied()
                .filter(|edge| joins(filament.positions(), *edge))
                .collect::<Vec<_>>();
            if edges.is_empty() {
                continue;
            }
            let index = objects.len() as u32;
            let branches = push_graph(
                &mut vertices,
                &mut indices,
                filament.positions(),
                filament.radii(),
                &edges,
                index,
            );
            objects.push(measures(filament, &edges, branches)?);
            let samples = match filament.intensities() {
                Some(samples) if samples.len() == channels => {
                    samples.iter().copied().map(statistics_sample).collect()
                }
                _ => {
                    *sampled += 1;
                    sampling.filament(*time, filament)?
                }
            };
            intensities.extend(samples);
        }
        if vertices.is_empty() || indices.is_empty() {
            continue;
        }
        let mesh = Mesh::new(vertices, indices);
        let table = IntensityTable::new(channels, intensities);
        let set = FilamentSet::paged(0, store, mesh, objects, table)
            .map_err(|error| error.to_string())?;
        loaded.push((*time, ObjectFrame::Filaments(set)));
    }
    Ok(loaded)
}

fn joins(positions: &[[f32; 3]], edge: [u32; 2]) -> bool {
    let (Some(first), Some(second)) = (
        positions.get(edge[0] as usize),
        positions.get(edge[1] as usize),
    ) else {
        return false;
    };
    edge[0] != edge[1] && first != second
}

fn measures(
    filament: &ImportedFilament,
    edges: &[[u32; 2]],
    branches: usize,
) -> Result<FilamentStatistics, String> {
    let positions = filament.positions();
    let radii = filament.radii();
    let mut length = 0.0f64;
    let mut volume = 0.0f64;
    let mut moment = Vec3::ZERO;
    for edge in edges {
        let (first, second) = (
            Vec3::from(positions[edge[0] as usize]),
            Vec3::from(positions[edge[1] as usize]),
        );
        let distance = f64::from(first.distance(second));
        if !(distance.is_finite() && distance > 0.0) {
            continue;
        }
        let radius = 0.5 * (radii[edge[0] as usize] + radii[edge[1] as usize]);
        length += distance;
        volume += std::f64::consts::PI * f64::from(radius).powi(2) * distance;
        moment += (first + second) * (0.5 * distance as f32);
    }
    if !(length.is_finite() && length > 0.0 && volume.is_finite() && volume > 0.0) {
        return Err("a filament holds no measurable length".to_string());
    }
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for position in positions {
        for (axis, value) in position.iter().enumerate() {
            low[axis] = low[axis].min(*value);
            high[axis] = high[axis].max(*value);
        }
    }
    let diameter = 2.0 * (volume / (std::f64::consts::PI * length)).sqrt();
    let ends = ends(edges, positions.len());
    let nodes = positions.len().min(u32::MAX as usize) as u32;
    let centroid = moment / length as f32;
    Ok(FilamentStatistics::new(
        length,
        diameter,
        volume,
        FilamentTopology::new(branches as u32, ends.terminals, ends.junctions, nodes),
        centroid.to_array(),
        Bounds::new(low, high),
    ))
}

fn statistics_sample(sample: ImportedSample) -> SampleStatistics {
    SampleStatistics::new(
        sample.mean(),
        sample.minimum(),
        sample.maximum(),
        sample.deviation(),
        sample.sum(),
    )
}

fn statistics_of(surface: &ImportedSurface, voxel: f32) -> Result<SurfaceStatistics, String> {
    let vertices = surface.vertices();
    let triangles = surface.triangles();
    if vertices.is_empty() || triangles.is_empty() {
        return Err("the surface is empty".to_string());
    }
    let point = |index: usize| Vec3::from(vertices[index]);
    let mut area = 0.0f64;
    let mut signed = 0.0f64;
    let mut moment = Vec3::ZERO;
    for triangle in triangles {
        let [first, second, third] = [
            point(triangle[0] as usize),
            point(triangle[1] as usize),
            point(triangle[2] as usize),
        ];
        let cross = (second - first).cross(third - first);
        area += 0.5 * f64::from(cross.length());
        let volume = f64::from(first.dot(second.cross(third)));
        signed += volume;
        moment += (first + second + third) * (0.25 * volume as f32);
    }
    let volume = signed.abs() / 6.0;
    if !volume.is_finite()
        || volume <= 1.0e-9
        || !area.is_finite()
        || area <= 1.0e-9
        || signed.abs() < 1.0e-9
    {
        return Err("the surface encloses no volume".to_string());
    }
    let centroid = moment / signed as f32;
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for vertex in vertices {
        for (axis, value) in vertex.iter().enumerate() {
            low[axis] = low[axis].min(*value);
            high[axis] = high[axis].max(*value);
        }
    }
    if !centroid.is_finite() || !low.iter().chain(high.iter()).all(|value| value.is_finite()) {
        return Err("the surface names a degenerate extent".to_string());
    }
    let voxels = (volume / f64::from(voxel.max(1.0e-9)).powi(3))
        .round()
        .max(1.0) as u64;
    Ok(SurfaceStatistics::new(
        voxels,
        volume,
        area,
        centroid.to_array(),
        Bounds::new(low, high),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_a_tetrahedron() {
        let vertices = vec![
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 2.0],
        ];
        let normals = vec![[0.0, 1.0, 0.0]; 4];
        let triangles = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let surface = ImportedSurface::new(vertices, normals, triangles, Vec::new());
        let statistics = statistics_of(&surface, 1.0).expect("the tetrahedron has a volume");
        assert!((statistics.volume() - 4.0 / 3.0).abs() <= 1.0e-4);
        assert!(statistics.area() > 6.0);
        assert_eq!(statistics.centroid(), [0.5, 0.5, 0.5]);
        assert_eq!(statistics.voxel_count(), 1);
    }
}
