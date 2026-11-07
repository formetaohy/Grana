use std::sync::Arc;

use grana_display::{Shading, TransferFunction};
use grana_object::{RelationFrame, TrackSet};
use grana_volume::{Clip, Intensity, Region};

use crate::annotation::AnnotationState;
use crate::coloc::{ColocState, MaskKey};
use crate::color::ObjectColor;
use crate::curation::Curation;
use crate::derived::DerivedSpec;
use crate::drift::Drift;
use crate::key::ObjectSetKey;
use crate::mask::MaskFilter;
use crate::object::{ObjectFrame, ObjectKind, ObjectParams, TrackStyle};
use crate::object_set::{ObjectSetId, Visibility};
use crate::provenance::Provenance;
use crate::scene::{Item, Scene, Selection};
use crate::seed::Seeds;
use crate::track_color::TrackColor;
use grana_display::ColorState;

pub(crate) const HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, PartialEq)]
pub struct SceneState {
    pub channels: Vec<ChannelState>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
    pub exposure: f32,
    pub region: Region,
    pub clip: Clip,
    pub drift: Drift,
    pub mask: Option<u64>,
    pub mask_filter: Option<MaskFilter>,
    pub mask_radius: f32,
    pub annotations: AnnotationState,
    pub time: u32,
    pub selection: Option<SelectionState>,
    pub sets: Vec<SetState>,
    pub coloc: ColocState,
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
pub struct RelationState {
    pub target: u64,
    pub shift: i32,
    pub key: String,
    pub frames: Vec<Option<Arc<RelationFrame>>>,
}

#[derive(Debug, PartialEq)]
pub struct SetState {
    pub id: u64,
    pub key: ObjectSetKey,
    pub kind: ObjectKind,
    pub name: String,
    pub channel: usize,
    pub color: [f32; 3],
    pub visible: bool,
    pub labels: bool,
    pub label: Option<ObjectColor>,
    pub hidden: Vec<Vec<bool>>,
    pub opacity: f32,
    pub params: ObjectParams,
    pub track_style: Option<TrackStyle>,
    pub tracks: Option<Arc<TrackSet>>,
    pub track_thickness: Option<f32>,
    pub track_visible: Option<bool>,
    pub object_color: ColorState<ObjectColor>,
    pub track_color: ColorState<TrackColor>,
    pub classifier: Option<grana_measure::ClassifierState>,
    pub visibility: Visibility,
    pub seeds: Seeds,
    pub frames: Vec<Option<Arc<ObjectFrame>>>,
    pub relations: Vec<RelationState>,
    pub provenance: Provenance,
    pub curation: Curation,
    pub document: u64,
}

impl SceneState {
    pub fn matches(&self, channel_count: usize, frame_count: u32) -> bool {
        self.channels.len() == channel_count
            && self.selected < channel_count
            && self.time < frame_count
            && (self.drift.is_empty()
                || (self.drift.frames() == frame_count
                    && self.drift.channel() < channel_count
                    && self.drift.reference() < frame_count))
            && self.mask_radius.is_finite()
            && self.mask_radius >= 0.0
            && self
                .annotations
                .selected
                .is_none_or(|index| index < self.annotations.entries.len())
            && self
                .annotations
                .entries
                .iter()
                .all(|entry| entry.frame().is_none_or(|frame| frame < frame_count))
            && self.mask_filter.is_none_or(|filter| {
                self.mask.is_some_and(|mask| {
                    self.sets.iter().any(|set| {
                        set.id == mask
                            && set.relations.iter().any(|relation| {
                                relation.target == filter.target().raw()
                                    && relation.shift == filter.shift()
                            })
                    })
                }) && self.sets.iter().any(|set| set.id == filter.target().raw())
            })
            && self.sets.iter().all(|set| {
                set.classifier.as_ref().is_none_or(|classifier| {
                    classifier.features.iter().all(|feature| {
                        set.kind.measurements(channel_count).contains(feature)
                            && match feature {
                                grana_measure::Measurement::Intensity { channel, .. } => {
                                    *channel < channel_count
                                }
                                _ => true,
                            }
                    })
                })
            })
            && self
                .sets
                .iter()
                .all(|set| set.channel < channel_count && set.frames.len() == frame_count as usize)
            && self.sets.iter().all(|set| match &set.seeds {
                Seeds::Points(frames) => frames.len() == frame_count as usize,
                Seeds::Source(source) => source.raw() > 0 && source.raw() != set.id,
            })
            && self.sets.iter().all(|set| {
                set.hidden.is_empty()
                    || (set.hidden.len() == set.frames.len()
                        && set.hidden.iter().zip(&set.frames).all(|(mask, frame)| {
                            mask.len() == frame.as_ref().map_or(0, |frame| frame.object_count())
                        }))
            })
            && self
                .coloc
                .pair
                .is_none_or(|pair| pair.a() < channel_count && pair.b() < channel_count)
            && self.coloc.results.iter().all(|result| {
                let pair = result.pair();
                pair.a() < channel_count
                    && pair.b() < channel_count
                    && result.series().frames().len() == frame_count as usize
                    && result
                        .mask()
                        .is_none_or(|mask| self.sets.iter().any(|set| set.id == mask))
            })
            && self
                .coloc
                .results
                .iter()
                .enumerate()
                .all(|(index, result)| {
                    self.coloc.results[index + 1..]
                        .iter()
                        .all(|other| other.pair() != result.pair())
                })
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
            exposure: self.exposure,
            region: self.region,
            clip: self.clip,
            drift: self.drift().clone(),
            mask: self.mask.map(|set| set.raw()),
            mask_filter: self.mask_filter,
            mask_radius: self.mask_radius,
            annotations: self.annotations.state(),
            time: self.time(),
            selection: self.selection().map(|selection| SelectionState {
                set: selection.set.raw(),
                item: match selection.item {
                    Item::Object(index) => ItemState::Object(index),
                    Item::Track(index) => ItemState::Track(index),
                },
            }),
            sets: self.objects.iter().map(|set| set.state()).collect(),
            coloc: self.coloc.state(),
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
        self.exposure = state.exposure;
        self.region = state.region;
        self.clip = state.clip;
        self.set_drift(state.drift);
        self.objects.restore(state.sets, &self.dataset);
        self.mask = state
            .mask
            .and_then(ObjectSetId::from_raw)
            .filter(|set| self.objects.get(*set).is_some());
        self.mask_filter = state.mask_filter.filter(|filter| {
            self.objects.get(filter.target()).is_some()
                && self.mask.is_some_and(|mask| {
                    self.objects
                        .get(mask)
                        .is_some_and(|set| set.relation(filter.target(), filter.shift()).is_some())
                })
        });
        self.mask_radius = state.mask_radius;
        self.annotations.restore(state.annotations);
        let mask = self.mask.and_then(|set| self.objects.get(set)).map(|set| {
            MaskKey::new(
                set.id(),
                set.document(),
                state.mask_radius,
                self.mask_filter,
            )
        });
        self.coloc.restore(
            state.coloc,
            self.channels.len(),
            self.frame_count(),
            state.region,
            mask,
        );
        self.set_time(state.time);
        self.select(state.selection.map(|selection| Selection {
            set: ObjectSetId::new(selection.set),
            item: match selection.item {
                ItemState::Object(index) => Item::Object(index),
                ItemState::Track(index) => Item::Track(index),
            },
        }));
        self.validate_selection();
        self.sync_masks();
    }

    pub fn document_hash(&self) -> u64 {
        let mut hash = HASH_OFFSET;
        for entry in self.derived() {
            let spec = entry.spec();
            hash = mix(hash, spec.source() as u64);
            for byte in spec.name().bytes() {
                hash = mix(hash, u64::from(byte));
            }
            for component in spec.tint() {
                hash = mix(hash, u64::from(component.to_bits()));
            }
            match spec {
                DerivedSpec::Mask(mask) => hash = mix(hash, mask.set().raw()),
                DerivedSpec::Restore(restore) => {
                    let params = restore.params();
                    for value in [
                        params.numerical_aperture(),
                        params.wavelength(),
                        params.immersion(),
                        params.sample(),
                        params.depth(),
                    ] {
                        hash = mix(hash, u64::from(value.to_bits()));
                    }
                    hash = mix(hash, u64::from(restore.iterations()));
                }
            }
        }
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
        hash = mix(hash, u64::from(self.exposure.to_bits()));
        for value in [
            self.shading.ambient,
            self.shading.diffuse,
            self.shading.specular,
            self.shading.shininess,
            self.shading.shadows,
            self.shading.azimuth,
            self.shading.elevation,
        ] {
            hash = mix(hash, u64::from(value.to_bits()));
        }
        for axis in 0..3 {
            hash = mix(hash, u64::from(self.region.min()[axis].to_bits()));
            hash = mix(hash, u64::from(self.region.max()[axis].to_bits()));
        }
        hash = mix(hash, u64::from(u8::from(self.region.enabled())));
        for value in self
            .clip
            .normal()
            .to_array()
            .into_iter()
            .chain([self.clip.offset()])
        {
            hash = mix(hash, u64::from(value.to_bits()));
        }
        for set in self.objects.iter() {
            hash = mix(hash, set.id().raw());
            hash = mix(hash, set.kind().index() as u64);
            hash = mix(hash, set.document());
            hash = mix(hash, set.seed_source().map_or(0, ObjectSetId::raw));
            for time in 0..self.frame_count() {
                for seed in set.seeds(time) {
                    for value in seed {
                        hash = mix(hash, u64::from(value.to_bits()));
                    }
                }
            }
        }
        hash = mix(hash, self.mask.map_or(0, ObjectSetId::raw));
        hash = mix(hash, u64::from(self.mask_radius.to_bits()));
        hash = mix(hash, self.annotations.document());
        if let Some(filter) = self.mask_filter {
            hash = mix(hash, 1);
            hash = mix(hash, filter.target().raw());
            hash = mix(hash, u64::from(filter.shift() as u32));
            hash = mix(hash, u64::from(filter.field().index()));
            for value in filter.range() {
                hash = mix(hash, u64::from(value.to_bits()));
            }
        } else {
            hash = mix(hash, 0);
        }
        for offset in self.drift().offsets() {
            for value in offset {
                hash = mix(hash, u64::from(value.to_bits()));
            }
        }
        hash = mix(hash, self.coloc.document());
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
