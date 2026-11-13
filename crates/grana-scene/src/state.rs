use std::sync::Arc;

use grana_display::{Shading, TransferFunction, VolumeStyle};
use grana_measure::ObjectColoc;
use grana_object::{RelationFrame, TrackSet};
use grana_volume::{Clip, Intensity, Region};

use crate::annotation::AnnotationState;
use crate::coloc::{ColocState, MaskKey};
use crate::color::ObjectColor;
use crate::curation::Curation;
use crate::derived::{DerivedSpec, Operation};
use crate::drifts::Drifts;
use crate::key::ObjectSetKey;
use crate::label_export::LabelExport;
use crate::localization::LocalizationSource;
use crate::mask::MaskFilter;
use crate::object::{ObjectFrame, ObjectKind, ObjectParams, TrackStyle};
use crate::object_set::ObjectSetId;
use crate::provenance::Provenance;
use crate::registration::Registration;
use crate::revision::Revision;
use crate::scene::{Item, Scene, Selection};
use crate::seed::Seeds;
use crate::track_color::TrackColor;
use crate::validate::SceneShape;
use grana_display::{ColorState, Material};

pub(crate) const HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, PartialEq)]
pub struct SceneState {
    pub channels: Vec<ChannelState>,
    pub derived: Vec<Arc<DerivedSpec>>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
    pub exposure: f32,
    pub region: Region,
    pub clip: Clip,
    pub drifts: Drifts,
    pub registration: Registration,
    pub mask: Option<u64>,
    pub mask_filter: Option<MaskFilter>,
    pub mask_radius: f32,
    pub annotations: AnnotationState,
    pub time: u32,
    pub selections: Vec<SelectionState>,
    pub sets: Vec<SetState>,
    pub coloc: ColocState,
}

#[derive(Debug, PartialEq)]
pub struct ChannelState {
    pub enabled: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub transfer: TransferFunction,
    pub volume: VolumeStyle,
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
    pub material: Material,
    pub params: ObjectParams,
    pub track_style: Option<TrackStyle>,
    pub tracks: Option<Arc<TrackSet>>,
    pub object_color: ColorState<ObjectColor>,
    pub track_color: ColorState<TrackColor>,
    pub classifier: Option<grana_measure::ClassifierState>,
    pub object_coloc: Option<ObjectColoc>,
    pub label_export: Option<LabelExport>,
    pub localization: Option<LocalizationSource>,
    pub seeds: Seeds,
    pub frames: Vec<Option<Arc<ObjectFrame>>>,
    pub relations: Vec<RelationState>,
    pub provenance: Provenance,
    pub version: u32,
    pub curation: Curation,
    pub document: Revision,
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
                    volume: channel.volume(),
                })
                .collect(),
            derived: self.derived_shared(),
            selected: self.selected,
            shading: self.shading,
            step_scale: self.step_scale,
            exposure: self.exposure,
            region: self.region,
            clip: self.clip,
            drifts: self.drifts().clone(),
            registration: self.registration().clone(),
            mask: self.mask.map(|set| set.raw()),
            mask_filter: self.mask_filter,
            mask_radius: self.mask_radius,
            annotations: self.annotations.state(),
            time: self.time(),
            selections: self
                .selections()
                .iter()
                .map(|selection| SelectionState {
                    set: selection.set.raw(),
                    item: match selection.item {
                        Item::Object(index) => ItemState::Object(index),
                        Item::Track(index) => ItemState::Track(index),
                    },
                })
                .collect(),
            sets: self.objects.iter().map(|set| set.state()).collect(),
            coloc: self.coloc.state(),
        }
    }

    pub fn restore(&mut self, state: SceneState) {
        let shape = SceneShape::new(self.base_channels(), self.frame_count());
        if let Err(fault) = state.validate(shape) {
            panic!(
                "a scene state matches the dataset it was captured from: {}",
                fault.detail()
            );
        }
        let derived = state.derived != self.derived_shared();
        if derived {
            let specs = state
                .derived
                .iter()
                .map(|spec| spec.as_ref().clone())
                .collect::<Vec<_>>();
            let infos = crate::derived::derived_infos(
                &specs,
                &self.dataset.channels()[..self.base_channels()],
            )
            .expect("a scene state names a derived layout of its dataset");
            self.set_derived_layout(infos)
                .expect("a scene state fits the channel limit of its dataset");
        }
        for (channel, source) in self.channels.iter_mut().zip(state.channels) {
            channel.enabled = source.enabled;
            channel.opacity = source.opacity;
            channel.window = source.window;
            channel.set_transfer(source.transfer);
            channel.set_volume(source.volume);
        }
        self.selected = state.selected;
        self.shading = state.shading;
        self.step_scale = state.step_scale;
        self.exposure = state.exposure;
        self.region = state.region;
        self.clip = state.clip;
        self.set_drifts(state.drifts);
        self.set_registration(state.registration);
        self.objects.restore(state.sets, &self.dataset);
        if derived {
            let specs = state
                .derived
                .iter()
                .map(|spec| spec.as_ref().clone())
                .collect::<Vec<_>>();
            self.bind_derived(specs)
                .expect("a scene state binds the derived channels of its objects");
        }
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
                set.document().raw(),
                state.mask_radius,
                self.mask_filter,
            )
        });
        self.coloc
            .restore(state.coloc, self.channels.len(), self.frame_count(), mask);
        self.set_time(state.time);
        self.select_many(
            state
                .selections
                .into_iter()
                .map(|selection| Selection {
                    set: ObjectSetId::new(selection.set),
                    item: match selection.item {
                        ItemState::Object(index) => Item::Object(index),
                        ItemState::Track(index) => Item::Track(index),
                    },
                })
                .collect(),
        );
        self.validate_selections();
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
            hash = mix(hash, u64::from(spec.intensity().min().to_bits()));
            hash = mix(hash, u64::from(spec.intensity().max().to_bits()));
            match spec.operation() {
                Operation::Mask { set } => hash = mix(hash, set.raw()),
                Operation::Restore(restore) => {
                    match restore.psf() {
                        grana_optics::Psf::Theoretical(params) => {
                            for value in [
                                params.numerical_aperture(),
                                params.wavelength(),
                                params.immersion(),
                                params.sample(),
                                params.depth(),
                            ] {
                                hash = mix(hash, u64::from(value.to_bits()));
                            }
                        }
                        grana_optics::Psf::Measured(psf) => {
                            for value in [psf.spacing().x(), psf.spacing().y(), psf.spacing().z()] {
                                hash = mix(hash, u64::from(value.to_bits()));
                            }
                            for value in psf.dims() {
                                hash = mix(hash, u64::from(value));
                            }
                            hash = mix(hash, u64::from(psf.layers()));
                            for kernel in psf.kernels() {
                                for value in kernel.values() {
                                    hash = mix(hash, u64::from(value.to_bits()));
                                }
                            }
                        }
                    }
                    hash = mix(hash, u64::from(restore.iterations()));
                }
                Operation::Smooth(smooth) => {
                    hash = mix(hash, u64::from(smooth.sigma().to_bits()));
                }
                Operation::Background(background) => {
                    hash = mix(hash, u64::from(background.radius().to_bits()));
                }
                Operation::Ratio(ratio) => {
                    hash = mix(hash, ratio.denominator() as u64);
                }
                Operation::Bleach => {}
            }
        }
        for channel in &self.channels {
            hash = mix(hash, u64::from(u8::from(channel.enabled)));
            hash = mix(hash, u64::from(channel.opacity.to_bits()));
            hash = mix(hash, u64::from(channel.window.min().to_bits()));
            hash = mix(hash, u64::from(channel.window.max().to_bits()));
            hash = mix(hash, channel.volume().mode().index() as u64);
            hash = mix(hash, u64::from(channel.volume().isovalue().to_bits()));
            for stop in channel.transfer().stops() {
                hash = mix(hash, u64::from(stop.position().to_bits()));
                hash = mix(hash, u64::from(stop.opacity().to_bits()));
                for component in stop.color() {
                    hash = mix(hash, u64::from(component.to_bits()));
                }
            }
        }
        hash = mix(hash, u64::from(self.step_scale.to_bits()));
        hash = mix(hash, u64::from(self.exposure.to_bits()));
        for value in [
            self.shading.ambient,
            self.shading.diffuse,
            self.shading.shadows,
            self.shading.azimuth,
            self.shading.elevation,
        ] {
            hash = mix(hash, u64::from(value.to_bits()));
        }
        for index in 0..3 {
            let axis = self.region.axis(index);
            for component in [axis.x, axis.y, axis.z, self.region.half()[index]] {
                hash = mix(hash, u64::from(component.to_bits()));
            }
        }
        for value in [
            self.region.center().x,
            self.region.center().y,
            self.region.center().z,
        ] {
            hash = mix(hash, u64::from(value.to_bits()));
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
            hash = mix(hash, u64::from(set.material().roughness().to_bits()));
            hash = mix(hash, u64::from(set.material().metallic().to_bits()));
            hash = mix(hash, set.kind().index() as u64);
            hash = mix(hash, set.document().raw());
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
        hash = mix(hash, self.drifts().digest());
        hash = mix(hash, self.registration().digest());
        hash = mix(hash, self.coloc.revision().raw());
        hash
    }

    fn validate_selections(&mut self) {
        let time = self.time();
        let objects = &self.objects;
        self.selections.retain(|selection| {
            objects
                .get(selection.set)
                .is_some_and(|set| match selection.item {
                    Item::Object(index) => set
                        .frame(time)
                        .is_some_and(|frame| index < frame.object_count()),
                    Item::Track(index) => {
                        set.tracks().is_some_and(|layer| index < layer.set().len())
                    }
                })
        });
    }
}

pub(crate) fn mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(HASH_PRIME)
}
