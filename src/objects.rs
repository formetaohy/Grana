use grana_analysis::{SpotParams, SurfaceParams, TrackParams};
use grana_object::{Mesh, SpotSet, SurfaceSet, TrackSet};
use grana_volume::ChannelInfo;

use crate::dataset::Dataset;
use crate::tracks::TrackLayer;

const SPOT_STEP: f32 = 8.0;
const SURFACE_STEP: f32 = 12.0;
const SPOT_THICKNESS: f32 = 1.0;
const SURFACE_THICKNESS: f32 = 2.0;
const MIN_THICKNESS: f32 = 0.25;
const MAX_THICKNESS: f32 = 40.0;
const MAX_GAP: u32 = 1;
const DIVISION_SCALE: f32 = 2.0;
const SPOT_DIAMETER: f32 = 4.0;
const SPOT_CONTRAST: f32 = 0.2;
const SURFACE_THRESHOLD: f32 = 0.5;
const SURFACE_MIN_VOLUME: f32 = 0.0;

const PALETTE: [[f32; 3]; 8] = [
    [0.35, 0.70, 1.00],
    [0.35, 0.90, 0.55],
    [1.00, 0.55, 0.35],
    [0.80, 0.55, 1.00],
    [1.00, 0.80, 0.35],
    [0.35, 0.90, 0.90],
    [1.00, 0.45, 0.70],
    [0.75, 0.85, 0.45],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectSetId(u64);

impl ObjectSetId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Spots,
    Surfaces,
}

impl ObjectKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Spots => "Spots",
            Self::Surfaces => "Surfaces",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Spots => 0,
            Self::Surfaces => 1,
        }
    }

    pub fn track_step(self) -> f32 {
        match self {
            Self::Spots => SPOT_STEP,
            Self::Surfaces => SURFACE_STEP,
        }
    }

    pub fn tube_thickness(self) -> f32 {
        match self {
            Self::Spots => SPOT_THICKNESS,
            Self::Surfaces => SURFACE_THICKNESS,
        }
    }

    pub fn default_params(self, channel: &ChannelInfo, voxel: f32) -> ObjectParams {
        match self {
            Self::Spots => {
                ObjectParams::Spots(SpotParams::new(SPOT_DIAMETER * voxel, SPOT_CONTRAST))
            }
            Self::Surfaces => ObjectParams::Surfaces(SurfaceParams::new(
                channel.intensity().value(SURFACE_THRESHOLD),
                SURFACE_MIN_VOLUME,
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObjectParams {
    Spots(SpotParams),
    Surfaces(SurfaceParams),
}

impl ObjectParams {
    pub fn kind(self) -> ObjectKind {
        match self {
            Self::Spots(_) => ObjectKind::Spots,
            Self::Surfaces(_) => ObjectKind::Surfaces,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectFrame {
    Spots(SpotSet),
    Surfaces(SurfaceSet),
}

impl ObjectFrame {
    pub fn kind(&self) -> ObjectKind {
        match self {
            Self::Spots(_) => ObjectKind::Spots,
            Self::Surfaces(_) => ObjectKind::Surfaces,
        }
    }

    pub fn level(&self) -> usize {
        match self {
            Self::Spots(set) => set.level(),
            Self::Surfaces(set) => set.level(),
        }
    }

    pub fn object_count(&self) -> usize {
        match self {
            Self::Spots(set) => set.len(),
            Self::Surfaces(set) => set.object_count(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.object_count() == 0
    }

    pub fn spots(&self) -> Option<&SpotSet> {
        match self {
            Self::Spots(set) => Some(set),
            Self::Surfaces(_) => None,
        }
    }

    pub fn surfaces(&self) -> Option<&SurfaceSet> {
        match self {
            Self::Surfaces(set) => Some(set),
            Self::Spots(_) => None,
        }
    }

    pub fn mesh(&self) -> Option<&Mesh> {
        match self {
            Self::Spots(_) => None,
            Self::Surfaces(set) => Some(set.mesh()),
        }
    }

    pub fn centroids(&self) -> Vec<[f32; 3]> {
        match self {
            Self::Spots(set) => set.spots().iter().map(|spot| spot.center()).collect(),
            Self::Surfaces(set) => set
                .objects()
                .iter()
                .map(|object| object.centroid())
                .collect(),
        }
    }
}

struct FrameSlot {
    stamp: u64,
    data: Option<ObjectFrame>,
}

impl FrameSlot {
    fn new() -> Self {
        Self {
            stamp: 0,
            data: None,
        }
    }
}

pub struct ObjectSet {
    id: ObjectSetId,
    kind: ObjectKind,
    name: String,
    channel: usize,
    color: [f32; 3],
    visible: bool,
    voxel: f32,
    params: ObjectParams,
    track_params: TrackParams,
    frames: Vec<FrameSlot>,
    stamp: u64,
    tracks: Option<TrackLayer>,
}

impl ObjectSet {
    fn new(
        id: ObjectSetId,
        kind: ObjectKind,
        name: String,
        channel: usize,
        color: [f32; 3],
        dataset: &Dataset,
    ) -> Self {
        let voxel = dataset.voxel_size().min_element();
        let params = kind.default_params(&dataset.channels()[channel], voxel);
        let step = kind.track_step() * voxel;
        let track_params = TrackParams::new(step, MAX_GAP, true, DIVISION_SCALE * step);
        Self {
            id,
            kind,
            name,
            channel,
            color,
            visible: true,
            voxel,
            params,
            track_params,
            frames: (0..dataset.frame_count())
                .map(|_| FrameSlot::new())
                .collect(),
            stamp: 1,
            tracks: None,
        }
    }

    pub fn id(&self) -> ObjectSetId {
        self.id
    }

    pub fn kind(&self) -> ObjectKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn color(&self) -> [f32; 3] {
        self.color
    }

    pub fn set_color(&mut self, color: [f32; 3]) {
        assert!(
            color.iter().all(|value| (0.0..=1.0).contains(value)),
            "an object color is an srgb triple"
        );
        self.color = color;
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn params(&self) -> ObjectParams {
        self.params
    }

    pub fn set_params(&mut self, params: ObjectParams) {
        assert_eq!(
            params.kind(),
            self.kind,
            "the parameters match the object kind"
        );
        self.params = params;
    }

    pub fn frame(&self, time: u32) -> Option<&ObjectFrame> {
        self.frames
            .get(time as usize)
            .and_then(|slot| slot.data.as_ref())
    }

    pub fn frame_stamp(&self, time: u32) -> Option<u64> {
        self.frames
            .get(time as usize)
            .and_then(|slot| slot.data.as_ref().map(|_| slot.stamp))
    }

    pub fn frames(&self) -> impl Iterator<Item = (u32, &ObjectFrame)> + '_ {
        self.frames
            .iter()
            .enumerate()
            .filter_map(|(time, slot)| slot.data.as_ref().map(|data| (time as u32, data)))
    }

    pub fn detected(&self) -> usize {
        self.frames
            .iter()
            .filter(|slot| slot.data.is_some())
            .count()
    }

    pub fn install(&mut self, time: u32, frame: ObjectFrame) {
        assert_eq!(
            frame.kind(),
            self.kind,
            "an object frame matches its object set"
        );
        let slot = self
            .frames
            .get_mut(time as usize)
            .expect("the object frame lies inside the time series");
        self.stamp += 1;
        slot.stamp = self.stamp;
        slot.data = Some(frame);
    }

    pub fn clear(&mut self) {
        for slot in &mut self.frames {
            slot.data = None;
        }
        self.tracks = None;
    }

    pub fn tracks(&self) -> Option<&TrackLayer> {
        self.tracks.as_ref()
    }

    pub fn track_params(&self) -> TrackParams {
        self.track_params
    }

    pub fn set_track_params(&mut self, params: TrackParams) {
        self.track_params = params;
    }

    pub fn install_tracks(&mut self, set: TrackSet) {
        let visible = self.tracks.as_ref().is_none_or(TrackLayer::visible);
        let thickness = self
            .tracks
            .as_ref()
            .map_or_else(|| self.default_thickness(), TrackLayer::thickness);
        let mut layer = TrackLayer::new(set, thickness);
        layer.set_visible(visible);
        self.tracks = Some(layer);
    }

    pub fn clear_tracks(&mut self) {
        self.tracks = None;
    }

    pub fn min_thickness(&self) -> f32 {
        MIN_THICKNESS * self.voxel
    }

    pub fn max_thickness(&self) -> f32 {
        MAX_THICKNESS * self.voxel
    }

    pub fn set_track_thickness(&mut self, thickness: f32) {
        let thickness = thickness.clamp(self.min_thickness(), self.max_thickness());
        if let Some(layer) = self.tracks.as_mut() {
            layer.set_thickness(thickness);
        }
    }

    pub fn set_track_visible(&mut self, visible: bool) {
        if let Some(layer) = self.tracks.as_mut() {
            layer.set_visible(visible);
        }
    }

    pub fn track_at(&self, time: u32, object: usize) -> Option<usize> {
        self.tracks.as_ref()?.set().track_at(time, object)
    }

    fn default_thickness(&self) -> f32 {
        self.kind.tube_thickness() * self.voxel
    }
}

pub struct Objects {
    sets: Vec<ObjectSet>,
    next_id: u64,
    ordinals: [u64; 2],
}

impl Objects {
    pub fn new() -> Self {
        Self {
            sets: Vec::new(),
            next_id: 1,
            ordinals: [0; 2],
        }
    }

    pub fn add(&mut self, kind: ObjectKind, channel: usize, dataset: &Dataset) -> ObjectSetId {
        assert!(
            channel < dataset.channels().len(),
            "every object set names an existing channel"
        );
        let id = ObjectSetId(self.next_id);
        self.next_id += 1;
        let ordinal = self.ordinals[kind.index()] + 1;
        self.ordinals[kind.index()] = ordinal;
        let color = PALETTE[(id.raw() as usize - 1) % PALETTE.len()];
        let name = format!("{} {ordinal}", kind.name());
        self.sets
            .push(ObjectSet::new(id, kind, name, channel, color, dataset));
        id
    }

    pub fn remove(&mut self, id: ObjectSetId) {
        self.sets.retain(|set| set.id() != id);
    }

    pub fn get(&self, id: ObjectSetId) -> Option<&ObjectSet> {
        self.sets.iter().find(|set| set.id() == id)
    }

    pub fn get_mut(&mut self, id: ObjectSetId) -> Option<&mut ObjectSet> {
        self.sets.iter_mut().find(|set| set.id() == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ObjectSet> + '_ {
        self.sets.iter()
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }
}
