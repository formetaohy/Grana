use glam::Vec3;
use grana_display::{Ray, Shading, TransferFunction};
use grana_object::TrackSet;
use grana_volume::{Axis, ChannelInfo, Intensity, Region};

use crate::channel::Channel;
use crate::coloc::Coloc;
use crate::dataset::Dataset;
use crate::object::{ObjectFrame, ObjectKind};
use crate::object_set::{ObjectSet, ObjectSetId};
use crate::objects::Objects;
use crate::pick;

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

pub struct Scene {
    pub dataset: Dataset,
    pub channels: Vec<Channel>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
    pub exposure: f32,
    pub region: Region,
    pub objects: Objects,
    pub coloc: Coloc,
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
            .collect::<Vec<Channel>>();
        let half = 0.5 * dataset.world_size();
        let channel_count = channels.len();
        Self {
            dataset,
            channels,
            selected: 0,
            shading: Shading::default(),
            step_scale: 1.0,
            exposure: 1.0,
            region: Region::full(-half, half),
            objects: Objects::new(),
            coloc: Coloc::new(channel_count),
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
        if !self.objects.install(id, time, frame) {
            return;
        }
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
        self.objects.clear(id);
        if self.selection.is_some_and(|selection| selection.set == id) {
            self.selection = None;
        }
    }

    pub fn install_tracks(&mut self, id: ObjectSetId, set: TrackSet) {
        if self.objects.install_tracks(id, set) {
            self.drop_track_selection(id);
        }
    }

    pub fn clear_tracks(&mut self, id: ObjectSetId) {
        self.objects.clear_tracks(id);
        self.drop_track_selection(id);
    }

    pub fn cut_track(&mut self, id: ObjectSetId, time: u32, object: usize) -> bool {
        if !self.objects.cut_track(id, time, object) {
            return false;
        }
        self.drop_track_selection(id);
        true
    }

    pub fn join_tracks(
        &mut self,
        id: ObjectSetId,
        source: (u32, usize),
        target: (u32, usize),
    ) -> bool {
        if !self.objects.join_tracks(id, source, target) {
            return false;
        }
        self.drop_track_selection(id);
        true
    }

    pub fn set_track_thickness(&mut self, id: ObjectSetId, thickness: f32) {
        self.objects.set_track_thickness(id, thickness);
    }

    fn drop_track_selection(&mut self, id: ObjectSetId) {
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

    pub fn pick_ray(&self, ray: &Ray) -> Option<Selection> {
        pick::ray(self, self.time, ray)
    }

    pub fn pick_slice(&self, axis: Axis, point: Vec3) -> Option<Selection> {
        pick::slice(self, self.time, axis, point)
    }

    pub fn geometry_bytes(&self) -> u64 {
        self.objects
            .iter()
            .map(|set| {
                set.frames()
                    .map(|(_, frame)| frame.geometry_bytes())
                    .sum::<u64>()
            })
            .sum()
    }

    pub fn pickable(&self) -> impl Iterator<Item = &ObjectSet> + '_ {
        self.objects.iter().filter(|set| self.shown(set))
    }

    pub fn shown(&self, set: &ObjectSet) -> bool {
        set.visible() && self.channels[set.channel()].enabled
    }

    pub fn object_selected(&self, set: ObjectSetId, index: usize) -> bool {
        self.selection
            == Some(Selection {
                set,
                item: Item::Object(index),
            })
    }

    pub fn track_selected(&self, set: ObjectSetId, index: usize) -> bool {
        self.selection
            == Some(Selection {
                set,
                item: Item::Track(index),
            })
    }
}
