use std::sync::Arc;

use grana_measure::{Measurement, TrackMeasurement};
use grana_object::TrackParams;
use grana_object::{RelationFrame, TrackSet};

use crate::color::ObjectColor;
use crate::curation::{Curation, TrackAnchor, TrackEdit};
use crate::dataset::Dataset;
use crate::object::{ObjectFrame, ObjectKind, ObjectParams, TrackStyle};
use crate::provenance::{Provenance, Recipe};
use crate::relation::Relation;
use crate::seed::Seeds;
use crate::state::{RelationState, SetState};
use crate::track_color::TrackColor;
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
    opacity: f32,
    voxel: f32,
    params: ObjectParams,
    track_style: Option<TrackStyle>,
    frames: Vec<FrameSlot>,
    frame_bounds: Vec<Option<Extent>>,
    object_coloring: Coloring<ObjectColor>,
    track_coloring: Coloring<TrackColor>,
    track_bounds: Option<Extent>,
    revision: u64,
    document: u64,
    visibility: Visibility,
    seeds: Seeds,
    tracks: Option<TrackLayer>,
    relations: Vec<Relation>,
    provenance: Provenance,
    curation: Curation,
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
            opacity: 1.0,
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
            seeds: Seeds::points(dataset.frame_count() as usize),
            tracks: None,
            relations: Vec::new(),
            provenance: Provenance::detection(),
            curation: Curation::default(),
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

    pub fn set_name(&mut self, name: &str) -> bool {
        let name = name.trim();
        assert!(!name.is_empty(), "an object set carries a name");
        if self.name == name {
            return false;
        }
        self.name = name.to_string();
        self.document += 1;
        true
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub(crate) fn set_provenance(&mut self, provenance: Provenance) {
        if self.provenance == provenance {
            return;
        }
        self.provenance = provenance;
        self.document += 1;
    }

    pub(crate) fn recipe(&self) -> Recipe {
        Recipe::new(
            self.name.clone(),
            self.channel,
            self.params,
            self.seeds.clone(),
            self.curation.clone(),
            self.provenance.clone(),
        )
    }

    pub fn seed_source(&self) -> Option<ObjectSetId> {
        self.seeds.source()
    }

    pub fn surface_seeds(&self) -> &Seeds {
        &self.seeds
    }

    pub(crate) fn set_seed_source(&mut self, source: Option<ObjectSetId>, frames: usize) -> bool {
        assert_eq!(
            self.kind,
            ObjectKind::Surfaces,
            "seed points cut a surface detection"
        );
        let seeds = match source {
            Some(source) => {
                assert_ne!(
                    source, self.id,
                    "an object set seeds from another object set"
                );
                Seeds::Source(source)
            }
            None => Seeds::points(frames),
        };
        if self.seeds == seeds {
            return false;
        }
        self.seeds = seeds;
        self.document += 1;
        true
    }

    pub(crate) fn forget_copies(&mut self, removed: &[ObjectSetId]) {
        let mut provenance = self.provenance.clone();
        provenance.forget_copied(removed);
        self.set_provenance(provenance);
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

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    pub fn set_opacity(&mut self, opacity: f32) {
        let opacity = opacity.clamp(0.0, 1.0);
        if self.opacity == opacity {
            return;
        }
        self.opacity = opacity;
        self.document += 1;
    }

    pub fn object_coloring(&self) -> &Coloring<ObjectColor> {
        &self.object_coloring
    }

    pub fn colorable_measurements(&self) -> Vec<Measurement> {
        self.kind
            .measurements(self.channels)
            .into_iter()
            .filter(|measurement| measurement.colorable())
            .collect()
    }

    pub fn set_object_coloring(&mut self, coloring: Coloring<ObjectColor>) -> bool {
        self.check_coloring(coloring.metric());
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

    fn check_coloring(&self, metric: Option<ObjectColor>) {
        match metric {
            None => {}
            Some(ObjectColor::Measurement(measurement)) => assert!(
                self.colorable_measurements().contains(&measurement),
                "the measurement colors the objects of this set"
            ),
            Some(ObjectColor::Relation {
                target,
                field,
                shift,
            }) => {
                assert!(
                    self.relation(target, shift).is_some(),
                    "the relation colors the objects of this set"
                );
                assert!(
                    crate::color::RELATION_FIELDS.contains(&field),
                    "a relation field colors the objects of this set"
                );
            }
        }
    }

    pub fn object_extent(&self) -> Option<Extent> {
        self.object_coloring.metric()?;
        self.object_coloring
            .manual()
            .or_else(|| merged(self.frame_bounds.iter().flatten().copied()))
    }

    pub fn object_color(&self, time: u32, frame: &ObjectFrame, object: usize) -> [f32; 3] {
        let Some(metric) = self.object_coloring.metric() else {
            return self.color;
        };
        let Some(extent) = self.object_extent() else {
            return self.color;
        };
        let relation = metric
            .relation()
            .and_then(|(target, shift)| self.relation(target, shift))
            .and_then(|relation| relation.frame(time));
        let value = metric.value(frame.objects(), relation, object);
        if value.is_finite() {
            self.object_coloring.color(extent, value)
        } else {
            self.color
        }
    }

    pub fn track_coloring(&self) -> &Coloring<TrackColor> {
        &self.track_coloring
    }

    pub fn colorable_track_measurements(&self) -> Vec<TrackMeasurement> {
        TrackMeasurement::ALL
            .into_iter()
            .filter(|measurement| measurement.colorable())
            .collect()
    }

    pub fn set_track_coloring(&mut self, coloring: Coloring<TrackColor>) -> bool {
        match coloring.metric() {
            None => {}
            Some(TrackColor::Measurement(measurement)) => {
                assert!(
                    self.kind.trackable(),
                    "only trackable objects carry track colors"
                );
                assert!(
                    measurement.colorable(),
                    "an identity column does not color tracks"
                );
            }
            Some(TrackColor::Relation {
                target,
                field,
                shift,
                ..
            }) => {
                assert!(
                    self.kind.trackable(),
                    "only trackable objects carry track colors"
                );
                assert!(
                    self.relation(target, shift).is_some(),
                    "the relation colors the tracks of this set"
                );
                assert!(
                    crate::color::RELATION_FIELDS.contains(&field),
                    "a relation field colors the tracks of this set"
                );
            }
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

    pub fn track_color(&self, track: usize) -> [f32; 3] {
        let Some(metric) = self.track_coloring.metric() else {
            return self.color;
        };
        let Some(extent) = self.track_extent() else {
            return self.color;
        };
        let value = self.track_value(track, metric);
        if value.is_finite() {
            self.track_coloring.color(extent, value)
        } else {
            self.color
        }
    }

    fn track_value(&self, track: usize, metric: TrackColor) -> f32 {
        let Some(layer) = self.tracks.as_ref() else {
            return f32::NAN;
        };
        let relation = metric
            .relation()
            .and_then(|(target, shift)| self.relation(target, shift));
        metric.value(track, layer.set(), relation)
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

    pub fn relations(&self) -> &[Relation] {
        &self.relations
    }

    pub fn relation(&self, target: ObjectSetId, shift: i32) -> Option<&Relation> {
        self.relations
            .iter()
            .find(|relation| relation.target() == target && relation.shift() == shift)
    }

    pub(crate) fn add_relation(&mut self, target: ObjectSetId, shift: i32, stamp: u64) -> bool {
        if self.relation(target, shift).is_some() {
            return false;
        }
        self.relations.push(Relation::new(
            target,
            shift,
            vec![None; self.frame_count() as usize],
        ));
        self.revision = stamp;
        self.document += 1;
        true
    }

    pub(crate) fn move_relation(
        &mut self,
        target: ObjectSetId,
        from: i32,
        to: i32,
        stamp: u64,
    ) -> bool {
        if self.relation(target, to).is_some() {
            return false;
        }
        let Some(index) = self
            .relations
            .iter()
            .position(|relation| relation.target() == target && relation.shift() == from)
        else {
            return false;
        };
        let moved = self.relations[index].moved(to);
        self.relations[index] = moved;
        if let Some(ObjectColor::Relation {
            target: current,
            field,
            shift,
        }) = self.object_coloring.metric()
            && current == target
            && shift == from
        {
            self.object_coloring.set_metric(Some(ObjectColor::Relation {
                target,
                field,
                shift: to,
            }));
            self.rebuild_frame_bounds();
        }
        if let Some(TrackColor::Relation {
            target: current,
            field,
            aggregate,
            shift,
        }) = self.track_coloring.metric()
            && current == target
            && shift == from
        {
            self.track_coloring.set_metric(Some(TrackColor::Relation {
                target,
                field,
                aggregate,
                shift: to,
            }));
            self.rebuild_track_bounds();
        }
        self.revision = stamp;
        self.document += 1;
        true
    }

    pub(crate) fn remove_relation(&mut self, target: ObjectSetId, shift: i32, stamp: u64) -> bool {
        let Some(index) = self
            .relations
            .iter()
            .position(|relation| relation.target() == target && relation.shift() == shift)
        else {
            return false;
        };
        self.relations.remove(index);
        if self
            .object_coloring
            .metric()
            .and_then(ObjectColor::relation)
            == Some((target, shift))
        {
            self.object_coloring.set_metric(None);
            self.rebuild_frame_bounds();
        }
        if self.track_coloring.metric().and_then(TrackColor::relation) == Some((target, shift)) {
            self.track_coloring.set_metric(None);
            self.rebuild_track_bounds();
        }
        self.revision = stamp;
        self.document += 1;
        true
    }

    pub(crate) fn remove_relations_to(&mut self, target: ObjectSetId, stamp: u64) -> bool {
        let count = self.relations.len();
        self.relations
            .retain(|relation| relation.target() != target);
        if self.relations.len() == count {
            return false;
        }
        if self.object_coloring.metric().and_then(ObjectColor::target) == Some(target) {
            self.object_coloring.set_metric(None);
            self.rebuild_frame_bounds();
        }
        if self.track_coloring.metric().and_then(TrackColor::target) == Some(target) {
            self.track_coloring.set_metric(None);
            self.rebuild_track_bounds();
        }
        self.revision = stamp;
        self.document += 1;
        true
    }

    pub(crate) fn install_relation(
        &mut self,
        target: ObjectSetId,
        shift: i32,
        time: u32,
        frame: RelationFrame,
        stamp: u64,
    ) {
        let Some(relation) = self
            .relations
            .iter_mut()
            .find(|relation| relation.target() == target && relation.shift() == shift)
        else {
            return;
        };
        if !relation.install(time, frame) {
            return;
        }
        self.refresh_relation_bounds(target, time);
        self.revision = stamp;
        self.document += 1;
    }

    pub(crate) fn clear_relations(&mut self, stamp: u64) {
        if self.relations.is_empty() {
            return;
        }
        self.relations.clear();
        if self
            .object_coloring
            .metric()
            .and_then(ObjectColor::relation)
            .is_some()
        {
            self.object_coloring.set_metric(None);
            self.rebuild_frame_bounds();
        }
        if self
            .track_coloring
            .metric()
            .and_then(TrackColor::relation)
            .is_some()
        {
            self.track_coloring.set_metric(None);
            self.rebuild_track_bounds();
        }
        self.revision = stamp;
        self.document += 1;
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
            opacity: self.opacity,
            params: self.params,
            track_style: self.track_style,
            tracks: self.tracks.as_ref().map(TrackLayer::shared_set),
            track_thickness: self.tracks.as_ref().map(TrackLayer::thickness),
            track_visible: self.tracks.as_ref().map(TrackLayer::visible),
            object_color: self.object_coloring.state(),
            track_color: self.track_coloring.state(),
            visibility: self.visibility.clone(),
            seeds: self.seeds.clone(),
            frames: self.frames.iter().map(|slot| slot.data.clone()).collect(),
            relations: self
                .relations
                .iter()
                .map(|relation| RelationState {
                    target: relation.target().raw(),
                    shift: relation.shift(),
                    frames: relation.shared_frames().to_vec(),
                })
                .collect(),
            provenance: self.provenance.clone(),
            curation: self.curation.clone(),
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
            opacity,
            params,
            track_style,
            tracks,
            track_thickness,
            track_visible,
            object_color,
            track_color,
            visibility,
            seeds,
            frames,
            relations,
            provenance,
            curation,
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
        let applied = curation.applied();
        let mut set = Self {
            id: ObjectSetId::new(id),
            kind,
            name,
            channel,
            channels: dataset.channels().len(),
            color,
            visible,
            opacity,
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
            seeds: Seeds::points(frame_count),
            tracks: None,
            relations: Vec::new(),
            provenance,
            curation,
        };
        match &seeds {
            Seeds::Points(frames) => {
                assert_eq!(
                    frames.len(),
                    frame_count,
                    "an object set state covers the seeds of every frame"
                );
                assert!(
                    frames
                        .iter()
                        .flatten()
                        .all(|seed| seed.iter().all(|value| value.is_finite())),
                    "a restored seed point is finite"
                );
                assert!(
                    kind == ObjectKind::Surfaces || frames.iter().all(Vec::is_empty),
                    "only a surface set carries seed points"
                );
            }
            Seeds::Source(source) => {
                assert_eq!(
                    kind,
                    ObjectKind::Surfaces,
                    "only a surface set seeds from another object set"
                );
                assert!(
                    source.raw() > 0 && *source != ObjectSetId::new(id),
                    "a restored surface seeds from another object set"
                );
            }
        }
        set.seeds = seeds;
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
        if applied {
            set.curation.set_applied();
        }
        for relation in relations {
            let target = ObjectSetId::new(relation.target);
            assert_eq!(
                relation.frames.len(),
                frame_count,
                "a restored relation covers every frame of its set"
            );
            assert!(
                set.relation(target, relation.shift).is_none(),
                "a restored relation measures one offset of its target"
            );
            set.relations
                .push(Relation::new(target, relation.shift, relation.frames));
        }
        if let Some(tracks) = tracks {
            let thickness = track_thickness.expect("a restored track layer carries a thickness");
            let visible = track_visible.expect("a restored track layer carries visibility");
            let mut layer = TrackLayer::shared(tracks, thickness, take_stamp(stamp));
            layer.set_visible(visible);
            set.tracks = Some(layer);
            if let Some(style) = set.track_style.as_mut() {
                style.set_visible(visible);
            }
        }
        set.restore_coloring(object_color, track_color);
        set.set_visibility(visibility);
        set.document = document;
        set
    }

    fn restore_coloring(&mut self, object: ColorState<ObjectColor>, track: ColorState<TrackColor>) {
        let object = Coloring::restore(object);
        self.check_coloring(object.metric());
        self.object_coloring = object;
        self.rebuild_frame_bounds();
        let track = Coloring::restore(track);
        if let Some(metric) = track.metric() {
            assert!(
                self.kind.trackable(),
                "only trackable objects carry track colors"
            );
            match metric {
                TrackColor::Measurement(measurement) => {
                    assert!(
                        measurement.colorable(),
                        "a restored measurement colors tracks"
                    );
                }
                TrackColor::Relation {
                    target,
                    field,
                    shift,
                    ..
                } => {
                    assert!(
                        self.relation(target, shift).is_some(),
                        "a restored track color names a relation of its set"
                    );
                    assert!(
                        crate::color::RELATION_FIELDS.contains(&field),
                        "a restored track color names a relation field"
                    );
                }
            }
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

    pub fn seeds(&self, time: u32) -> &[[f32; 3]] {
        self.seeds.frame(time)
    }

    fn manual_seeds(&mut self) -> Option<&mut Vec<Vec<[f32; 3]>>> {
        match &mut self.seeds {
            Seeds::Points(frames) => Some(frames),
            Seeds::Source(_) => None,
        }
    }

    pub(crate) fn add_seed(&mut self, time: u32, position: [f32; 3]) -> bool {
        assert_eq!(
            self.kind,
            ObjectKind::Surfaces,
            "seed points cut a surface detection"
        );
        assert!(
            position.iter().all(|value| value.is_finite()),
            "a seed point is finite"
        );
        let Some(frames) = self.manual_seeds() else {
            return false;
        };
        let Some(seeds) = frames.get_mut(time as usize) else {
            return false;
        };
        seeds.push(position);
        true
    }

    pub(crate) fn remove_seed(&mut self, time: u32, index: usize) -> bool {
        let Some(frames) = self.manual_seeds() else {
            return false;
        };
        let Some(seeds) = frames.get_mut(time as usize) else {
            return false;
        };
        if index >= seeds.len() {
            return false;
        }
        seeds.remove(index);
        true
    }

    pub(crate) fn clear_seeds(&mut self, time: u32) -> bool {
        let Some(frames) = self.manual_seeds() else {
            return false;
        };
        let Some(seeds) = frames.get_mut(time as usize) else {
            return false;
        };
        if seeds.is_empty() {
            return false;
        }
        seeds.clear();
        true
    }

    pub(crate) fn manual_seed_frames(&self) -> Option<&[Vec<[f32; 3]>]> {
        self.seeds.manual()
    }

    pub(crate) fn set_seed_frames(&mut self, seeds: Vec<Vec<[f32; 3]>>) -> bool {
        assert_eq!(
            seeds.len(),
            self.frames.len(),
            "the seed points cover every frame of the set"
        );
        assert!(
            self.kind == ObjectKind::Surfaces || seeds.iter().all(Vec::is_empty),
            "only a surface set carries seed points"
        );
        assert!(
            seeds
                .iter()
                .flatten()
                .all(|seed| seed.iter().all(|value| value.is_finite())),
            "a seed point is finite"
        );
        if self.seeds == Seeds::Points(seeds.clone()) {
            return false;
        }
        self.seeds = Seeds::Points(seeds);
        self.document += 1;
        true
    }

    pub(crate) fn retain_objects(
        &mut self,
        time: u32,
        keep: &[bool],
        stamp: u64,
    ) -> Result<bool, String> {
        let Some(frame) = self
            .frames
            .get(time as usize)
            .and_then(|slot| slot.data.clone())
        else {
            return Ok(false);
        };
        assert_eq!(
            keep.len(),
            frame.object_count(),
            "the kept objects cover every object of the frame"
        );
        if keep.iter().all(|kept| *kept) {
            return Ok(false);
        }
        if !self.curation.is_empty() && !self.curation.applied() {
            self.apply_curation(stamp);
        }
        let counts = self
            .frames
            .iter()
            .map(|slot| slot.data.as_deref().map(ObjectFrame::object_count))
            .collect::<Vec<_>>();
        self.curation.seed(counts.len(), |time| counts[time]);
        self.curation.record(time, keep, &frame.anchors());
        let removed = keep
            .iter()
            .enumerate()
            .filter(|(_, kept)| !**kept)
            .map(|(object, _)| object)
            .collect::<Vec<_>>();
        let pruned = frame.retain(keep).map_err(|error| error.to_string())?;
        self.install_retained(time, pruned, &removed, stamp);
        self.curation.set_applied();
        Ok(true)
    }

    pub fn curation(&self) -> &Curation {
        &self.curation
    }

    pub(crate) fn set_curation(&mut self, curation: Curation) -> bool {
        if self.curation == curation {
            return false;
        }
        self.curation = curation;
        self.document += 1;
        true
    }

    pub(crate) fn apply_curation(&mut self, stamp: u64) -> usize {
        if self.curation.is_empty() || self.curation.applied() {
            return 0;
        }
        let mut keeps = Vec::new();
        let mut pending = 0;
        for time in 0..self.frames.len() {
            if !self.curation.produced(time as u32) {
                continue;
            }
            let anchors = self.frames[time]
                .data
                .as_deref()
                .map_or_else(Vec::new, ObjectFrame::anchors);
            if let Some(kept) = self.curation.keep(time as u32, &anchors, self.voxel) {
                pending += kept.pending;
                if kept.keep.iter().any(|kept| !kept) {
                    keeps.push((time as u32, kept.keep));
                }
            }
        }
        let mut applied = 0usize;
        for (time, keep) in keeps {
            let Some(frame) = self.frames[time as usize].data.clone() else {
                continue;
            };
            let removed = keep
                .iter()
                .enumerate()
                .filter(|(_, kept)| !**kept)
                .map(|(object, _)| object)
                .collect::<Vec<_>>();
            applied += removed.len();
            let Ok(pruned) = frame.retain(&keep) else {
                continue;
            };
            self.install_retained(time, pruned, &removed, stamp);
        }
        self.curation.set_pending(pending);
        self.curation.set_applied();
        applied
    }

    pub(crate) fn apply_edits(&mut self, stamp: u64) -> usize {
        if self.curation.edits().is_empty() {
            return 0;
        }
        if self.tracks.is_none() {
            let dropped = self.curation.forget_edits();
            self.document += 1;
            return dropped;
        }
        let edits = self.curation.edits().to_vec();
        let mut resolved = Vec::with_capacity(edits.len());
        for edit in &edits {
            let Some(edit) = self.resolve_edit(*edit) else {
                let dropped = self.curation.forget_edits();
                self.document += 1;
                return dropped;
            };
            resolved.push(edit);
        }
        let mut applied = 0usize;
        for edit in resolved {
            let Some(layer) = self.tracks.as_mut() else {
                break;
            };
            let done = match edit {
                TrackEdit::Cut(node) => layer.cut(node.time(), node.object(), stamp),
                TrackEdit::Join { source, target } => layer.join(
                    (source.time(), source.object()),
                    (target.time(), target.object()),
                    stamp,
                ),
            };
            if !done {
                let dropped = self.curation.forget_edits();
                self.rebuild_track_bounds();
                self.document += 1;
                return dropped;
            }
            applied += 1;
        }
        if applied > 0 {
            self.rebuild_track_bounds();
            self.document += 1;
        }
        0
    }

    fn resolve_edit(&self, edit: TrackEdit) -> Option<TrackEdit> {
        let resolve = |node: TrackAnchor| -> Option<TrackAnchor> {
            let frame = self.frames.get(node.time() as usize)?.data.as_deref()?;
            let object =
                crate::curation::match_anchor(&frame.anchors(), node.position(), self.voxel)?;
            Some(TrackAnchor::new(node.time(), object, node.position()))
        };
        match edit {
            TrackEdit::Cut(node) => Some(TrackEdit::Cut(resolve(node)?)),
            TrackEdit::Join { source, target } => Some(TrackEdit::Join {
                source: resolve(source)?,
                target: resolve(target)?,
            }),
        }
    }

    pub(crate) fn edit_track(&mut self, edit: TrackEdit, stamp: u64) -> bool {
        let Some(layer) = self.tracks.as_mut() else {
            return false;
        };
        let done = match edit {
            TrackEdit::Cut(node) => layer.cut(node.time(), node.object(), stamp),
            TrackEdit::Join { source, target } => layer.join(
                (source.time(), source.object()),
                (target.time(), target.object()),
                stamp,
            ),
        };
        if !done {
            return false;
        }
        self.curation.record_edit(edit);
        self.revision = stamp;
        self.document += 1;
        self.visibility = Visibility::All;
        self.rebuild_track_bounds();
        true
    }

    fn install_retained(
        &mut self,
        time: u32,
        pruned: Option<ObjectFrame>,
        objects: &[usize],
        stamp: u64,
    ) {
        let empty = pruned.as_ref().is_none_or(ObjectFrame::is_empty);
        let mut visibility = match &self.visibility {
            Visibility::All => Visibility::All,
            Visibility::Object(frames) => {
                let mut frames = frames.as_ref().clone();
                if let Some(mask) = frames.get_mut(time as usize) {
                    *mask = retained(mask, objects);
                }
                Visibility::Object(Arc::new(frames))
            }
            Visibility::Track(tracks) => Visibility::Track(Arc::clone(tracks)),
        };
        let pruned_tracks = self.tracks.as_ref().map(|layer| {
            (
                layer.thickness(),
                layer.visible(),
                layer.set().without_objects(time, objects),
            )
        });
        let slot = self
            .frames
            .get_mut(time as usize)
            .expect("the pruned object lies inside the time series");
        slot.stamp = stamp;
        slot.data = if empty { None } else { pruned.map(Arc::new) };
        self.frame_bounds[time as usize] = self.frame_extent(time);
        if let Some((thickness, visible, pruned)) = pruned_tracks
            && let Some(set) = pruned
        {
            self.tracks = (!set.is_empty()).then(|| {
                let mut layer = TrackLayer::new(set, thickness, stamp);
                layer.set_visible(visible);
                layer
            });
            visibility = Visibility::All;
        }
        let id = self.id;
        let mut cleared = Vec::new();
        for relation in &mut self.relations {
            if relation.invalidate(time) {
                cleared.push((relation.target(), time));
            }
            if relation.target() == id
                && let Some(source) = relation.source_of(time)
                && relation.invalidate(source)
            {
                cleared.push((id, source));
            }
        }
        for (target, source) in cleared {
            self.refresh_relation_bounds(target, source);
        }
        self.revision = stamp;
        self.document += 1;
        self.visibility = visibility;
        self.rebuild_track_bounds();
    }

    pub(crate) fn duplicate(
        &self,
        id: ObjectSetId,
        name: String,
        color: [f32; 3],
        provenance: Provenance,
    ) -> Self {
        let relations = self
            .relations
            .iter()
            .map(|relation| {
                Relation::new(
                    relation.target(),
                    relation.shift(),
                    vec![None; self.frames.len()],
                )
            })
            .collect::<Vec<_>>();
        let mut duplicate = Self {
            id,
            kind: self.kind,
            name,
            channel: self.channel,
            channels: self.channels,
            color,
            visible: self.visible,
            opacity: self.opacity,
            voxel: self.voxel,
            params: self.params,
            track_style: self.track_style,
            frames: self
                .frames
                .iter()
                .map(|slot| FrameSlot {
                    stamp: 0,
                    data: slot.data.clone(),
                })
                .collect(),
            frame_bounds: vec![None; self.frames.len()],
            object_coloring: self.object_coloring.clone(),
            track_coloring: self.track_coloring.clone(),
            track_bounds: None,
            revision: 1,
            document: 1,
            visibility: Visibility::All,
            seeds: self.seeds.clone(),
            tracks: None,
            relations,
            provenance,
            curation: Curation::default(),
        };
        duplicate.rebuild_frame_bounds();
        duplicate.rebuild_track_bounds();
        duplicate
    }

    pub(crate) fn merged(&self, other: &Self, stamp: u64) -> Result<Self, String> {
        assert_eq!(self.kind, other.kind, "a merge combines one object kind");
        assert_eq!(
            self.channels, other.channels,
            "a merge measures one set of channels"
        );
        if self.frames.len() != other.frames.len() {
            return Err(format!(
                "the object set {} covers another set of frames",
                other.name
            ));
        }
        let mut frames = Vec::with_capacity(self.frames.len());
        for (left, right) in self.frames.iter().zip(&other.frames) {
            let frame = match (left.data.as_deref(), right.data.as_deref()) {
                (Some(carried), Some(absorbed)) => {
                    if carried.level() != absorbed.level() {
                        return Err(format!(
                            "the object set {} detects another resolution",
                            other.name
                        ));
                    }
                    Some(Arc::new(carried.merge(absorbed)?))
                }
                (Some(_), None) => left.data.clone(),
                (None, Some(_)) => right.data.clone(),
                (None, None) => None,
            };
            frames.push(FrameSlot { stamp, data: frame });
        }
        let offsets = self
            .frames
            .iter()
            .map(|slot| slot.data.as_deref().map_or(0, ObjectFrame::object_count))
            .collect::<Vec<_>>();
        let tracks = match (&self.tracks, &other.tracks) {
            (None, None) => None,
            (Some(layer), None) => Some((layer.set().clone(), layer.thickness(), layer.visible())),
            (None, Some(layer)) => Some((
                layer.set().shift(&offsets),
                layer.thickness(),
                layer.visible(),
            )),
            (Some(layer), Some(absorbed)) => Some((
                layer.set().append(&absorbed.set().shift(&offsets)),
                layer.thickness(),
                layer.visible(),
            )),
        };
        let tracks = tracks.map(|(set, thickness, visible)| {
            let mut layer = TrackLayer::new(set, thickness, stamp);
            layer.set_visible(visible);
            layer
        });
        let mut object_coloring = self.object_coloring.clone();
        if object_coloring.metric().and_then(ObjectColor::target) == Some(other.id) {
            object_coloring.set_metric(None);
        }
        let mut track_coloring = self.track_coloring.clone();
        if track_coloring.metric().and_then(TrackColor::target) == Some(other.id) {
            track_coloring.set_metric(None);
        }
        let mut relations = self
            .relations
            .iter()
            .filter(|relation| relation.target() != other.id)
            .map(|relation| {
                Relation::new(
                    relation.target(),
                    relation.shift(),
                    vec![None; self.frames.len()],
                )
            })
            .collect::<Vec<_>>();
        for relation in &other.relations {
            if relation.target() == self.id
                || relations.iter().any(|existing| {
                    existing.target() == relation.target() && existing.shift() == relation.shift()
                })
            {
                continue;
            }
            relations.push(Relation::new(
                relation.target(),
                relation.shift(),
                vec![None; self.frames.len()],
            ));
        }
        let seeds = self.seeds.clone();
        let mut merged = Self {
            id: self.id,
            kind: self.kind,
            name: self.name.clone(),
            channel: self.channel,
            channels: self.channels,
            color: self.color,
            visible: self.visible,
            opacity: self.opacity,
            voxel: self.voxel,
            params: self.params,
            track_style: self.track_style.or(other.track_style),
            frames,
            frame_bounds: vec![None; self.frames.len()],
            object_coloring,
            track_coloring,
            track_bounds: None,
            revision: stamp,
            document: self.document + 1,
            visibility: Visibility::All,
            seeds,
            tracks,
            relations,
            provenance: self.provenance.clone(),
            curation: self.curation.combined(&other.curation),
        };
        merged.rebuild_frame_bounds();
        merged.rebuild_track_bounds();
        Ok(merged)
    }

    pub(crate) fn invalidate_relation(
        &mut self,
        target: ObjectSetId,
        time: u32,
        stamp: u64,
    ) -> bool {
        let mut cleared = Vec::new();
        for relation in self
            .relations
            .iter_mut()
            .filter(|relation| relation.target() == target)
        {
            let Some(source) = relation.source_of(time) else {
                continue;
            };
            if relation.invalidate(source) {
                cleared.push(source);
            }
        }
        if cleared.is_empty() {
            return false;
        }
        for source in cleared {
            self.refresh_relation_bounds(target, source);
        }
        self.revision = stamp;
        self.document += 1;
        true
    }

    pub(crate) fn invalidate_relations(&mut self, time: u32, stamp: u64) -> bool {
        let id = self.id;
        let mut cleared = Vec::new();
        for relation in &mut self.relations {
            if relation.invalidate(time) {
                cleared.push((relation.target(), time));
            }
            if relation.target() == id
                && let Some(source) = relation.source_of(time)
                && relation.invalidate(source)
            {
                cleared.push((id, source));
            }
        }
        if cleared.is_empty() {
            return false;
        }
        for (target, source) in cleared {
            self.refresh_relation_bounds(target, source);
        }
        self.revision = stamp;
        self.document += 1;
        true
    }

    pub(crate) fn install(&mut self, time: u32, frame: ObjectFrame, stamp: u64) {
        self.install_shared(time, Arc::new(frame), stamp);
    }

    pub(crate) fn install_shared(&mut self, time: u32, frame: Arc<ObjectFrame>, stamp: u64) {
        assert_eq!(
            frame.kind(),
            self.kind,
            "an object frame matches its object set"
        );
        let slot = self
            .frames
            .get_mut(time as usize)
            .expect("the object frame lies inside the time series");
        self.revision = stamp;
        self.document += 1;
        self.visibility = Visibility::All;
        slot.stamp = stamp;
        slot.data = Some(frame);
        self.frame_bounds[time as usize] = self.frame_extent(time);
        self.curation.set_produced(time);
    }

    pub(crate) fn install_optional(
        &mut self,
        time: u32,
        frame: Option<Arc<ObjectFrame>>,
        stamp: u64,
    ) {
        if let Some(frame) = &frame {
            assert_eq!(
                frame.kind(),
                self.kind,
                "an object frame matches its object set"
            );
        }
        let slot = self
            .frames
            .get_mut(time as usize)
            .expect("the object frame lies inside the time series");
        self.revision = stamp;
        self.document += 1;
        self.visibility = Visibility::All;
        slot.stamp = stamp;
        slot.data = frame;
        self.frame_bounds[time as usize] = self.frame_extent(time);
        self.curation.set_produced(time);
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

    pub fn set_track_style(&mut self, params: TrackParams, thickness: f32, stamp: u64) -> bool {
        let Some(style) = self.track_style else {
            return false;
        };
        let next = TrackStyle::new(
            params,
            thickness.clamp(self.min_thickness(), self.max_thickness()),
        );
        if style == next {
            return false;
        }
        self.track_style = Some(next);
        if let Some(layer) = self.tracks.as_mut() {
            layer.set_thickness(next.thickness(), stamp);
        }
        self.revision = stamp;
        self.document += 1;
        true
    }

    pub(crate) fn install_tracks(&mut self, set: TrackSet, stamp: u64) {
        let style = self
            .track_style
            .expect("only trackable objects install tracks");
        let thickness = self
            .tracks
            .as_ref()
            .map_or(style.thickness(), TrackLayer::thickness);
        let mut layer = TrackLayer::new(set, thickness, stamp);
        layer.set_visible(style.visible());
        self.tracks = Some(layer);
        self.rebuild_track_bounds();
        self.revision = stamp;
        self.document += 1;
        self.visibility = Visibility::All;
        self.apply_edits(stamp);
    }

    pub(crate) fn clear_tracks(&mut self, stamp: u64) {
        self.revision = stamp;
        self.document += 1;
        self.visibility = Visibility::All;
        self.tracks = None;
        self.track_bounds = None;
    }

    pub(crate) fn cut_track(&mut self, time: u32, object: usize, stamp: u64) -> bool {
        let Some(position) = self.anchor_at(time, object) else {
            return false;
        };
        self.edit_track(
            TrackEdit::Cut(TrackAnchor::new(time, object, position)),
            stamp,
        )
    }

    pub(crate) fn join_tracks(
        &mut self,
        source: (u32, usize),
        target: (u32, usize),
        stamp: u64,
    ) -> bool {
        let (Some(source_position), Some(target_position)) = (
            self.anchor_at(source.0, source.1),
            self.anchor_at(target.0, target.1),
        ) else {
            return false;
        };
        self.edit_track(
            TrackEdit::Join {
                source: TrackAnchor::new(source.0, source.1, source_position),
                target: TrackAnchor::new(target.0, target.1, target_position),
            },
            stamp,
        )
    }

    fn anchor_at(&self, time: u32, object: usize) -> Option<[f32; 3]> {
        let frame = self.frames.get(time as usize)?.data.as_deref()?;
        let anchors = frame.anchors();
        Some(anchors.get(object)?.position())
    }

    fn rebuild_frame_bounds(&mut self) {
        for time in 0..self.frames.len() {
            self.frame_bounds[time] = self.frame_extent(time as u32);
        }
    }

    fn refresh_relation_bounds(&mut self, target: ObjectSetId, source: u32) {
        let object = self.object_coloring.metric().and_then(ObjectColor::target) == Some(target);
        let track = self.track_coloring.metric().and_then(TrackColor::target) == Some(target);
        if object {
            let bounds = self.frame_extent(source);
            if let Some(slot) = self.frame_bounds.get_mut(source as usize) {
                *slot = bounds;
            }
        }
        if track {
            self.rebuild_track_bounds();
        }
    }

    fn frame_extent(&self, time: u32) -> Option<Extent> {
        let metric = self.object_coloring.metric()?;
        let frame = self.frames.get(time as usize)?.data.as_deref()?;
        let relation = metric
            .relation()
            .and_then(|(target, shift)| self.relation(target, shift))
            .and_then(|relation| relation.frame(time));
        metric.extent(frame, relation)
    }

    fn rebuild_track_bounds(&mut self) {
        let bounds = self
            .track_coloring
            .metric()
            .and_then(|metric| self.track_bounds_of(metric));
        self.track_bounds = bounds;
    }

    fn track_bounds_of(&self, metric: TrackColor) -> Option<Extent> {
        let set = self.tracks.as_ref()?.set();
        (0..set.len()).fold(None, |accumulated, track| {
            accumulate(accumulated, self.track_value(track, metric))
        })
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

    pub fn track_visible(&self) -> bool {
        match &self.tracks {
            Some(layer) => layer.visible(),
            None => self.track_style.is_none_or(TrackStyle::visible),
        }
    }

    pub fn set_track_visible(&mut self, visible: bool) {
        if let Some(style) = self.track_style.as_mut() {
            style.set_visible(visible);
        }
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

fn retained(mask: &[bool], objects: &[usize]) -> Vec<bool> {
    mask.iter()
        .enumerate()
        .filter(|(index, _)| objects.binary_search(index).is_err())
        .map(|(_, value)| *value)
        .collect()
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
