use std::collections::HashMap;
use std::path::Path;

use grana_interop::geff;
use grana_interop::trackmate::{self, Header};
use grana_interop::{Detection, Edge, Lineage};
use grana_scene::{ObjectSet, Scene};

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
        let set = scene.import_tracks(&file);
        self.start_import(set);
        true
    }

    pub fn export_tracks(&mut self, set: ObjectSetId, path: &Path) -> Result<(), String> {
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| "no dataset is open".to_string())?;
        let object = scene
            .objects
            .get(set)
            .ok_or_else(|| "the object set left the scene".to_string())?;
        let lineage = lineage(scene, object)?;
        let extent = scene.dataset.extent();
        let spacing = scene.dataset.spacing();
        let header = Header::new(
            scene.dataset.name(),
            String::new(),
            [extent.x(), extent.y(), extent.z()],
            scene.frame_count(),
            [spacing.x(), spacing.y(), spacing.z()],
            scene.dataset.frame_interval(),
        );
        trackmate::write(path, &lineage, &header).map_err(|error| error.to_string())
    }

    pub fn export_geff(&mut self, set: ObjectSetId, path: &Path) -> Result<(), String> {
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| "no dataset is open".to_string())?;
        let object = scene
            .objects
            .get(set)
            .ok_or_else(|| "the object set left the scene".to_string())?;
        let lineage = lineage(scene, object)?;
        let spacing = scene.dataset.spacing();
        let axes = geff::Axes::new(
            [spacing.x(), spacing.y(), spacing.z()],
            scene.dataset.frame_interval(),
        );
        geff::write(path, &lineage, &axes).map_err(|error| error.to_string())
    }

    pub(super) fn choose_import_tracks(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open a TrackMate or MaMuT track file")
            .add_filter("TrackMate XML", &["xml"])
            .pick_file()
        else {
            return;
        };
        self.import_tracks(&path);
    }

    pub(super) fn choose_export_tracks(&mut self, set: ObjectSetId) {
        let Some(path) = self.track_path(set, "tracks.xml", "Export TrackMate tracks") else {
            return;
        };
        match self.export_tracks(set, &path) {
            Ok(()) => self.message = None,
            Err(detail) => {
                self.message = Some(Message::error(format!(
                    "failed to export tracks to {}: {detail}",
                    path.display()
                )));
            }
        }
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
