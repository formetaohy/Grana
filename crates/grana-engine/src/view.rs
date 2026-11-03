use grana_display::{Shading, srgb_to_linear};
use grana_object::Spot;
use grana_render::{ChannelRow, Layer, LayerKey, Marker, SceneFrame, Style};
use grana_scene::{Item, ObjectSet, Scene, Selection};

const SELECTED_COLOR: [f32; 3] = [1.00, 0.93, 0.62];
const SELECTED_SCALE: f32 = 1.3;
const SEED_COLOR: [f32; 3] = [1.00, 0.97, 0.90];
const SEED_SCALE: f32 = 2.0;

pub struct RenderedScene<'a> {
    pub channels: Vec<ChannelRow<'a>>,
    pub styles: Vec<Style>,
    pub layers: Vec<Layer<'a>>,
    pub markers: Vec<Marker>,
    pub style_ids: Vec<Selection>,
    pub marker_ids: Vec<Option<Selection>>,
    pub shading: Shading,
    pub step_scale: f32,
}

impl RenderedScene<'_> {
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

pub fn frame(scene: &Scene) -> RenderedScene<'_> {
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
        let extent = set.object_extent();
        let metric = set.object_coloring().metric();
        let objects = frame.objects();
        let relation = metric.and_then(|metric| {
            metric
                .target()
                .and_then(|target| set.relation(target))
                .and_then(|relation| relation.frame(scene.time()))
        });
        let color = |index: usize| {
            extent
                .zip(metric)
                .map(|(extent, metric)| {
                    let value = metric.value(objects, relation, index);
                    if value.is_finite() {
                        set.object_coloring().color(extent, value)
                    } else {
                        set.color()
                    }
                })
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
        if let (Some(mesh), Some(stamp)) = (frame.geometry(), set.frame_stamp(scene.time()))
            && !mesh.is_empty()
        {
            let start = styles.len();
            for index in 0..frame.object_count() {
                styles.push(Style::new(
                    srgb_to_linear(color(index)),
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
                key: LayerKey::at_frame(set.id().raw(), 0, scene.time(), stamp),
                mesh,
                style_start: start,
                style_count: frame.object_count(),
            });
        }
        if let Some(tracks) = set.tracks()
            && !tracks.set().is_empty()
        {
            let extent = set.track_extent();
            let metric = set.track_coloring().metric();
            let color = |index: usize| {
                extent
                    .zip(metric)
                    .map(|(extent, metric)| {
                        set.track_coloring()
                            .color(extent, metric.value(index, tracks.set()))
                    })
                    .unwrap_or_else(|| set.color())
            };
            let start = styles.len();
            for index in 0..tracks.set().len() {
                styles.push(Style::new(
                    srgb_to_linear(color(index)),
                    tracks.visible() && set.visible_track(index),
                    scene.track_selected(set.id(), index),
                    set.opacity(),
                ));
                style_ids.push(Selection {
                    set: set.id(),
                    item: Item::Track(index),
                });
            }
            layers.push(Layer {
                key: LayerKey::stable(set.id().raw(), 1, tracks.revision()),
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
            transfer: channel.transfer(),
        })
        .collect();
    RenderedScene {
        channels,
        styles,
        layers,
        markers,
        style_ids,
        marker_ids,
        shading: scene.shading,
        step_scale: scene.step_scale,
    }
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
