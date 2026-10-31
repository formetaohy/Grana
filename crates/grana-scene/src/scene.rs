use glam::Vec3;
use grana_object::{Spot, TrackSet};
use grana_render::{
    ChannelRow, Layer, LayerKey, Marker, Ray, SceneFrame, Shading, Style, TransferFunction,
};
use grana_volume::{Axis, ChannelInfo, Intensity, Region};

use crate::channel::Channel;
use crate::dataset::Dataset;
use crate::object::{ObjectFrame, ObjectKind};
use crate::object_set::{ObjectSet, ObjectSetId};
use crate::objects::Objects;
use crate::pick;

const SELECTED_COLOR: [f32; 3] = [1.00, 0.93, 0.62];
const SELECTED_SCALE: f32 = 1.3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub set: ObjectSetId,
    pub item: Item,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Object(usize),
    Track(usize),
}

pub struct RenderedScene<'a> {
    pub channels: Vec<ChannelRow<'a>>,
    pub styles: Vec<Style>,
    pub layers: Vec<Layer<'a>>,
    pub markers: Vec<Marker>,
    pub shading: Shading,
    pub step_scale: f32,
}

impl<'a> RenderedScene<'a> {
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

pub struct Scene {
    pub dataset: Dataset,
    pub channels: Vec<Channel>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
    pub region: Region,
    pub objects: Objects,
    time: u32,
    selection: Option<Selection>,
}

impl Scene {
    pub fn new(dataset: Dataset) -> Self {
        let channels = dataset
            .channels()
            .iter()
            .map(|channel| {
                Channel::new(
                    channel.window(),
                    channel.window(),
                    TransferFunction::tinted(channel.tint()),
                )
            })
            .collect();
        let half = 0.5 * dataset.world_size();
        Self {
            dataset,
            channels,
            selected: 0,
            shading: Shading::default(),
            step_scale: 1.0,
            region: Region::full(-half, half),
            objects: Objects::new(),
            time: 0,
            selection: None,
        }
    }

    pub fn channel(&self, index: usize) -> &ChannelInfo {
        &self.dataset.channels()[index]
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn frame_count(&self) -> u32 {
        self.dataset.frame_count()
    }

    pub fn set_time(&mut self, time: u32) {
        assert!(time < self.frame_count(), "the frame lies inside the scene");
        if time == self.time {
            return;
        }
        self.time = time;
        if self
            .selection
            .is_some_and(|selection| matches!(selection.item, Item::Object(_)))
        {
            self.selection = None;
        }
    }

    pub fn select_channel(&mut self, index: usize) {
        assert!(index < self.channels.len(), "channel index out of bounds");
        self.selected = index;
    }

    pub fn set_window(&mut self, index: usize, start: f32, end: f32) {
        let data = self.dataset.channels()[index].intensity();
        let low = start.min(end).clamp(data.min(), data.max());
        let high = start.max(end).clamp(data.min(), data.max());
        let minimum = (data.span() * 1.0e-3).max(f32::EPSILON);
        let high = if high - low < minimum {
            (low + minimum).min(data.max())
        } else {
            high
        };
        if high - low <= 0.0 {
            return;
        }
        self.channels[index].window = Intensity::new(low, high);
    }

    pub fn reset_window(&mut self, index: usize) {
        self.channels[index].window = self.channels[index].default_window;
    }

    pub fn set_transfer(&mut self, index: usize, transfer: TransferFunction) {
        self.channels[index].set_transfer(transfer);
    }

    pub fn add_object(&mut self, kind: ObjectKind, channel: usize) -> ObjectSetId {
        self.objects.add(kind, channel, &self.dataset)
    }

    pub fn remove_object(&mut self, id: ObjectSetId) {
        self.objects.remove(id);
        if self.selection.is_some_and(|selection| selection.set == id) {
            self.selection = None;
        }
    }

    pub fn install(&mut self, id: ObjectSetId, time: u32, frame: ObjectFrame) {
        let Some(set) = self.objects.get_mut(id) else {
            return;
        };
        set.install(time, frame);
        if time == self.time {
            let stale = self.selection.is_some_and(|selection| {
                selection.set == id && matches!(selection.item, Item::Object(_))
            });
            if stale {
                self.selection = None;
            }
        }
    }

    pub fn clear_object(&mut self, id: ObjectSetId) {
        if let Some(set) = self.objects.get_mut(id) {
            set.clear();
        }
        if self.selection.is_some_and(|selection| selection.set == id) {
            self.selection = None;
        }
    }

    pub fn install_tracks(&mut self, id: ObjectSetId, set: TrackSet) {
        let Some(object) = self.objects.get_mut(id) else {
            return;
        };
        object.install_tracks(set);
        let stale = self.selection.is_some_and(|selection| {
            selection.set == id && matches!(selection.item, Item::Track(_))
        });
        if stale {
            self.selection = None;
        }
    }

    pub fn clear_tracks(&mut self, id: ObjectSetId) {
        if let Some(object) = self.objects.get_mut(id) {
            object.clear_tracks();
        }
        let stale = self.selection.is_some_and(|selection| {
            selection.set == id && matches!(selection.item, Item::Track(_))
        });
        if stale {
            self.selection = None;
        }
    }

    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    pub fn select(&mut self, selection: Option<Selection>) {
        self.selection = selection;
    }

    pub fn select_track(&mut self, id: ObjectSetId, index: usize) {
        assert!(
            self.objects
                .get(id)
                .and_then(ObjectSet::tracks)
                .is_some_and(|layer| index < layer.set().len()),
            "the selected track exists"
        );
        self.selection = Some(Selection {
            set: id,
            item: Item::Track(index),
        });
    }

    pub fn selected_track(&self, id: ObjectSetId) -> Option<usize> {
        match self.selection {
            Some(Selection {
                set,
                item: Item::Track(index),
            }) if set == id => Some(index),
            _ => None,
        }
    }

    pub fn rendered(&self) -> RenderedScene<'_> {
        let mut styles = Vec::new();
        let mut layers = Vec::new();
        let mut markers = Vec::new();
        for set in self.objects.iter() {
            if !set.visible() || !self.channels[set.channel()].enabled {
                continue;
            }
            let Some(frame) = set.frame(self.time) else {
                continue;
            };
            if let Some(spots) = frame.spots() {
                for (index, spot) in spots.spots().iter().enumerate() {
                    if !set.visible_object(self.time, index) {
                        continue;
                    }
                    markers.push(self.marker(set, index, *spot));
                }
            }
            if let (Some(mesh), Some(stamp)) = (frame.mesh(), set.frame_stamp(self.time)) {
                let start = styles.len();
                for index in 0..frame.object_count() {
                    styles.push(Style::new(
                        set.color(),
                        set.visible_object(self.time, index),
                        self.object_selected(set.id(), index),
                    ));
                }
                layers.push(Layer {
                    key: LayerKey::at_frame(set.id().raw(), 0, self.time, stamp),
                    mesh: Some(mesh),
                    style_start: start,
                    style_count: frame.object_count(),
                });
            }
            if let Some(tracks) = set.tracks()
                && !tracks.set().is_empty()
            {
                let start = styles.len();
                for index in 0..tracks.set().len() {
                    styles.push(Style::new(
                        set.color(),
                        tracks.visible() && set.visible_track(index),
                        self.track_selected(set.id(), index),
                    ));
                }
                layers.push(Layer {
                    key: LayerKey::stable(set.id().raw(), 1, tracks.revision()),
                    mesh: Some(tracks.mesh()),
                    style_start: start,
                    style_count: tracks.set().len(),
                });
            }
        }
        let channels = self
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
            shading: self.shading,
            step_scale: self.step_scale,
        }
    }

    pub fn pick_ray(&self, ray: &Ray) -> Option<Selection> {
        pick::ray(self, self.time, ray)
    }

    pub fn pick_slice(&self, axis: Axis, point: Vec3) -> Option<Selection> {
        pick::slice(self, self.time, axis, point)
    }

    pub fn pickable(&self) -> impl Iterator<Item = &ObjectSet> + '_ {
        self.objects
            .iter()
            .filter(|set| set.visible())
            .filter(|set| self.channels[set.channel()].enabled)
    }

    fn object_selected(&self, set: ObjectSetId, index: usize) -> bool {
        self.selection
            == Some(Selection {
                set,
                item: Item::Object(index),
            })
    }

    fn track_selected(&self, set: ObjectSetId, index: usize) -> bool {
        self.selection
            == Some(Selection {
                set,
                item: Item::Track(index),
            })
    }

    fn marker(&self, set: &ObjectSet, index: usize, spot: Spot) -> Marker {
        if self.object_selected(set.id(), index) {
            Marker::new(
                spot.center(),
                SELECTED_SCALE * spot.radius(),
                SELECTED_COLOR,
                1.0,
            )
        } else {
            Marker::new(spot.center(), spot.radius(), set.color(), 1.0)
        }
    }
}
