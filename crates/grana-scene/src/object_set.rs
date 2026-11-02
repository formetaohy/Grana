use std::sync::Arc;

use grana_analysis::TrackParams;
use grana_measure::{Measurement, TrackMeasurement};
use grana_object::TrackSet;

use crate::dataset::Dataset;
use crate::object::{ObjectFrame, ObjectKind, ObjectParams, TrackStyle};
use crate::state::SetState;
use crate::track_layer::TrackLayer;
use grana_display::{ColorState, Coloring, Extent, merged};

const MIN_THICKNESS: f32 = 0.25;
const MAX_THICKNESS: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectSetId(u64);

impl ObjectSetId {
    pub(crate) fn new(raw: u64) -> Self {
        assert!(raw > 0, "an object set id is positive");
        Self(raw)
    }

    pub fn from_raw(raw: u64) -> Option<Self> {
        (raw > 0).then_some(Self(raw))
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Visibility {
    All,
    Object(Arc<Vec<Vec<bool>>>),
    Track(Arc<Vec<bool>>),
}

struct FrameSlot {
    stamp: u64,
    data: Option<Arc<ObjectFrame>>,
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
    channels: usize,
    color: [f32; 3],
    visible: bool,
    voxel: f32,
    params: ObjectParams,
    track_style: Option<TrackStyle>,
    frames: Vec<FrameSlot>,
    frame_bounds: Vec<Option<Extent>>,
    object_coloring: Coloring<Measurement>,
    track_coloring: Coloring<TrackMeasurement>,
    track_bounds: Option<Extent>,
    revision: u64,
    document: u64,
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
        let params = kind.default_params(voxel);
        let track_style = kind.track_style(voxel);
        let frames = (0..dataset.frame_count())
            .map(|_| FrameSlot::new())
            .collect::<Vec<_>>();
        Self {
            id,
            kind,
            name,
            channel,
            channels: dataset.channels().len(),
            color,
            visible: true,
            voxel,
            params,
            track_style,
            frame_bounds: vec![None; frames.len()],
            frames,
            object_coloring: Coloring::new(),
            track_coloring: Coloring::new(),
            track_bounds: None,
            revision: 1,
            document: 1,
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
        if self.color == color {
            return;
        }
        self.color = color;
        self.document += 1;
    }

    pub fn object_coloring(&self) -> &Coloring<Measurement> {
        &self.object_coloring
    }

    pub fn colorable_measurements(&self) -> Vec<Measurement> {
        self.kind
            .measurements(self.channels)
            .into_iter()
            .filter(|measurement| measurement.colorable())
            .collect()
    }

    pub fn set_object_coloring(&mut self, coloring: Coloring<Measurement>) -> bool {
        if let Some(metric) = coloring.metric() {
            assert!(
                self.colorable_measurements().contains(&metric),
                "the measurement colors the objects of this set"
            );
        }
        if self.object_coloring == coloring {
            return false;
        }
        let metric_changed = self.object_coloring.metric() != coloring.metric();
        self.object_coloring = coloring;
        if metric_changed {
            self.rebuild_frame_bounds();
        }
        self.document += 1;
        true
    }

    pub fn object_extent(&self) -> Option<Extent> {
        self.object_coloring.metric()?;
        self.object_coloring
            .manual()
            .or_else(|| merged(self.frame_bounds.iter().flatten().copied()))
    }

    pub fn track_coloring(&self) -> &Coloring<TrackMeasurement> {
        &self.track_coloring
    }

    pub fn colorable_track_measurements(&self) -> Vec<TrackMeasurement> {
        TrackMeasurement::ALL
            .into_iter()
            .filter(|measurement| measurement.colorable())
            .collect()
    }

    pub fn set_track_coloring(&mut self, coloring: Coloring<TrackMeasurement>) -> bool {
        if let Some(metric) = coloring.metric() {
            assert!(
                self.kind.trackable(),
                "only trackable objects carry track colors"
            );
            assert!(
                metric.colorable(),
                "an identity column does not color tracks"
            );
        }
        if self.track_coloring == coloring {
            return false;
        }
        let metric_changed = self.track_coloring.metric() != coloring.metric();
        self.track_coloring = coloring;
        if metric_changed {
            self.rebuild_track_bounds();
        }
        self.document += 1;
        true
    }

    pub fn track_extent(&self) -> Option<Extent> {
        self.track_coloring.metric()?;
        self.track_coloring.manual().or(self.track_bounds)
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        self.document += 1;
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
        if self.params == params {
            return;
        }
        self.params = params;
        self.document += 1;
    }

    pub fn frame(&self, time: u32) -> Option<&ObjectFrame> {
        self.frames
            .get(time as usize)
            .and_then(|slot| slot.data.as_deref())
    }

    pub fn frame_shared(&self, time: u32) -> Option<Arc<ObjectFrame>> {
        self.frames
            .get(time as usize)
            .and_then(|slot| slot.data.clone())
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
            .filter_map(|(time, slot)| slot.data.as_deref().map(|data| (time as u32, data)))
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

    pub fn document(&self) -> u64 {
        self.document
    }

    pub(crate) fn state(&self) -> SetState {
        SetState {
            id: self.id.raw(),
            kind: self.kind,
            name: self.name.clone(),
            channel: self.channel,
            color: self.color,
            visible: self.visible,
            params: self.params,
            track_style: self.track_style,
            tracks: self.tracks.as_ref().map(TrackLayer::shared_set),
            track_thickness: self.tracks.as_ref().map(TrackLayer::thickness),
            track_visible: self.tracks.as_ref().map(TrackLayer::visible),
            object_color: self.object_coloring.state(),
            track_color: self.track_coloring.state(),
            visibility: self.visibility.clone(),
            frames: self.frames.iter().map(|slot| slot.data.clone()).collect(),
            document: self.document,
        }
    }

    pub(crate) fn restore(state: SetState, dataset: &Dataset, stamp: &mut u64) -> Self {
        let SetState {
            id,
            kind,
            name,
            channel,
            color,
            visible,
            params,
            track_style,
            tracks,
            track_thickness,
            track_visible,
            object_color,
            track_color,
            visibility,
            frames,
            document,
        } = state;
        assert_eq!(
            params.kind(),
            kind,
            "an object set state names parameters of its own kind"
        );
        assert_eq!(
            kind.trackable(),
            track_style.is_some(),
            "only trackable object sets carry track parameters"
        );
        assert_eq!(
            tracks.is_some(),
            track_thickness.is_some() && track_visible.is_some(),
            "a restored track layer carries its thickness and visibility"
        );
        let frame_count = frames.len();
        let mut set = Self {
            id: ObjectSetId::new(id),
            kind,
            name,
            channel,
            channels: dataset.channels().len(),
            color,
            visible,
            voxel: dataset.voxel_size().min_element(),
            params,
            track_style,
            frame_bounds: vec![None; frame_count],
            frames: (0..frame_count).map(|_| FrameSlot::new()).collect(),
            object_coloring: Coloring::new(),
            track_coloring: Coloring::new(),
            track_bounds: None,
            revision: 1,
            document: 1,
            visibility: Visibility::All,
            tracks: None,
        };
        assert_eq!(
            set.frames.len(),
            dataset.frame_count() as usize,
            "an object set state covers every frame of the dataset"
        );
        for (time, frame) in frames.into_iter().enumerate() {
            if let Some(frame) = frame {
                set.install_shared(time as u32, frame, take_stamp(stamp));
            }
        }
        if let Some(tracks) = tracks {
            let thickness = track_thickness.expect("a restored track layer carries a thickness");
            let visible = track_visible.expect("a restored track layer carries visibility");
            let mut layer = TrackLayer::shared(tracks, thickness, take_stamp(stamp));
            layer.set_visible(visible);
            set.tracks = Some(layer);
        }
        set.restore_coloring(object_color, track_color);
        set.set_visibility(visibility);
        set.document = document;
        set
    }

    fn restore_coloring(
        &mut self,
        object: ColorState<Measurement>,
        track: ColorState<TrackMeasurement>,
    ) {
        let object = Coloring::restore(object);
        if let Some(metric) = object.metric() {
            assert!(
                self.colorable_measurements().contains(&metric),
                "a restored measurement colors the objects of its set"
            );
        }
        self.object_coloring = object;
        self.rebuild_frame_bounds();
        let track = Coloring::restore(track);
        if let Some(metric) = track.metric() {
            assert!(
                self.kind.trackable(),
                "only trackable objects carry track colors"
            );
            assert!(metric.colorable(), "a restored measurement colors tracks");
        }
        self.track_coloring = track;
        self.rebuild_track_bounds();
    }

    pub fn set_visibility(&mut self, visibility: Visibility) {
        if self.visibility == visibility {
            return;
        }
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
                        slot.data.as_deref().map_or(0, ObjectFrame::object_count),
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
        self.document += 1;
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

    pub(crate) fn install(&mut self, time: u32, frame: ObjectFrame, stamp: u64) {
        self.install_shared(time, Arc::new(frame), stamp);
    }

    fn install_shared(&mut self, time: u32, frame: Arc<ObjectFrame>, stamp: u64) {
        assert_eq!(
            frame.kind(),
            self.kind,
            "an object frame matches its object set"
        );
        let bounds = self
            .object_coloring
            .metric()
            .and_then(|metric| frame_extent(&frame, metric));
        let slot = self
            .frames
            .get_mut(time as usize)
            .expect("the object frame lies inside the time series");
        self.revision = stamp;
        self.document += 1;
        self.visibility = Visibility::All;
        slot.stamp = stamp;
        slot.data = Some(frame);
        self.frame_bounds[time as usize] = bounds;
    }

    pub(crate) fn clear(&mut self, stamp: u64) {
        for slot in &mut self.frames {
            slot.data = None;
        }
        self.frame_bounds.fill(None);
        self.track_bounds = None;
        self.revision = stamp;
        self.document += 1;
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
        if let Some(style) = self.track_style
            && style.params() != params
        {
            self.track_style = Some(TrackStyle::new(params, style.thickness()));
            self.document += 1;
        }
    }

    pub(crate) fn install_tracks(&mut self, set: TrackSet, stamp: u64) {
        let style = self
            .track_style
            .expect("only trackable objects install tracks");
        let visible = self.tracks.as_ref().is_none_or(TrackLayer::visible);
        let thickness = self
            .tracks
            .as_ref()
            .map_or(style.thickness(), TrackLayer::thickness);
        let bounds = self
            .track_coloring
            .metric()
            .and_then(|metric| track_extent(&set, metric));
        let mut layer = TrackLayer::new(set, thickness, stamp);
        layer.set_visible(visible);
        self.tracks = Some(layer);
        self.track_bounds = bounds;
        self.revision = stamp;
        self.document += 1;
        self.visibility = Visibility::All;
    }

    pub(crate) fn clear_tracks(&mut self, stamp: u64) {
        self.revision = stamp;
        self.document += 1;
        self.visibility = Visibility::All;
        self.tracks = None;
        self.track_bounds = None;
    }

    pub(crate) fn cut_track(&mut self, time: u32, object: usize, stamp: u64) -> bool {
        let Some(layer) = self.tracks.as_mut() else {
            return false;
        };
        if !layer.cut(time, object, stamp) {
            return false;
        }
        self.revision = stamp;
        self.document += 1;
        self.visibility = Visibility::All;
        self.rebuild_track_bounds();
        true
    }

    pub(crate) fn join_tracks(
        &mut self,
        source: (u32, usize),
        target: (u32, usize),
        stamp: u64,
    ) -> bool {
        let Some(layer) = self.tracks.as_mut() else {
            return false;
        };
        if !layer.join(source, target, stamp) {
            return false;
        }
        self.revision = stamp;
        self.document += 1;
        self.visibility = Visibility::All;
        self.rebuild_track_bounds();
        true
    }

    fn rebuild_frame_bounds(&mut self) {
        let metric = self.object_coloring.metric();
        for (slot, bounds) in self.frames.iter().zip(self.frame_bounds.iter_mut()) {
            *bounds = slot
                .data
                .as_deref()
                .and_then(|frame| metric.and_then(|metric| frame_extent(frame, metric)));
        }
    }

    fn rebuild_track_bounds(&mut self) {
        self.track_bounds = self.track_coloring.metric().and_then(|metric| {
            self.tracks
                .as_ref()
                .and_then(|layer| track_extent(layer.set(), metric))
        });
    }

    pub fn min_thickness(&self) -> f32 {
        MIN_THICKNESS * self.voxel
    }

    pub fn max_thickness(&self) -> f32 {
        MAX_THICKNESS * self.voxel
    }

    pub(crate) fn set_track_thickness(&mut self, thickness: f32, stamp: u64) {
        let thickness = thickness.clamp(self.min_thickness(), self.max_thickness());
        if let Some(layer) = self.tracks.as_mut() {
            let previous = layer.thickness();
            layer.set_thickness(thickness, stamp);
            if layer.thickness() != previous {
                self.document += 1;
                self.revision = stamp;
            }
        }
    }

    pub fn set_track_visible(&mut self, visible: bool) {
        if let Some(layer) = self.tracks.as_mut() {
            let previous = layer.visible();
            layer.set_visible(visible);
            if layer.visible() != previous {
                self.document += 1;
            }
        }
    }

    pub fn track_at(&self, time: u32, object: usize) -> Option<usize> {
        self.tracks.as_ref()?.set().track_at(time, object)
    }
}

fn frame_extent(frame: &ObjectFrame, metric: Measurement) -> Option<Extent> {
    let objects = frame.objects();
    (0..frame.object_count()).fold(None, |accumulated, object| {
        accumulate(accumulated, metric.value(objects, object))
    })
}

fn track_extent(set: &TrackSet, metric: TrackMeasurement) -> Option<Extent> {
    (0..set.len()).fold(None, |accumulated, track| {
        accumulate(accumulated, metric.value(track, set))
    })
}

fn accumulate(accumulated: Option<Extent>, value: f32) -> Option<Extent> {
    if !value.is_finite() {
        return accumulated;
    }
    Some(match accumulated {
        None => (value, value),
        Some((minimum, maximum)) => (minimum.min(value), maximum.max(value)),
    })
}

pub(crate) fn take_stamp(stamp: &mut u64) -> u64 {
    *stamp += 1;
    *stamp
}
