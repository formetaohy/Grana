use std::sync::Arc;

use grana_display::{Camera, Shading, srgb_to_linear};

use grana_geometry::TriangleBudget;
use grana_render::{
    ChannelRow, Digest, Layer, LayerKey, SceneFrame, Splat, SplatChunk, SplatLevel, Splats, Style,
    Versions,
};
use grana_scene::{Item, ObjectFrame, ObjectSet, Scene, Selection};

use crate::geometry::GeometryCache;
use crate::splat::{SplatCache, SplatMembers};
use crate::spot::{self, Choice};
use crate::style::StyleCache;
use crate::surface::ChannelSurface;

const CHANNEL_OWNER: u64 = 1 << 63;

const SEED_COLOR: [f32; 3] = [1.00, 0.97, 0.90];
const SEED_SCALE: f32 = 2.0;
const SEED_PART: u64 = 0;
const DETECTION_PART: u64 = 1;
const HIGHLIGHT_PART: u64 = 2;
pub(crate) const OBJECT_PART: u32 = 0;
pub(crate) const TRACK_PART: u32 = 1;

#[derive(Clone, Copy)]
pub struct Lens<'a> {
    pub camera: &'a Camera,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Targets {
    detail: Vec<Option<TriangleBudget>>,
    splats: Vec<Choice>,
}

pub struct Cache {
    splats: SplatCache,
    geometry: GeometryCache,
    styles: StyleCache,
}

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

impl Cache {
    pub fn new() -> Self {
        Self {
            splats: SplatCache::new(),
            geometry: GeometryCache::new(),
            styles: StyleCache::new(),
        }
    }

    pub fn clear(&mut self) {
        self.splats.clear();
        self.geometry.clear();
        self.styles.clear();
    }
}

#[derive(Clone)]
pub struct PickRows {
    base: u32,
    ids: Arc<[Selection]>,
}

impl PickRows {
    pub fn base(&self) -> u32 {
        self.base
    }

    pub fn resolve(&self, index: u32) -> Option<Selection> {
        let offset = index.checked_sub(self.base)?;
        self.ids.get(offset as usize).copied()
    }
}

pub struct RenderedScene {
    pub versions: Versions,
    pub channels: Vec<ChannelRow>,
    pub layers: Vec<Layer>,
    pub splats: Vec<SplatChunk>,
    pub members: Vec<SplatMembers>,
    shading: Shading,
    step_scale: f32,
    picks: Vec<PickRows>,
}

impl RenderedScene {
    pub fn frame<'a>(&'a self, overlays: &'a [grana_render::Overlay<'a>]) -> SceneFrame<'a> {
        SceneFrame {
            versions: self.versions,
            channels: &self.channels,
            shading: self.shading,
            step_scale: self.step_scale,
            layers: &self.layers,
            splats: Splats::new(&self.splats),
            overlays,
        }
    }

    pub fn splat_count(&self) -> usize {
        self.splats
            .iter()
            .filter(|chunk| chunk.active())
            .map(SplatChunk::len)
            .sum()
    }

    pub fn splat_members(&self, key: u64, level: u8) -> Option<&[u32]> {
        self.members
            .iter()
            .find(|members| members.key() == key)?
            .members(level)
    }

    pub fn style(&self, index: u32) -> Option<Selection> {
        self.picks.iter().find_map(|rows| rows.resolve(index))
    }

    pub fn picks(&self) -> &[PickRows] {
        &self.picks
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    topology: u64,
    classification: u64,
    appearance: u64,
    display: u64,
}

impl Key {
    pub fn topology(self) -> u64 {
        self.topology
    }

    pub fn classification(self) -> u64 {
        self.classification
    }

    pub fn appearance(self) -> u64 {
        self.appearance
    }

    pub fn display(self) -> u64 {
        self.display
    }

    pub fn digest(self) -> u64 {
        let mut digest = Digest::new();
        digest.mix(self.topology);
        digest.mix(self.classification);
        digest.mix(self.appearance);
        digest.mix(self.display);
        digest.finish()
    }
}

fn item_digest(item: Item) -> u64 {
    match item {
        Item::Object(id) => u64::from(id.raw()) * 2,
        Item::Track(id) => u64::from(id.raw()) * 2 + 1,
    }
}

pub fn key(scene: &Scene, surfaces: &[ChannelSurface<'_>]) -> Key {
    let mut topology = Digest::new();
    topology.mix(u64::from(scene.time()));
    topology.mix(scene.channels.len() as u64);
    topology.mix(scene.derived().len() as u64);
    for surface in surfaces {
        topology.mix(surface.channel() as u64);
        topology.mix(surface.revision());
    }
    let mut classification = Digest::new();
    let mut appearance = Digest::new();
    for set in scene.objects.iter() {
        topology.mix(set.id().raw());
        topology.mix(set.revision().raw());
        topology.mix(
            set.frame_stamp(scene.time())
                .map_or(u64::MAX, |stamp| stamp.raw()),
        );
        if let Some(source) = set.localization()
            && source.projected()
        {
            for time in source.projected_frames(scene.time(), scene.frame_count()) {
                topology.mix(set.frame_stamp(time).map_or(u64::MAX, |stamp| stamp.raw()));
            }
        }
        topology.mix(
            set.tracks()
                .map_or(u64::MAX, |tracks| tracks.revision().raw()),
        );
        classification.mix(set.shape().raw());
        classification.flag(scene.shown(set));
        classification.mix(u64::from(set.opacity().to_bits()));
        classification.flag(set.track_visible());
        classification.mix(
            set.frame_appearance(scene.time())
                .map_or(u64::MAX, |stamp| stamp.raw()),
        );
        appearance.mix(set.document().raw());
        appearance.mix(scene.table_revision(set.id()).unwrap_or(u64::MAX));
    }
    for selection in scene.selections() {
        appearance.mix(selection.set.raw());
        appearance.mix(item_digest(selection.item));
    }
    let mut display = Digest::new();
    for channel in &scene.channels {
        display.flag(channel.enabled);
        display.real(channel.opacity);
        display.real(channel.window.min());
        display.real(channel.window.max());
        display.mix(channel.volume().mode().index() as u64);
        display.real(channel.volume().isovalue());
        display.flag(channel.transfer().stops().is_some());
        for stop in channel.transfer().stops().unwrap_or_default() {
            display.real(stop.position());
            display.real(stop.opacity());
            for component in stop.color() {
                display.real(component);
            }
        }
        for color in channel.transfer().colors() {
            for component in color {
                display.real(*component);
            }
        }
    }
    display.real(scene.shading.ambient);
    display.real(scene.shading.diffuse);
    display.real(scene.shading.shadows);
    display.real(scene.shading.azimuth);
    display.real(scene.shading.elevation);
    display.real(scene.step_scale);
    Key {
        topology: topology.finish(),
        classification: classification.finish(),
        appearance: appearance.finish(),
        display: display.finish(),
    }
}

pub fn plan(scene: &Scene, lens: Option<Lens<'_>>) -> Targets {
    plan_with(scene, lens, &mut Cache::new())
}

pub fn plan_with(scene: &Scene, lens: Option<Lens<'_>>, cache: &mut Cache) -> Targets {
    let mut detail = Vec::new();
    let mut choices = Vec::new();
    for set in scene.objects.iter() {
        if !scene.shown(set) {
            continue;
        }
        let frame = set.frame_shared(scene.time());
        detail.push(match (frame, lens) {
            (Some(frame), Some(lens)) => match frame.as_ref() {
                ObjectFrame::Surfaces(surfaces) => {
                    Some(cache.geometry.target(scene, set, surfaces, lens))
                }
                ObjectFrame::Cells(cells) => {
                    Some(cache.geometry.target(scene, set, cells.bodies(), lens))
                }
                ObjectFrame::Spots(_) | ObjectFrame::Filaments(_) => None,
            },
            _ => None,
        });
        choices.push(match lens {
            Some(lens) => {
                let pixels = lens
                    .camera
                    .world_per_point(lens.height as f32, lens.camera.distance());
                cache.splats.choose(scene, set, pixels)
            }
            None => Choice::Full,
        });
    }
    Targets {
        detail,
        splats: choices,
    }
}

pub fn frame(
    scene: &Scene,
    targets: &Targets,
    cache: &mut Cache,
    budget: u64,
    surfaces: &[ChannelSurface<'_>],
) -> Result<RenderedScene, String> {
    assert_eq!(
        targets.detail.len(),
        targets.splats.len(),
        "a target names one detail level and one splat level per shown set"
    );
    let mut available = budget;
    let mut layers = Vec::new();
    let mut picks = Vec::new();
    let mut splats = Vec::new();
    let mut members = Vec::new();
    let mut topology = Digest::new();
    let mut classification = Digest::new();
    let mut appearance = Digest::new();
    let mut splat_versions = Digest::new();
    let mut ordinal = 0usize;
    let mut base = 0u32;
    let time = scene.time();
    let seed_radius = SEED_SCALE * scene.dataset.voxel_size().min_element();
    topology.mix(scene.channels.len() as u64);
    topology.mix(scene.dataset.channels().len() as u64);
    for set in scene.objects.iter() {
        let shown = scene.shown(set);
        topology.mix(set.id().raw());
        classification.mix(set.shape().raw());
        classification.flag(shown);
        classification.mix(u64::from(set.opacity().to_bits()));
        classification.flag(set.track_visible());
        classification.mix(
            set.frame_appearance(time)
                .map_or(u64::MAX, |stamp| stamp.raw()),
        );
        appearance.mix(set.document().raw());
        appearance.mix(scene.table_revision(set.id()).unwrap_or(u64::MAX));
        if !shown {
            continue;
        }
        let Some(detail) = targets.detail.get(ordinal).copied() else {
            return Err("the targets leave a shown object set without a detail level".to_string());
        };
        let choice = targets.splats[ordinal];
        ordinal += 1;
        let mut seeded = Vec::new();
        let seed_sigma = seed_radius / grana_object::RADIUS_PER_SIGMA;
        let seed_color = srgb_to_linear(SEED_COLOR);
        for seed in scene.seeds(set.id()) {
            seeded.push(Splat::gaussian(*seed, [seed_sigma; 3], seed_color, 1.0));
        }
        if !seeded.is_empty() {
            let key = splat_key(set.id().raw(), SEED_PART);
            let stamp = seed_stamp(scene, set);
            let count = seeded.len();
            splat_versions.mix(key);
            splat_versions.mix(stamp);
            let chunk = SplatChunk::single(key, stamp, seeded);
            spend(&mut available, chunk.bytes(), "seed");
            splats.push(chunk);
            members.push(SplatMembers::standalone(key, set.id(), count));
        }
        let Some(frame) = set.frame_shared(time) else {
            continue;
        };
        let colors = scene.object_colors(set);
        let color = |index: usize| {
            colors
                .as_ref()
                .and_then(|colors| colors.get(index).copied())
                .unwrap_or_else(|| set.color())
        };
        if frame.spots().is_some() {
            let prepared = cache.splats.prepare(
                scene,
                set,
                splat_key(set.id().raw(), DETECTION_PART),
                choice,
                available,
            );
            spend(&mut available, prepared.used, "detection");
            for chunk in &prepared.chunks {
                splat_versions.mix(chunk.key());
                splat_versions.mix(u64::from(chunk.level()));
                splat_versions.flag(chunk.active());
                splat_versions.mix(chunk.data().geometry_stamp());
                splat_versions.mix(chunk.data().style_stamp());
            }
            splats.extend(prepared.chunks);
            members.push(prepared.members);
            let spots = frame.spots().expect("the frame carries spots");
            let selected = scene
                .selections()
                .iter()
                .filter(|held| held.set == set.id())
                .filter_map(|held| match held.item {
                    Item::Object(id) => Some(id),
                    Item::Track(_) => None,
                })
                .collect::<Vec<_>>();
            let visible = |index: usize| set.visible_object(time, index);
            let highlight = spot::highlights(spots, &selected, set.opacity(), visible);
            if !highlight.geometry.is_empty() {
                let key = splat_key(set.id().raw(), HIGHLIGHT_PART);
                let stamp = highlight_stamp(scene, set);
                splat_versions.mix(key);
                splat_versions.mix(stamp);
                let chunk = SplatChunk::new(
                    key,
                    0,
                    true,
                    SplatLevel::new(
                        stamp,
                        stamp,
                        Arc::new(highlight.geometry),
                        Arc::new(highlight.style),
                    ),
                );
                spend(&mut available, chunk.bytes(), "highlight");
                splats.push(chunk);
                members.push(SplatMembers::new(key, set.id(), highlight.members));
            }
        }
        let (geometry, level) = match frame.as_ref() {
            ObjectFrame::Surfaces(surfaces) => {
                let budget = detail.expect("a surface frame carries a triangle budget");
                let (mesh, available) =
                    surfaces.detail(budget).map_err(|error| error.to_string())?;
                (Some(mesh), available)
            }
            ObjectFrame::Cells(cells) => {
                let budget = detail.expect("a cell frame carries a triangle budget");
                let (mesh, available) = cells.detail(budget).map_err(|error| error.to_string())?;
                (Some(mesh), available)
            }
            ObjectFrame::Spots(_) | ObjectFrame::Filaments(_) => {
                (frame.geometry().map_err(|error| error.to_string())?, 0)
            }
        };
        if let (Some(mesh), Some(stamp), Some(geometry_id), Some(look_id)) = (
            geometry,
            set.frame_stamp(time),
            set.frame_geometry(time),
            set.frame_appearance(time),
        ) && !mesh.is_empty()
        {
            let count = frame.object_count();
            let rows = cache
                .styles
                .rows(scene, set, OBJECT_PART, count, |styles, ids| {
                    for index in 0..count {
                        let visible = set.visible_object(time, index);
                        let selected = scene.object_selected(set.id(), index);
                        styles.push(
                            Style::new(color(index), visible, selected, set.opacity())
                                .with_material(set.material()),
                        );
                        if let Some(id) = frame.id_of(index) {
                            ids.push(Selection {
                                set: set.id(),
                                item: Item::Object(id),
                            });
                        }
                    }
                });
            appearance.mix(rows.digest());
            classification.mix(rows.classification());
            let layer = Layer::new(
                LayerKey::new(set.id().raw(), OBJECT_PART, level as u16),
                geometry_id.raw(),
                look_id.raw(),
                mesh,
                rows.styles(),
                base,
            );
            topology.mix(layer.key.digest());
            topology.mix(layer.geometry);
            topology.mix(stamp.raw());
            topology.mix(count as u64);
            topology.mix(u64::from(base));
            appearance.mix(layer.look);
            base += count as u32;
            picks.push(PickRows {
                base: layer.pick_base,
                ids: rows.ids(),
            });
            layers.push(layer);
        }
        if let Some(tracks) = set.tracks()
            && !tracks.set().is_empty()
        {
            let visible = set.track_visible();
            let count = tracks.set().len();
            let rows = cache
                .styles
                .rows(scene, set, TRACK_PART, count, |styles, ids| {
                    for track in tracks.set().tracks() {
                        let shown = visible && set.visible_track(track.id());
                        styles.push(
                            Style::new(
                                set.track_color(track.id()),
                                shown,
                                scene.track_selected(set.id(), track.id()),
                                set.opacity(),
                            )
                            .with_material(set.material()),
                        );
                        ids.push(Selection {
                            set: set.id(),
                            item: Item::Track(track.id()),
                        });
                    }
                });
            appearance.mix(rows.digest());
            classification.mix(rows.classification());
            let layer = Layer::new(
                LayerKey::new(set.id().raw(), TRACK_PART, 0),
                tracks.revision().raw(),
                tracks.revision().raw(),
                tracks.mesh(),
                rows.styles(),
                base,
            );
            topology.mix(layer.key.digest());
            topology.mix(layer.geometry);
            topology.mix(count as u64);
            topology.mix(u64::from(base));
            base += count as u32;
            picks.push(PickRows {
                base: layer.pick_base,
                ids: rows.ids(),
            });
            layers.push(layer);
        }
    }
    for selection in scene.selections() {
        appearance.mix(selection.set.raw());
        appearance.mix(item_digest(selection.item));
    }
    let channels = channel_rows(scene);
    channel_layers(
        scene,
        surfaces,
        &mut base,
        &mut topology,
        &mut appearance,
        &mut layers,
        &mut picks,
    );
    assert_eq!(
        ordinal,
        targets.detail.len(),
        "the targets cover every shown set"
    );
    assert_eq!(layers.len(), picks.len(), "every layer names its pick rows");
    let present = scene.objects.iter().map(|set| set.id()).collect::<Vec<_>>();
    cache.splats.retain(&present);
    cache.geometry.retain(&present);
    cache.styles.retain(&present);
    Ok(RenderedScene {
        versions: Versions::new(
            topology.finish(),
            classification.finish(),
            appearance.finish(),
            splat_versions.finish(),
        ),
        channels,
        layers,
        splats,
        members,
        shading: scene.shading,
        step_scale: scene.step_scale,
        picks,
    })
}

fn channel_layers(
    scene: &Scene,
    surfaces: &[ChannelSurface<'_>],
    base: &mut u32,
    topology: &mut Digest,
    appearance: &mut Digest,
    layers: &mut Vec<Layer>,
    picks: &mut Vec<PickRows>,
) {
    for surface in surfaces {
        let Some(channel) = scene.channels.get(surface.channel()) else {
            continue;
        };
        if !channel.enabled {
            continue;
        }
        let mut look = Digest::new();
        for color in surface.colors() {
            for component in color {
                look.real(*component);
            }
        }
        look.real(channel.opacity);
        let look = look.finish();
        let styles = surface
            .colors()
            .iter()
            .map(|color| Style::new(*color, true, false, channel.opacity))
            .collect::<Vec<_>>();
        let mesh = Arc::clone(surface.mesh());
        let count = mesh.object_count();
        let key = LayerKey::new(CHANNEL_OWNER | surface.channel() as u64, 0, 0);
        let layer = Layer::new(key, surface.revision(), look, mesh, styles.into(), *base);
        topology.mix(key.digest());
        topology.mix(surface.revision());
        topology.mix(count as u64);
        topology.mix(u64::from(*base));
        appearance.mix(look);
        *base += count as u32;
        picks.push(PickRows {
            base: layer.pick_base,
            ids: Arc::from(Vec::new()),
        });
        layers.push(layer);
    }
}

fn channel_rows(scene: &Scene) -> Vec<ChannelRow> {
    scene
        .channels
        .iter()
        .enumerate()
        .map(|(index, channel)| ChannelRow {
            visible: channel.enabled,
            opacity: channel.opacity,
            intensity: channel.window,
            transfer: channel.transfer().clone(),
            volume: channel.volume(),
            surface: crate::demand::surface_channel(scene, index),
        })
        .collect()
}

fn seed_stamp(scene: &Scene, set: &ObjectSet) -> u64 {
    let mut stamp = Digest::new();
    stamp.mix(set.id().raw());
    stamp.mix(u64::from(scene.time()));
    for seed in scene.seeds(set.id()) {
        for axis in seed {
            stamp.real(*axis);
        }
    }
    stamp.finish()
}

fn highlight_stamp(scene: &Scene, set: &ObjectSet) -> u64 {
    let mut stamp = Digest::new();
    stamp.mix(set.id().raw());
    stamp.mix(
        set.frame_stamp(scene.time())
            .map_or(u64::MAX, |held| held.raw()),
    );
    stamp.mix(u64::from(scene.time()));
    stamp.mix(set.document().raw());
    stamp.mix(u64::from(set.opacity().to_bits()));
    stamp.mix(crate::splat::selection_digest(scene, set));
    stamp.finish()
}

fn splat_key(set: u64, part: u64) -> u64 {
    let mut key = Digest::new();
    key.mix(set);
    key.mix(part);
    key.finish()
}

fn spend(available: &mut u64, bytes: u64, part: &str) {
    *available = available.checked_sub(bytes).unwrap_or_else(|| {
        panic!("the {part} splats hold {bytes} bytes but the splat residency keeps {available}")
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use grana_geometry::{Mesh, MeshVertex, PackedMesh};
    use grana_scene::Dataset;
    use grana_volume::{
        Calibration, ChannelInfo, Extent, Intensity, Level, Pyramid, Spacing, Statistics,
        VoxelFormat, VoxelSemantics,
    };

    fn scene() -> Scene {
        let level = Level::new(Extent::new(8, 8, 4), Spacing::new(1.0, 1.0, 1.0));
        let statistics = Statistics::declared(Intensity::new(0.0, 255.0), VoxelSemantics::Scalar);
        let channel = ChannelInfo::new("signal", [0.2, 0.6, 0.2], statistics, VoxelFormat::U8);
        let pyramid = Pyramid::new(vec![level], 1, vec![channel], 1, 1.0, Calibration::Recorded);
        Scene::new(Dataset::synthetic("keys", pyramid))
    }

    fn mesh() -> Arc<PackedMesh> {
        let vertices = vec![
            MeshVertex::new([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0),
            MeshVertex::new([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0),
            MeshVertex::new([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 0),
        ];
        Arc::new(PackedMesh::new(Mesh::new(vertices, vec![0, 1, 2])))
    }

    #[test]
    fn a_channel_surface_changes_the_prepared_keys() {
        let scene = scene();
        let colors = Arc::new(vec![[0.2, 0.6, 0.2]]);
        let mesh = mesh();
        let surface = ChannelSurface::new(0, &mesh, &colors, 11);
        assert_ne!(
            key(&scene, &[]).topology(),
            key(&scene, &[surface]).topology(),
            "a channel surface that arrives after the frame settled must re-prepare it"
        );
    }
}
