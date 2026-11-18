use std::sync::Arc;

use grana_display::{Camera, Shading, srgb_to_linear};
use grana_render::{
    ChannelRow, Digest, Layer, LayerKey, SceneFrame, Splat, SplatChunk, SplatLevel, Splats, Style,
    Versions,
};
use grana_scene::{Item, ObjectFrame, ObjectSet, Scene, Selection};

use crate::geometry::GeometryCache;
use crate::splat::{SplatCache, SplatMembers};
use crate::spot::{self, Choice};
use crate::style::StyleCache;

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
    detail: Vec<usize>,
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
        Item::Object(index) => (index as u64) * 2,
        Item::Track(index) => (index as u64) * 2 + 1,
    }
}

pub fn key(scene: &Scene) -> Key {
    let mut topology = Digest::new();
    topology.mix(u64::from(scene.time()));
    topology.mix(scene.channels.len() as u64);
    topology.mix(scene.derived().len() as u64);
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
        for stop in channel.transfer().stops() {
            display.real(stop.position());
            display.real(stop.opacity());
            for component in stop.color() {
                display.real(component);
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
                    cache.geometry.target(scene, set, surfaces, lens)
                }
                ObjectFrame::Cells(cells) => {
                    cache.geometry.target(scene, set, cells.bodies(), lens)
                }
                ObjectFrame::Spots(_) | ObjectFrame::Filaments(_) => 0,
            },
            _ => 0,
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

pub fn frame(scene: &Scene, targets: &Targets, cache: &mut Cache) -> Result<RenderedScene, String> {
    assert_eq!(
        targets.detail.len(),
        targets.splats.len(),
        "a target names one detail level and one splat level per shown set"
    );
    let mut layers = Vec::new();
    let mut picks = Vec::new();
    let mut splats = Vec::new();
    let mut members = Vec::new();
    let mut topology = Digest::new();
    let mut classification = Digest::new();
    let mut appearance = Digest::new();
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
        topology.mix(detail as u64);
        topology.mix(choice.digest());
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
            topology.mix(key);
            topology.mix(stamp);
            splats.push(SplatChunk::single(key, stamp, seeded));
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
            );
            for chunk in &prepared.chunks {
                topology.mix(chunk.key());
                topology.mix(u64::from(chunk.level()));
                topology.flag(chunk.active());
                classification.mix(chunk.data().geometry_stamp());
                classification.mix(chunk.data().style_stamp());
                appearance.mix(chunk.data().style_stamp());
            }
            splats.extend(prepared.chunks);
            members.push(prepared.members);
            let spots = frame.spots().expect("the frame carries spots");
            let selected = scene
                .selections()
                .iter()
                .filter(|held| held.set == set.id())
                .filter_map(|held| match held.item {
                    Item::Object(index) => Some(index as u32),
                    Item::Track(_) => None,
                })
                .collect::<Vec<_>>();
            let visible = |index: usize| set.visible_object(time, index);
            let highlight = spot::highlights(spots, &selected, set.opacity(), visible);
            if !highlight.geometry.is_empty() {
                let key = splat_key(set.id().raw(), HIGHLIGHT_PART);
                let stamp = highlight_stamp(scene, set);
                topology.mix(key);
                appearance.mix(stamp);
                splats.push(SplatChunk::new(
                    key,
                    0,
                    true,
                    SplatLevel::new(
                        stamp,
                        stamp,
                        Arc::new(highlight.geometry),
                        Arc::new(highlight.style),
                    ),
                ));
                members.push(SplatMembers::new(key, set.id(), highlight.members));
            }
        }
        let (geometry, level) = match frame.as_ref() {
            ObjectFrame::Surfaces(surfaces) => {
                let (mesh, available) =
                    surfaces.detail(detail).map_err(|error| error.to_string())?;
                (Some(mesh), available)
            }
            ObjectFrame::Cells(cells) => {
                let (mesh, available) = cells.detail(detail).map_err(|error| error.to_string())?;
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
                        ids.push(Selection {
                            set: set.id(),
                            item: Item::Object(index),
                        });
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
                    for index in 0..count {
                        let shown = visible && set.visible_track(index);
                        styles.push(
                            Style::new(
                                set.track_color(index),
                                shown,
                                scene.track_selected(set.id(), index),
                                set.opacity(),
                            )
                            .with_material(set.material()),
                        );
                        ids.push(Selection {
                            set: set.id(),
                            item: Item::Track(index),
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

fn channel_rows(scene: &Scene) -> Vec<ChannelRow> {
    scene
        .channels
        .iter()
        .map(|channel| ChannelRow {
            visible: channel.enabled,
            opacity: channel.opacity,
            window: channel.window,
            transfer: channel.transfer().clone(),
            volume: channel.volume(),
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
