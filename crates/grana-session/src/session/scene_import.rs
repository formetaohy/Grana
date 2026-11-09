use glam::Vec3;
use grana_geometry::{Bounds, Mesh, MeshVertex};
use grana_object::{
    IntensityTable, SampleStatistics, Spot, SpotSet, SurfaceSet, SurfaceStatistics,
};
use grana_readout::{
    Imported, ImportedFrame, ImportedKind, ImportedObject, ImportedSample, ImportedSurface,
};
use grana_scene::ObjectKind;

use super::*;

impl Session {
    pub(super) fn import_scene(&mut self, imported: Imported, file: &str) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        let channels = scene.dataset.channels().len();
        let voxel = scene.dataset.voxel_size().min_element();
        let frames = scene.dataset.frame_count();
        let store = Arc::clone(&self.store);
        let mut failed = Vec::new();
        for object in imported.objects() {
            let built = match object.kind() {
                ImportedKind::Surfaces => surfaces(object, channels, voxel, frames, &store),
                ImportedKind::Spots => spots(object, channels, frames),
            };
            let loaded = match built {
                Ok(loaded) if !loaded.is_empty() => loaded,
                Ok(_) => continue,
                Err(detail) => {
                    failed.push(format!("{}: {detail}", object.name()));
                    continue;
                }
            };
            let kind = match object.kind() {
                ImportedKind::Surfaces => ObjectKind::Surfaces,
                ImportedKind::Spots => ObjectKind::Spots,
            };
            let set = scene
                .objects
                .attached(kind, object.name(), file, &scene.dataset);
            for (time, frame) in loaded {
                scene.objects.install(set, time, frame);
            }
        }
        if !failed.is_empty() {
            self.message = Some(Message::info(format!(
                "the Imaris objects could not be read: {}",
                failed.join(", ")
            )));
        }
        self.touch();
    }
}

type Loaded = Vec<(u32, ObjectFrame)>;

fn surfaces(
    object: &ImportedObject,
    channels: usize,
    voxel: f32,
    frames: u32,
    store: &Arc<GeometryStore>,
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
            let samples = surface
                .intensities()
                .filter(|samples| samples.len() == channels)
                .ok_or_else(|| "the Imaris statistics do not measure every channel".to_string())?;
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
            for sample in samples {
                intensities.push(statistics_sample(*sample));
            }
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

fn spots(object: &ImportedObject, channels: usize, frames: u32) -> Result<Loaded, String> {
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
            let samples = spot
                .intensities()
                .filter(|samples| samples.len() == channels)
                .ok_or_else(|| "the Imaris statistics do not measure every channel".to_string())?;
            held.push(Spot::new(spot.center(), spot.radius(), 0.0));
            for sample in samples {
                intensities.push(statistics_sample(*sample));
            }
        }
        if held.is_empty() {
            continue;
        }
        let table = IntensityTable::new(channels, intensities);
        loaded.push((*time, ObjectFrame::Spots(SpotSet::new(0, held, table))));
    }
    Ok(loaded)
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
