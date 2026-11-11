use glam::Vec3;
use grana_display::{Camera, Shading};
use grana_render::{
    ChannelRow, Digest, Layer, LayerKey, Marker, MarkerChunk, Marks, SceneFrame, Style, Versions,
};
use grana_scene::{Item, ObjectFrame, Scene, Selection};

const SELECTED_COLOR: [f32; 3] = [1.00, 0.93, 0.62];
const SELECTED_SCALE: f32 = 1.3;
const SEED_COLOR: [f32; 3] = [1.00, 0.97, 0.90];
const SEED_SCALE: f32 = 2.0;
const SEED_PART: u64 = 0;
const DETECTION_PART: u64 = 1;
const PIXEL_TRIANGLE_AREA: f32 = 4.0;
const MINIMUM_TARGET: f32 = 512.0;

#[derive(Clone, Copy)]
pub struct Lens<'a> {
    pub camera: &'a Camera,
    pub height: u32,
}

pub struct RenderedScene {
    pub versions: Versions,
    pub channels: Vec<ChannelRow>,
    pub styles: Vec<Style>,
    pub layers: Vec<Layer>,
    pub markers: Vec<MarkerChunk>,
    pub style_ids: Vec<Selection>,
    pub marker_ids: Vec<Option<Selection>>,
    pub shading: Shading,
    pub step_scale: f32,
}

impl RenderedScene {
    pub fn frame(&self) -> SceneFrame<'_> {
        SceneFrame {
            versions: self.versions,
            channels: &self.channels,
            shading: self.shading,
            step_scale: self.step_scale,
            styles: &self.styles,
            layers: &self.layers,
            markers: Marks::new(&self.markers),
        }
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
        key.mix(
            set.tracks()
                .map_or(u64::MAX, |tracks| tracks.revision().raw()),
        );
        key.flag(scene.shown(set));
    }
    key.finish()
}

pub fn detail(scene: &Scene, lens: Option<Lens<'_>>) -> Vec<usize> {
    let mut details = Vec::new();
    for set in scene.objects.iter() {
        if !scene.shown(set) {
            continue;
        }
        let Some(frame) = set.frame(scene.time()) else {
            details.push(0);
            continue;
        };
        details.push(match (frame, lens) {
            (ObjectFrame::Surfaces(surfaces), Some(lens)) => {
                surfaces.select_detail(triangle_target(surfaces, lens))
            }
            _ => 0,
        });
    }
    details
}

pub fn frame(scene: &Scene, detail: &[usize]) -> Result<RenderedScene, String> {
    let mut styles = Vec::new();
    let mut layers = Vec::new();
    let mut markers = Vec::new();
    let mut style_ids = Vec::new();
    let mut marker_ids = Vec::new();
    let mut data = Digest::new();
    let mut objects = Digest::new();
    let mut appearance = Digest::new();
    let mut ordinal = 0usize;
    let seed_radius = SEED_SCALE * scene.dataset.voxel_size().min_element();
    data.mix(scene.channels.len() as u64);
    data.mix(scene.dataset.channels().len() as u64);
    for set in scene.objects.iter() {
        objects.mix(set.id().raw());
        objects.flag(scene.shown(set));
        let Some(wanted) = scene
            .shown(set)
            .then(|| detail.get(ordinal).copied())
            .flatten()
        else {
            continue;
        };
        ordinal += 1;
        objects.mix(
            set.frame_stamp(scene.time())
                .map_or(u64::MAX, |stamp| stamp.raw()),
        );
        objects.mix(
            set.tracks()
                .map_or(u64::MAX, |tracks| tracks.revision().raw()),
        );
        objects.mix(wanted as u64);
        let mut seeded = Vec::new();
        for seed in scene.seeds(set.id()) {
            let [x, y, z] = *seed;
            objects.real(x);
            objects.real(y);
            objects.real(z);
            seeded.push(Marker::new(*seed, seed_radius, SEED_COLOR, 1.0));
            marker_ids.push(None);
        }
        if !seeded.is_empty() {
            markers.push(MarkerChunk::new(
                marker_key(set.id().raw(), SEED_PART),
                seeded,
            ));
        }
        let Some(frame) = set.frame(scene.time()) else {
            continue;
        };
        let colors = scene.object_colors(set);
        let color = |index: usize| {
            colors
                .as_ref()
                .and_then(|colors| colors.get(index).copied())
                .unwrap_or_else(|| set.color())
        };
        if let Some(spots) = frame.spots() {
            let mut detected = Vec::new();
            for (index, spot) in spots.spots().iter().enumerate() {
                if !set.visible_object(scene.time(), index) {
                    continue;
                }
                let selected = scene.object_selected(set.id(), index);
                let (radius, tint) = if selected {
                    (SELECTED_SCALE * spot.radius(), SELECTED_COLOR)
                } else {
                    (spot.radius(), color(index))
                };
                let [x, y, z] = spot.center();
                objects.real(x);
                objects.real(y);
                objects.real(z);
                objects.real(radius);
                let [red, green, blue] = tint;
                appearance.real(red);
                appearance.real(green);
                appearance.real(blue);
                appearance.real(set.opacity());
                detected.push(Marker::new(spot.center(), radius, tint, set.opacity()));
                marker_ids.push(Some(Selection {
                    set: set.id(),
                    item: Item::Object(index),
                }));
            }
            if !detected.is_empty() {
                markers.push(MarkerChunk::new(
                    marker_key(set.id().raw(), DETECTION_PART),
                    detected,
                ));
            }
        }
        let (geometry, level) = match frame {
            ObjectFrame::Surfaces(surfaces) => {
                let (mesh, available) =
                    surfaces.detail(wanted).map_err(|error| error.to_string())?;
                (Some(mesh), available)
            }
            _ => (frame.geometry().map_err(|error| error.to_string())?, 0),
        };
        if let (Some(mesh), Some(stamp)) = (geometry, set.frame_stamp(scene.time()))
            && !mesh.is_empty()
        {
            let start = styles.len();
            for index in 0..frame.object_count() {
                let visible = set.visible_object(scene.time(), index);
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
                styles.push(Style::new([red, green, blue], visible, selected, opacity));
                style_ids.push(Selection {
                    set: set.id(),
                    item: Item::Object(index),
                });
            }
            let layer = Layer {
                key: LayerKey::at_frame(set.id().raw(), 0, scene.time(), stamp.raw(), level as u16),
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
                styles.push(Style::new(
                    [red, green, blue],
                    visible && set.visible_track(index),
                    selected,
                    opacity,
                ));
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
        detail.len(),
        "the detail levels cover every shown set"
    );
    Ok(RenderedScene {
        versions: Versions::new(data.finish(), objects.finish(), appearance.finish()),
        channels,
        styles,
        layers,
        markers,
        style_ids,
        marker_ids,
        shading: scene.shading,
        step_scale: scene.step_scale,
    })
}

fn marker_key(set: u64, part: u64) -> u64 {
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
