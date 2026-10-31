use grana_object::TrackSet;
use grana_render::{Shading, TransferFunction};
use grana_volume::{Intensity, Region};

use crate::object::{ObjectFrame, ObjectKind, ObjectParams, TrackStyle};
use crate::object_set::{ObjectSetId, Visibility};
use crate::scene::{Item, Scene, Selection};

pub(crate) const HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, PartialEq)]
pub struct SceneState {
    pub channels: Vec<ChannelState>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
    pub region: Region,
    pub time: u32,
    pub selection: Option<SelectionState>,
    pub sets: Vec<SetState>,
}

#[derive(Debug, PartialEq)]
pub struct ChannelState {
    pub enabled: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub transfer: TransferFunction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionState {
    pub set: u64,
    pub item: ItemState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemState {
    Object(usize),
    Track(usize),
}

#[derive(Debug, PartialEq)]
pub struct SetState {
    pub id: u64,
    pub kind: ObjectKind,
    pub name: String,
    pub channel: usize,
    pub color: [f32; 3],
    pub visible: bool,
    pub params: ObjectParams,
    pub track_style: Option<TrackStyle>,
    pub tracks: Option<TrackSet>,
    pub track_thickness: Option<f32>,
    pub track_visible: Option<bool>,
    pub visibility: Visibility,
    pub frames: Vec<Option<ObjectFrame>>,
}

impl SceneState {
    pub fn matches(&self, channel_count: usize, frame_count: u32) -> bool {
        self.channels.len() == channel_count
            && self.selected < channel_count
            && self.time < frame_count
            && self
                .sets
                .iter()
                .all(|set| set.channel < channel_count && set.frames.len() == frame_count as usize)
    }
}

impl Scene {
    pub fn state(&self) -> SceneState {
        SceneState {
            channels: self
                .channels
                .iter()
                .map(|channel| ChannelState {
                    enabled: channel.enabled,
                    opacity: channel.opacity,
                    window: channel.window,
                    transfer: channel.transfer().clone(),
                })
                .collect(),
            selected: self.selected,
            shading: self.shading,
            step_scale: self.step_scale,
            region: self.region,
            time: self.time(),
            selection: self.selection().map(|selection| SelectionState {
                set: selection.set.raw(),
                item: match selection.item {
                    Item::Object(index) => ItemState::Object(index),
                    Item::Track(index) => ItemState::Track(index),
                },
            }),
            sets: self.objects.iter().map(|set| set.state()).collect(),
        }
    }

    pub fn restore(&mut self, state: SceneState) {
        assert!(
            state.matches(self.channels.len(), self.frame_count()),
            "a scene state matches the dataset it was captured from"
        );
        for (channel, source) in self.channels.iter_mut().zip(state.channels) {
            channel.enabled = source.enabled;
            channel.opacity = source.opacity;
            channel.window = source.window;
            channel.set_transfer(source.transfer);
        }
        self.selected = state.selected;
        self.shading = state.shading;
        self.step_scale = state.step_scale;
        self.region = state.region;
        self.objects.restore(state.sets, &self.dataset);
        self.set_time(state.time);
        self.select(state.selection.map(|selection| Selection {
            set: ObjectSetId::new(selection.set),
            item: match selection.item {
                ItemState::Object(index) => Item::Object(index),
                ItemState::Track(index) => Item::Track(index),
            },
        }));
        self.validate_selection();
    }

    pub fn document_hash(&self) -> u64 {
        let mut hash = HASH_OFFSET;
        for channel in &self.channels {
            hash = mix(hash, u64::from(u8::from(channel.enabled)));
            hash = mix(hash, u64::from(channel.opacity.to_bits()));
            hash = mix(hash, u64::from(channel.window.min().to_bits()));
            hash = mix(hash, u64::from(channel.window.max().to_bits()));
            for stop in channel.transfer().stops() {
                hash = mix(hash, u64::from(stop.position().to_bits()));
                hash = mix(hash, u64::from(stop.opacity().to_bits()));
                for component in stop.color() {
                    hash = mix(hash, u64::from(component.to_bits()));
                }
            }
        }
        hash = mix(hash, self.selected as u64);
        hash = mix(hash, u64::from(self.step_scale.to_bits()));
        for value in [
            self.shading.ambient,
            self.shading.diffuse,
            self.shading.specular,
            self.shading.shininess,
        ] {
            hash = mix(hash, u64::from(value.to_bits()));
        }
        for axis in 0..3 {
            hash = mix(hash, u64::from(self.region.min()[axis].to_bits()));
            hash = mix(hash, u64::from(self.region.max()[axis].to_bits()));
        }
        hash = mix(hash, u64::from(u8::from(self.region.enabled())));
        for set in self.objects.iter() {
            hash = mix(hash, set.id().raw());
            hash = mix(hash, set.kind().index() as u64);
            hash = mix(hash, set.document());
        }
        hash
    }

    fn validate_selection(&mut self) {
        let Some(selection) = self.selection() else {
            return;
        };
        let valid = self
            .objects
            .get(selection.set)
            .is_some_and(|set| match selection.item {
                Item::Object(index) => set
                    .frame(self.time())
                    .is_some_and(|frame| index < frame.object_count()),
                Item::Track(index) => set.tracks().is_some_and(|layer| index < layer.set().len()),
            });
        if !valid {
            self.select(None);
        }
    }
}

pub(crate) fn mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(HASH_PRIME)
}
