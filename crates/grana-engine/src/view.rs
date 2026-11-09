use glam::Vec3;
use grana_display::{Camera, Shading};
use grana_object::Spot;
use grana_render::{ChannelRow, Layer, LayerKey, Marker, SceneFrame, Style};
use grana_scene::{Item, ObjectFrame, ObjectSet, Scene, Selection};

const SELECTED_COLOR: [f32; 3] = [1.00, 0.93, 0.62];
const SELECTED_SCALE: f32 = 1.3;
const SEED_COLOR: [f32; 3] = [1.00, 0.97, 0.90];
const SEED_SCALE: f32 = 2.0;
const PIXEL_TRIANGLE_AREA: f32 = 4.0;
const MINIMUM_TARGET: f32 = 512.0;

#[derive(Clone, Copy)]
pub struct Lens<'a> {
    pub camera: &'a Camera,
    pub height: u32,
}

pub struct RenderedScene {
    pub channels: Vec<ChannelRow>,
    pub styles: Vec<Style>,
    pub layers: Vec<Layer>,
    pub markers: Vec<Marker>,
    pub style_ids: Vec<Selection>,
    pub marker_ids: Vec<Option<Selection>>,
    pub shading: Shading,
    pub step_scale: f32,
}

impl RenderedScene {
    pub fn frame(&self) -> SceneFrame<'_> {
        SceneFrame {
            channels: &self.channels,
            shading: self.shading,
            step_scale: self.step_scale,
            styles: &self.styles,
            layers: &self.layers,
            markers: &self.markers,
        }
    }
}

pub fn frame(scene: &Scene, lens: Option<Lens<'_>>) -> Result<RenderedScene, String> {
    let mut styles = Vec::new();
    let mut layers = Vec::new();
    let mut markers = Vec::new();
    let mut style_ids = Vec::new();
    let mut marker_ids = Vec::new();
    let seed_radius = SEED_SCALE * scene.dataset.voxel_size().min_element();
    for set in scene.objects.iter() {
        if !scene.shown(set) {
            continue;
        }
        for seed in scene.seeds(set.id()) {
            markers.push(Marker::new(*seed, seed_radius, SEED_COLOR, 1.0));
            marker_ids.push(None);
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
            for (index, spot) in spots.spots().iter().enumerate() {
                if !set.visible_object(scene.time(), index) {
                    continue;
                }
                markers.push(marker(scene, set, index, *spot, color(index)));
                marker_ids.push(Some(Selection {
                    set: set.id(),
                    item: Item::Object(index),
                }));
            }
        }
        let wanted = match (frame, lens) {
            (ObjectFrame::Surfaces(surfaces), Some(lens)) => {
                surfaces.select_detail(triangle_target(surfaces, lens))
            }
            _ => 0,
        };
        let (geometry, detail) = match frame {
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
                styles.push(Style::new(
                    color(index),
                    set.visible_object(scene.time(), index),
                    scene.object_selected(set.id(), index),
                    set.opacity(),
                ));
                style_ids.push(Selection {
                    set: set.id(),
                    item: Item::Object(index),
                });
            }
            layers.push(Layer {
                key: LayerKey::at_frame(
                    set.id().raw(),
                    0,
                    scene.time(),
                    stamp.raw(),
                    detail as u16,
                ),
                mesh,
                style_start: start,
                style_count: frame.object_count(),
            });
        }
        if let Some(tracks) = set.tracks()
            && !tracks.set().is_empty()
        {
            let visible = set.track_visible();
            let start = styles.len();
            for index in 0..tracks.set().len() {
                styles.push(Style::new(
                    set.track_color(index),
                    visible && set.visible_track(index),
                    scene.track_selected(set.id(), index),
                    set.opacity(),
                ));
                style_ids.push(Selection {
                    set: set.id(),
                    item: Item::Track(index),
                });
            }
            layers.push(Layer {
                key: LayerKey::stable(set.id().raw(), 1, tracks.revision().raw()),
                mesh: tracks.mesh(),
                style_start: start,
                style_count: tracks.set().len(),
            });
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
        })
        .collect();
    Ok(RenderedScene {
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

fn marker(scene: &Scene, set: &ObjectSet, index: usize, spot: Spot, color: [f32; 3]) -> Marker {
    if scene.object_selected(set.id(), index) {
        Marker::new(
            spot.center(),
            SELECTED_SCALE * spot.radius(),
            SELECTED_COLOR,
            set.opacity(),
        )
    } else {
        Marker::new(spot.center(), spot.radius(), color, set.opacity())
    }
}

fn triangle_target(surfaces: &grana_object::SurfaceSet, lens: Lens<'_>) -> usize {
    let focal = 0.5 * lens.height as f32 / (0.5 * lens.camera.fov_y()).tan();
    let eye = lens.camera.eye();
    let mut area = 0.0f64;
    for object in surfaces.objects() {
        let bounds = object.bounds();
        let radius = 0.5 * Vec3::from(bounds.size()).length();
        let center = Vec3::from(bounds.center());
        let distance = (eye - center).length().max(0.5 * radius).max(1.0e-6);
        let pixels = f64::from(radius * focal / distance);
        area += std::f64::consts::PI * pixels * pixels / f64::from(PIXEL_TRIANGLE_AREA);
    }
    area.max(f64::from(MINIMUM_TARGET)) as usize
}
