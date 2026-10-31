use grana_analysis::TrackParams;
use grana_object::TrackSet;

use crate::dataset::Dataset;
use crate::object::{ObjectFrame, ObjectKind, ObjectParams, TrackStyle};
use crate::track_layer::TrackLayer;

const MIN_THICKNESS: f32 = 0.25;
const MAX_THICKNESS: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectSetId(u64);

impl ObjectSetId {
    pub(crate) fn new(raw: u64) -> Self {
        assert!(raw > 0, "an object set id is positive");
        Self(raw)
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Visibility {
    All,
    Object(Vec<Vec<bool>>),
    Track(Vec<bool>),
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
    track_style: Option<TrackStyle>,
    frames: Vec<FrameSlot>,
    stamp: u64,
    revision: u64,
    visibility: Visibility,
    tracks: Option<TrackLayer>,
}

impl ObjectSet {
    pub(crate) fn new(
        id: ObjectSetId,
        kind: ObjectKind,
        name: String,
        channel: usize,
        color: [f32; 3],
        dataset: &Dataset,
    ) -> Self {
        let voxel = dataset.voxel_size().min_element();
        let params = kind.default_params(&dataset.channels()[channel], voxel);
        let track_style = kind.track_style(voxel);
        Self {
            id,
            kind,
            name,
            channel,
            color,
            visible: true,
            voxel,
            params,
            track_style,
            frames: (0..dataset.frame_count())
                .map(|_| FrameSlot::new())
                .collect(),
            stamp: 1,
            revision: 1,
            visibility: Visibility::All,
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

    pub fn frame_count(&self) -> u32 {
        self.frames.len() as u32
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn set_visibility(&mut self, visibility: Visibility) {
        match &visibility {
            Visibility::All => {}
            Visibility::Object(frames) => {
                assert_eq!(
                    frames.len(),
                    self.frames.len(),
                    "the visibility covers every frame"
                );
                for (mask, slot) in frames.iter().zip(&self.frames) {
                    assert_eq!(
                        mask.len(),
                        slot.data.as_ref().map_or(0, ObjectFrame::object_count),
                        "the visibility covers every object"
                    );
                }
            }
            Visibility::Track(tracks) => {
                assert_eq!(
                    tracks.len(),
                    self.tracks.as_ref().map_or(0, |layer| layer.set().len()),
                    "the visibility covers every track"
                );
            }
        }
        self.visibility = visibility;
    }

    pub fn visible_object(&self, time: u32, object: usize) -> bool {
        match &self.visibility {
            Visibility::All => true,
            Visibility::Object(frames) => frames
                .get(time as usize)
                .and_then(|mask| mask.get(object))
                .copied()
                .unwrap_or(false),
            Visibility::Track(tracks) => self
                .track_at(time, object)
                .is_some_and(|track| tracks.get(track).copied().unwrap_or(false)),
        }
    }

    pub fn visible_track(&self, track: usize) -> bool {
        match &self.visibility {
            Visibility::All | Visibility::Object(_) => true,
            Visibility::Track(tracks) => tracks.get(track).copied().unwrap_or(false),
        }
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
        self.revision += 1;
        self.visibility = Visibility::All;
        slot.stamp = self.stamp;
        slot.data = Some(frame);
    }

    pub fn clear(&mut self) {
        for slot in &mut self.frames {
            slot.data = None;
        }
        self.revision += 1;
        self.visibility = Visibility::All;
        self.tracks = None;
    }

    pub fn tracks(&self) -> Option<&TrackLayer> {
        self.tracks.as_ref()
    }

    pub fn track_style(&self) -> Option<TrackStyle> {
        self.track_style
    }

    pub fn set_track_params(&mut self, params: TrackParams) {
        if let Some(style) = self.track_style {
            self.track_style = Some(TrackStyle::new(params, style.thickness()));
        }
    }

    pub fn install_tracks(&mut self, set: TrackSet) {
        let style = self
            .track_style
            .expect("only trackable objects install tracks");
        let visible = self.tracks.as_ref().is_none_or(TrackLayer::visible);
        let thickness = self
            .tracks
            .as_ref()
            .map_or(style.thickness(), TrackLayer::thickness);
        let mut layer = TrackLayer::new(set, thickness);
        layer.set_visible(visible);
        self.tracks = Some(layer);
        self.revision += 1;
        self.visibility = Visibility::All;
    }

    pub fn clear_tracks(&mut self) {
        self.revision += 1;
        self.visibility = Visibility::All;
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
}
