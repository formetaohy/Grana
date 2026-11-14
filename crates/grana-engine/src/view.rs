use std::sync::Arc;

use glam::Vec3;
use grana_display::{Camera, Shading, srgb_to_linear};
use grana_render::{
    ChannelRow, Digest, Layer, LayerKey, SceneFrame, Splat, SplatChunk, SplatLevel, Splats, Style,
    Versions,
};
use grana_scene::{Item, ObjectFrame, ObjectSet, Scene, Selection};

use crate::splat::{SplatCache, SplatMembers};
use crate::spot::{self, Choice};

const SEED_COLOR: [f32; 3] = [1.00, 0.97, 0.90];
const SEED_SCALE: f32 = 2.0;
const SEED_PART: u64 = 0;
const DETECTION_PART: u64 = 1;
const HIGHLIGHT_PART: u64 = 2;
const PIXEL_TRIANGLE_AREA: f32 = 4.0;
const MINIMUM_TARGET: f32 = 512.0;

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

pub struct RenderedScene {
    pub versions: Versions,
    pub channels: Vec<ChannelRow>,
    pub styles: Vec<Style>,
    pub layers: Vec<Layer>,
    pub splats: Vec<SplatChunk>,
    pub members: Vec<SplatMembers>,
    pub style_ids: Vec<Selection>,
    pub shading: Shading,
    pub step_scale: f32,
}

impl RenderedScene {
    pub fn frame<'a>(&'a self, overlays: &'a [grana_render::Overlay<'a>]) -> SceneFrame<'a> {
        SceneFrame {
            versions: self.versions,
            channels: &self.channels,
            shading: self.shading,
            step_scale: self.step_scale,
            styles: &self.styles,
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
}

pub fn key(scene: &Scene) -> u64 {
    let mut key = Digest::new();
    key.mix(u64::from(scene.time()));
    key.mix(scene.channels.len() as u64);
    key.mix(scene.derived().len() as u64);
    for channel in &scene.channels {
        key.flag(channel.enabled);
        key.real(channel.opacity);
        key.real(channel.window.min());
        key.real(channel.window.max());
        key.mix(channel.volume().mode().index() as u64);
        key.real(channel.volume().isovalue());
        for stop in channel.transfer().stops() {
            key.real(stop.position());
            key.real(stop.opacity());
            for component in stop.color() {
                key.real(component);
            }
        }
    }
    key.real(scene.shading.ambient);
    key.real(scene.shading.diffuse);
    key.real(scene.shading.shadows);
    key.real(scene.shading.azimuth);
    key.real(scene.shading.elevation);
    key.real(scene.step_scale);
    for selection in scene.selections() {
        key.mix(selection.set.raw());
        key.mix(match selection.item {
            Item::Object(index) => (index as u64) * 2,
            Item::Track(index) => (index as u64) * 2 + 1,
        });
    }
    for set in scene.objects.iter() {
        key.mix(set.id().raw());
        key.mix(set.revision().raw());
        key.mix(set.document().raw());
        key.mix(
            set.frame_stamp(scene.time())
                .map_or(u64::MAX, |stamp| stamp.raw()),
        );
        if let Some(source) = set.localization()
            && source.projected()
        {
            for time in source.projected_frames(scene.time(), scene.frame_count()) {
                key.mix(set.frame_stamp(time).map_or(u64::MAX, |stamp| stamp.raw()));
            }
        }
        key.mix(
            set.tracks()
                .map_or(u64::MAX, |tracks| tracks.revision().raw()),
        );
        key.flag(scene.shown(set));
    }
    key.finish()
}

pub fn plan(scene: &Scene, lens: Option<Lens<'_>>) -> Targets {
    plan_with(scene, lens, &mut SplatCache::new())
}

pub fn plan_with(scene: &Scene, lens: Option<Lens<'_>>, splats: &mut SplatCache) -> Targets {
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
                    surfaces.select_detail(triangle_target(surfaces, lens))
                }
                ObjectFrame::Cells(cells) => {
                    cells.select_detail(triangle_target(cells.bodies(), lens))
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
                splats.choose(scene, set, pixels)
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
    cache: &mut SplatCache,
) -> Result<RenderedScene, String> {
    assert_eq!(
        targets.detail.len(),
        targets.splats.len(),
        "a target names one detail level and one splat level per shown set"
    );
    let mut styles = Vec::new();
    let mut layers = Vec::new();
    let mut splats = Vec::new();
    let mut members = Vec::new();
    let mut style_ids = Vec::new();
    let mut data = Digest::new();
    let mut objects = Digest::new();
    let mut appearance = Digest::new();
    let mut ordinal = 0usize;
    let time = scene.time();
    let seed_radius = SEED_SCALE * scene.dataset.voxel_size().min_element();
    data.mix(scene.channels.len() as u64);
    data.mix(scene.dataset.channels().len() as u64);
    for set in scene.objects.iter() {
        let shown = scene.shown(set);
        objects.mix(set.id().raw());
        objects.flag(shown);
        appearance.mix(set.document().raw());
        appearance.mix(set.revision().raw());
        if !shown {
            continue;
        }
        let Some(detail) = targets.detail.get(ordinal).copied() else {
            return Err("the targets leave a shown object set without a detail level".to_string());
        };
        let choice = targets.splats[ordinal];
        ordinal += 1;
        objects.mix(detail as u64);
        objects.mix(choice.digest());
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
            let prepared = cache.prepare(
                scene,
                set,
                splat_key(set.id().raw(), DETECTION_PART),
                choice,
            );
            for chunk in &prepared.chunks {
                objects.mix(chunk.key());
                objects.mix(u64::from(chunk.level()));
                objects.flag(chunk.active());
                objects.mix(chunk.data().geometry_stamp());
                objects.mix(chunk.data().style_stamp());
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
                objects.mix(key);
                objects.mix(stamp);
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
        if let (Some(mesh), Some(stamp)) = (geometry, set.frame_stamp(time))
            && !mesh.is_empty()
        {
            let start = styles.len();
            for index in 0..frame.object_count() {
                let visible = set.visible_object(time, index);
                let selected = scene.object_selected(set.id(), index);
                let opacity = set.opacity();
                let [red, green, blue] = color(index);
                let material = set.material();
                appearance.real(red);
                appearance.real(green);
                appearance.real(blue);
                appearance.flag(visible);
                appearance.flag(selected);
                appearance.real(opacity);
                appearance.real(material.roughness());
                appearance.real(material.metallic());
                styles.push(
                    Style::new([red, green, blue], visible, selected, opacity)
                        .with_material(material),
                );
                style_ids.push(Selection {
                    set: set.id(),
                    item: Item::Object(index),
                });
            }
            let layer = Layer {
                key: LayerKey::at_frame(set.id().raw(), 0, time, stamp.raw(), level as u16),
                mesh,
                style_start: start,
                style_count: frame.object_count(),
            };
            objects.mix(layer.key.digest());
            layers.push(layer);
        }
        if let Some(tracks) = set.tracks()
            && !tracks.set().is_empty()
        {
            let visible = set.track_visible();
            let opacity = set.opacity();
            let material = set.material();
            let start = styles.len();
            for index in 0..tracks.set().len() {
                let selected = scene.track_selected(set.id(), index);
                let [red, green, blue] = set.track_color(index);
                appearance.real(red);
                appearance.real(green);
                appearance.real(blue);
                appearance.flag(visible && set.visible_track(index));
                appearance.flag(selected);
                appearance.real(opacity);
                appearance.real(material.roughness());
                appearance.real(material.metallic());
                styles.push(
                    Style::new(
                        [red, green, blue],
                        visible && set.visible_track(index),
                        selected,
                        opacity,
                    )
                    .with_material(material),
                );
                style_ids.push(Selection {
                    set: set.id(),
                    item: Item::Track(index),
                });
            }
            let layer = Layer {
                key: LayerKey::stable(set.id().raw(), 1, tracks.revision().raw()),
                mesh: tracks.mesh(),
                style_start: start,
                style_count: tracks.set().len(),
            };
            objects.mix(layer.key.digest());
            layers.push(layer);
        }
    }
    for selection in scene.selections() {
        let item = match selection.item {
            Item::Object(index) => (index as u64) * 2,
            Item::Track(index) => (index as u64) * 2 + 1,
        };
        objects.mix(selection.set.raw());
        objects.mix(item);
        appearance.mix(selection.set.raw());
        appearance.mix(item);
    }
    let channels = scene
        .channels
        .iter()
        .map(|channel| ChannelRow {
            visible: channel.enabled,
            opacity: channel.opacity,
            window: channel.window,
            transfer: channel.transfer().clone(),
            volume: channel.volume(),
        })
        .collect();
    assert_eq!(
        ordinal,
        targets.detail.len(),
        "the targets cover every shown set"
    );
    let present = scene
        .objects
        .iter()
        .map(|set| set.id().raw())
        .collect::<Vec<_>>();
    cache.retain(&present);
    Ok(RenderedScene {
        versions: Versions::new(data.finish(), objects.finish(), appearance.finish()),
        channels,
        styles,
        layers,
        splats,
        members,
        style_ids,
        shading: scene.shading,
        step_scale: scene.step_scale,
    })
}

fn seed_stamp(scene: &Scene, set: &ObjectSet) -> u64 {
    let mut stamp = Digest::new();
    stamp.mix(set.id().raw());
    stamp.mix(set.revision().raw());
    stamp.mix(set.document().raw());
    stamp.mix(u64::from(scene.time()));
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

fn triangle_target(surfaces: &grana_object::SurfaceSet, lens: Lens<'_>) -> usize {
    let focal = lens.camera.pixels_per_world(lens.height);
    let eye = lens.camera.eye();
    let orthographic = lens.camera.projection().orthographic();
    let mut area = 0.0f64;
    for object in surfaces.objects() {
        let bounds = object.bounds();
        let radius = 0.5 * Vec3::from(bounds.size()).length();
        let center = Vec3::from(bounds.center());
        let distance = if orthographic {
            lens.camera.distance()
        } else {
            (eye - center).length().max(0.5 * radius)
        }
        .max(1.0e-6);
        let pixels = f64::from(radius * focal / distance);
        area += std::f64::consts::PI * pixels * pixels / f64::from(PIXEL_TRIANGLE_AREA);
    }
    area.max(f64::from(MINIMUM_TARGET)) as usize
}
