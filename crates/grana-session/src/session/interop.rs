use std::collections::HashMap;
use std::path::{Path, PathBuf};

use grana_geometry::PackedMesh;
use grana_interop::geff;
use grana_interop::{Detection, Edge, Lineage, Mesh, Objects, Related, Shape};
use grana_scene::{ObjectFrame, ObjectKind, ObjectSet, Origin, Scene};

use super::*;

const DEFAULT_QUALITY: f32 = 0.0;

impl Session {
    pub fn import_tracks(&mut self, path: &Path) -> bool {
        if self.loading.is_some() {
            return false;
        }
        let Some(scene) = self.scene.as_mut() else {
            return false;
        };
        let file = path.display().to_string();
        let kind = match geff::kind(path) {
            Ok(kind) => match kind {
                geff::Kind::Points => ObjectKind::Spots,
                geff::Kind::Surfaces => ObjectKind::Surfaces,
                geff::Kind::Filaments => ObjectKind::Filaments,
            },
            Err(error) => {
                self.message = Some(Message::error(error.to_string()));
                return false;
            }
        };
        let set = scene.import_graph(&file, kind);
        self.start_import(set);
        true
    }

    pub fn import_localizations(&mut self, path: &Path) -> bool {
        if self.loading.is_some() {
            return false;
        }
        if !path.is_file() || !grana_readout::localization::has_extension(path) {
            self.message = Some(Message::error(format!(
                "{} is not a localization table",
                path.display()
            )));
            return false;
        }
        let Some(scene) = self.scene.as_mut() else {
            return false;
        };
        let set = scene.import_localizations(&path.display().to_string());
        self.start_import(set);
        true
    }

    pub fn export_geff(&mut self, set: ObjectSetId, path: &Path) -> Result<(), String> {
        let image = self
            .source
            .as_ref()
            .and_then(|loader| loader.source().path().map(PathBuf::from));
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| "no dataset is open".to_string())?;
        let object = scene
            .objects
            .get(set)
            .ok_or_else(|| "the object set left the scene".to_string())?;
        let lineage = lineage(scene, object)?;
        let objects = objects(scene, object, path, image.as_deref())?;
        let spacing = scene.dataset.spacing();
        let axes = geff::Axes::new(
            [spacing.x(), spacing.y(), spacing.z()],
            scene.dataset.frame_interval(),
        );
        geff::write_objects(path, &lineage, &axes, &objects).map_err(|error| error.to_string())
    }

    pub fn export_localizations(&mut self, set: ObjectSetId, path: &Path) -> Result<(), String> {
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| "no dataset is open".to_string())?;
        let object = scene
            .objects
            .get(set)
            .ok_or_else(|| "the object set left the scene".to_string())?;
        if object.kind() != ObjectKind::Spots {
            return Err("only a spot object set exports localizations".to_string());
        }
        let extent = scene.dataset.extent();
        let level = scene.dataset.pyramid().level(0);
        let source = object.localization();
        let metadata = grana_readout::localization::Metadata::new(
            scene.frame_count(),
            source.map_or(extent.x(), grana_scene::LocalizationSource::width),
            source.map_or(extent.y(), grana_scene::LocalizationSource::height),
            source.map_or(
                scene.dataset.voxel_size().x * 1000.0,
                grana_scene::LocalizationSource::pixel,
            ),
            source.map_or(extent.z() > 1, grana_scene::LocalizationSource::volume),
            level.origin(),
        );
        let mut entries = Vec::new();
        for time in 0..scene.frame_count() {
            let Some(frame) = object.frame(time) else {
                continue;
            };
            for (index, spot) in frame.spots_of().iter().enumerate() {
                if !object.visible_object(time, index) {
                    continue;
                }
                entries.push(grana_readout::localization::Localization::new(
                    time,
                    spot.center(),
                    spot.sigma(),
                    spot.quality(),
                    spot.group(),
                ));
            }
        }
        let file = grana_file::Atomic::file(path).map_err(|error| error.to_string())?;
        grana_readout::localization::write(file.staging(), &metadata, &entries)
            .map_err(|error| error.to_string())?;
        file.commit().map_err(|error| error.to_string())
    }

    pub(super) fn choose_import_tracks(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open a GEFF tracking graph")
            .pick_folder()
        else {
            return;
        };
        self.import_tracks(&path);
    }

    pub(super) fn choose_import_localizations(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open a localization table")
            .add_filter(
                "Localization table",
                &["csv", "tsv", "txt", "dat", "hdf5", "h5"],
            )
            .pick_file()
        else {
            return;
        };
        self.import_localizations(&path);
    }

    pub(super) fn choose_export_geff(&mut self, set: ObjectSetId) {
        let Some(path) = self.track_path(set, "tracks.geff", "Export a GEFF tracking graph") else {
            return;
        };
        match self.export_geff(set, &path) {
            Ok(()) => self.message = None,
            Err(detail) => {
                self.message = Some(Message::error(format!(
                    "failed to export the graph to {}: {detail}",
                    path.display()
                )));
            }
        }
    }

    pub(super) fn choose_export_localizations(&mut self, set: ObjectSetId) {
        let Some(path) = self.track_path(
            set,
            "localizations.hdf5",
            "Export a Picasso localization table",
        ) else {
            return;
        };
        match self.export_localizations(set, &path) {
            Ok(()) => {
                self.message = Some(Message::info(format!(
                    "exported the localizations to {}",
                    path.display()
                )));
            }
            Err(detail) => {
                self.message = Some(Message::error(format!(
                    "failed to export the localizations to {}: {detail}",
                    path.display()
                )));
            }
        }
    }

    fn track_path(&self, set: ObjectSetId, suffix: &str, title: &str) -> Option<PathBuf> {
        let scene = self.scene.as_ref()?;
        let object = scene.objects.get(set)?;
        let suggested = format!(
            "{}-{}-{suffix}",
            crate::slug(scene.dataset.name()),
            object.key().as_str()
        );
        rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(suggested)
            .save_file()
    }
}

fn lineage(scene: &Scene, set: &ObjectSet) -> Result<Lineage, String> {
    let voxel = scene.dataset.voxel_size().min_element();
    let floor = (0.5 * voxel).max(1.0e-6);
    let mut detections = Vec::new();
    let mut ids = HashMap::new();
    let mut next = 0u64;
    for time in 0..scene.frame_count() {
        let Some(frame) = set.frame(time) else {
            continue;
        };
        let spots = frame.spots_of();
        let anchors = frame.anchors();
        for (object, anchor) in anchors.into_iter().enumerate() {
            if !set.visible_object(time, object) {
                continue;
            }
            let spot = spots.get(object).copied();
            let radius = spot.map_or_else(
                || {
                    let size = anchor.bounds().size();
                    (0.5 * size[0].max(size[1]).max(size[2])).max(floor)
                },
                |spot| spot.radius().max(floor),
            );
            let quality = spot.map_or(DEFAULT_QUALITY, |spot| spot.quality());
            detections.push(Detection::new(
                next,
                time,
                anchor.position(),
                radius,
                quality,
            ));
            ids.insert((time, object), next);
            next += 1;
        }
    }
    if detections.is_empty() {
        return Err(format!("{} holds no visible object to export", set.name()));
    }
    let mut edges = Vec::new();
    if let Some(tracks) = set.tracks() {
        let tracks = tracks.set();
        for (index, track) in tracks.tracks().iter().enumerate() {
            for pair in track.nodes().windows(2) {
                let source = ids.get(&(pair[0].time(), pair[0].object()));
                let target = ids.get(&(pair[1].time(), pair[1].object()));
                if let (Some(source), Some(target)) = (source, target) {
                    edges.push(Edge::new(*source, *target));
                }
            }
            for link in tracks.parents(index) {
                let Some(parent) = tracks.track(link.parent() as usize) else {
                    continue;
                };
                let ends = (
                    parent
                        .nodes()
                        .last()
                        .and_then(|node| ids.get(&(node.time(), node.object()))),
                    track
                        .nodes()
                        .first()
                        .and_then(|node| ids.get(&(node.time(), node.object()))),
                );
                if let (Some(source), Some(target)) = ends {
                    edges.push(Edge::new(*source, *target));
                }
            }
        }
    }
    Lineage::new(detections, edges).map_err(|error| error.to_string())
}

fn objects(
    scene: &Scene,
    set: &ObjectSet,
    path: &Path,
    image: Option<&Path>,
) -> Result<Objects, String> {
    let kind = match set.kind() {
        ObjectKind::Spots => geff::Kind::Points,
        ObjectKind::Surfaces | ObjectKind::Cells => geff::Kind::Surfaces,
        ObjectKind::Filaments => geff::Kind::Filaments,
    };
    let channels = scene.dataset.channels().len();
    let mut shapes = Vec::new();
    let mut labels = Vec::new();
    let mut intensities = Vec::new();
    let mut measured = true;
    for time in 0..scene.frame_count() {
        let Some(frame) = set.frame(time) else {
            continue;
        };
        let geometry = frame.geometry().map_err(|error| error.to_string())?;
        let table = frame.intensities();
        let complete = table.channels() == channels && table.objects() == frame.object_count();
        measured &= complete;
        for object in 0..frame.object_count() {
            if !set.visible_object(time, object) {
                continue;
            }
            shapes.push(shape(frame, geometry.as_deref(), object)?);
            labels.push(object as u64 + 1);
            if complete {
                intensities.push(
                    (0..channels)
                        .map(|channel| table.object(object, channel))
                        .collect::<Vec<_>>(),
                );
            }
        }
    }
    let intensities = measured.then_some(intensities);
    let related = related(scene, set, path, image);
    Ok(Objects::new(kind, shapes, labels, intensities, related))
}

fn shape(
    frame: &ObjectFrame,
    geometry: Option<&PackedMesh>,
    object: usize,
) -> Result<Shape, String> {
    match frame {
        ObjectFrame::Spots(_) => Ok(Shape::Point),
        ObjectFrame::Surfaces(set) => {
            let statistics = set
                .objects()
                .get(object)
                .ok_or_else(|| format!("the surface {object} left its frame"))?;
            Ok(Shape::Surface {
                mesh: mesh(geometry, object)?,
                voxels: statistics.voxel_count(),
                volume: statistics.volume(),
                area: statistics.area(),
            })
        }
        ObjectFrame::Cells(set) => {
            let statistics = set
                .bodies()
                .objects()
                .get(object)
                .ok_or_else(|| format!("the cell {object} left its frame"))?;
            Ok(Shape::Surface {
                mesh: mesh(geometry, object)?,
                voxels: statistics.voxel_count(),
                volume: statistics.volume(),
                area: statistics.area(),
            })
        }
        ObjectFrame::Filaments(set) => {
            let statistics = set
                .objects()
                .get(object)
                .ok_or_else(|| format!("the filament {object} left its frame"))?;
            Ok(Shape::Filament {
                mesh: mesh(geometry, object)?,
                length: statistics.length(),
                diameter: statistics.diameter(),
                volume: statistics.volume(),
                branches: statistics.branches(),
                terminals: statistics.terminals(),
                junctions: statistics.junctions(),
                nodes: statistics.nodes(),
            })
        }
    }
}

fn mesh(geometry: Option<&PackedMesh>, object: usize) -> Result<Mesh, String> {
    let packed = geometry.ok_or_else(|| "the object frame carries no mesh".to_string())?;
    let group = packed
        .groups()
        .get(object)
        .ok_or_else(|| format!("the object {object} carries no mesh"))?;
    let vertices = group.vertex_count() as usize;
    let mut positions = Vec::with_capacity(vertices);
    let mut normals = Vec::with_capacity(vertices);
    for vertex in 0..vertices {
        positions.push(packed.position(object, vertex));
        normals.push(packed.normal(object, vertex));
    }
    let mut indices = Vec::with_capacity(group.index_count() as usize);
    for triangle in 0..group.index_count() as usize / 3 {
        indices.extend_from_slice(&packed.triangle_vertices(object, triangle));
    }
    Ok(Mesh::new(positions, normals, indices))
}

fn related(scene: &Scene, set: &ObjectSet, path: &Path, image: Option<&Path>) -> Vec<Related> {
    let Some(base) = path.parent() else {
        return Vec::new();
    };
    let mut related = Vec::new();
    if let Some(export) = set.label_export()
        && let Some(path) = directory_relative(base, Path::new(export.path()))
    {
        related.push(Related::new("labels", &path, Some("seg_id")));
    }
    if scene.dataset.origin() == Origin::Zarr
        && let Some(image) = image
        && let Some(path) = directory_relative(base, image)
    {
        related.push(Related::new("image", &path, None));
    }
    related
}

fn directory_relative(base: &Path, target: &Path) -> Option<String> {
    let mut relative = relative(base, target)?;
    if !relative.ends_with('/') {
        relative.push('/');
    }
    Some(relative)
}

fn relative(base: &Path, target: &Path) -> Option<String> {
    let base = std::path::absolute(base).ok()?;
    let target = std::path::absolute(target).ok()?;
    let base = base.components().collect::<Vec<_>>();
    let target = target.components().collect::<Vec<_>>();
    let common = base
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return None;
    }
    let mut parts = Vec::new();
    for _ in common..base.len() {
        parts.push("..".to_string());
    }
    for component in &target[common..] {
        parts.push(component.as_os_str().to_str()?.to_string());
    }
    if parts.is_empty() {
        return Some(".".to_string());
    }
    Some(parts.join("/"))
}
