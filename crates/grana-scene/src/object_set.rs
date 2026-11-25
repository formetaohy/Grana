use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use grana_measure::{
    ChildField, ChildMetric, Classifier, Mask, Measurement, ObjectColoc, RelationField,
    TrackMeasurement,
};
use grana_object::TrackParams;
use grana_object::match_anchor;
use grana_object::{
    ObjectId, ObjectIdSource, ObjectIds, RelationFrame, SpotSet, TrackId, TrackIds, TrackSet,
};
use grana_store::SpillStore;

use crate::alignment::LocalizationAlignment;
use crate::color::ObjectColor;
use crate::curation::{Curation, TrackAnchor, TrackEdit};
use crate::dataset::Dataset;
use crate::frame::{Frame, SPOT_PAGE_THRESHOLD, open_store, place};
use crate::key::ObjectSetKey;
use crate::label_export::LabelExport;
use crate::localization::LocalizationSource;
use crate::object::{ObjectFrame, ObjectKind, ObjectParams, TrackStyle};
use crate::provenance::{Provenance, Recipe};
use crate::relation::Relation;
use crate::revision::{GeometryId, Revision, Revisions};
use crate::seed::Seeds;
use crate::source::ObjectSource;
use crate::state::{RelationState, SetState};
use crate::track_color::TrackColor;
use crate::track_layer::TrackLayer;
use grana_display::{ColorState, Coloring, Extent, Material};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Readoption {
    carried: usize,
    fresh: usize,
    vanished: usize,
    purged: usize,
}

impl Readoption {
    pub fn carried(self) -> usize {
        self.carried
    }

    pub fn fresh(self) -> usize {
        self.fresh
    }

    pub fn vanished(self) -> usize {
        self.vanished
    }

    pub fn purged(self) -> usize {
        self.purged
    }

    pub fn inherited(self) -> bool {
        self.carried > 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Visibility {
    All,
    Objects(ObjectIds),
    Tracks(TrackIds),
    Molecules {
        mask: Arc<Mask>,
        rows: Arc<HashMap<u32, u32>>,
    },
}

impl Visibility {
    pub(crate) fn objects(ids: ObjectIds) -> Self {
        Self::Objects(ids)
    }

    pub(crate) fn molecules(mask: Arc<Mask>, rows: &HashMap<u32, u32>) -> Self {
        Self::Molecules {
            mask,
            rows: Arc::new(rows.clone()),
        }
    }
}

struct FrameSlot {
    stamp: Revision,
    geometry: GeometryId,
    appearance: Revision,
    frame: Option<Frame>,
}

impl FrameSlot {
    fn new() -> Self {
        Self {
            stamp: Revision::ZERO,
            geometry: GeometryId::NONE,
            appearance: Revision::ZERO,
            frame: None,
        }
    }

    fn objects(&self) -> usize {
        self.frame.as_ref().map_or(0, Frame::objects)
    }

    fn shared(&self) -> Option<Arc<ObjectFrame>> {
        self.frame.as_ref().map(Frame::shared)
    }

    fn spot_frame(&self) -> Option<Arc<SpotSet>> {
        self.frame.as_ref()?.spot_frame()
    }
}

#[derive(Clone, Copy)]
struct TrackBounds {
    revision: Revision,
    metric: TrackColor,
    extent: Option<Extent>,
}

pub struct ObjectSet {
    id: ObjectSetId,
    key: ObjectSetKey,
    kind: ObjectKind,
    name: String,
    channel: usize,
    channels: usize,
    color: [f32; 3],
    visible: bool,
    labels: bool,
    label: Option<ObjectColor>,
    hidden: Arc<Vec<ObjectIds>>,
    hidden_total: usize,
    opacity: f32,
    material: Material,
    voxel: f32,
    params: ObjectParams,
    track_style: Option<TrackStyle>,
    frames: Vec<FrameSlot>,
    store: Option<Arc<SpillStore>>,
    installed_spots: usize,
    object_coloring: Coloring<ObjectColor>,
    track_coloring: Coloring<TrackColor>,
    classifier: Option<Classifier>,
    object_coloc: Option<Arc<ObjectColoc>>,
    label_export: Option<LabelExport>,
    localization: Option<LocalizationSource>,
    track_bounds: Mutex<Option<TrackBounds>>,
    revision: Revision,
    shape: Revision,
    document: Revision,
    visibility: Visibility,
    seeds: Seeds,
    tracks: Option<TrackLayer>,
    relations: Vec<Relation>,
    provenance: Provenance,
    version: u32,
    curation: Arc<Curation>,
    source: ObjectIdSource,
    replaced: Option<Arc<Vec<Option<Arc<ObjectFrame>>>>>,
}

impl ObjectSet {
    pub(crate) fn new(
        id: ObjectSetId,
        key: ObjectSetKey,
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
            key,
            kind,
            name,
            channel,
            channels: dataset.channels().len(),
            color,
            visible: true,
            labels: false,
            label: None,
            hidden: Arc::new(Vec::new()),
            hidden_total: 0,
            opacity: 1.0,
            material: Material::DEFAULT,
            voxel,
            params,
            track_style,
            frames,
            store: None,
            installed_spots: 0,
            object_coloring: Coloring::new(),
            track_coloring: Coloring::new(),
            classifier: None,
            object_coloc: None,
            label_export: None,
            localization: None,
            track_bounds: Mutex::new(None),
            revision: Revision::FIRST,
            shape: Revision::FIRST,
            document: Revision::FIRST,
            visibility: Visibility::All,
            seeds: Seeds::points(dataset.frame_count() as usize),
            tracks: None,
            relations: Vec::new(),
            provenance: Provenance::detection(),
            version: grana_object::Algorithm::Detection.version(),
            curation: Arc::new(Curation::default()),
            source: ObjectIdSource::new(),
            replaced: None,
        }
    }

    pub fn id_source(&self) -> ObjectIdSource {
        self.source.clone()
    }

    pub(crate) fn begin_replace(&mut self) -> bool {
        if self.replaced.is_some() {
            return false;
        }
        let frames = self
            .frames
            .iter()
            .map(FrameSlot::shared)
            .collect::<Vec<_>>();
        if frames.iter().all(Option::is_none) {
            return false;
        }
        self.replaced = Some(Arc::new(frames));
        true
    }

    pub(crate) fn end_replace(&mut self) -> bool {
        self.replaced.take().is_some()
    }

    pub fn replacing(&self) -> bool {
        self.replaced.is_some()
    }

    pub fn id(&self) -> ObjectSetId {
        self.id
    }

    pub fn key(&self) -> &ObjectSetKey {
        &self.key
    }

    pub(crate) fn set_key(&mut self, key: ObjectSetKey) -> bool {
        if self.key == key {
            return false;
        }
        self.key = key;
        self.document = Revision::mint();
        true
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
        self.document = Revision::mint();
        true
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn measured_channels(&self) -> usize {
        self.channels
    }

    pub fn voxel(&self) -> f32 {
        self.voxel
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub(crate) fn set_provenance(&mut self, provenance: Provenance) {
        if self.provenance == provenance {
            return;
        }
        self.version = provenance.algorithm().version();
        self.provenance = provenance;
        self.document = Revision::mint();
    }

    pub(crate) fn recipe(&self) -> Recipe {
        Recipe::new(
            self.name.clone(),
            self.channel,
            self.params,
            self.seeds.clone(),
            self.curation.as_ref().clone(),
            self.provenance.clone(),
        )
    }

    pub fn seed_source(&self) -> Option<ObjectSetId> {
        self.seeds.source()
    }

    pub fn surface_seeds(&self) -> &Seeds {
        &self.seeds
    }

    pub(crate) fn redirect_seeds(&mut self, from: ObjectSetId, to: ObjectSetId) -> bool {
        if self.seeds.source() != Some(from) {
            return false;
        }
        self.seeds = Seeds::Source(to);
        self.document = Revision::mint();
        true
    }

    pub(crate) fn set_seed_source(&mut self, source: Option<ObjectSetId>, frames: usize) -> bool {
        assert!(
            matches!(self.kind, ObjectKind::Surfaces | ObjectKind::Cells),
            "only a surface or a cell set seeds from another object set"
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
        self.document = Revision::mint();
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
        self.document = Revision::mint();
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    pub fn material(&self) -> Material {
        self.material
    }

    pub fn set_material(&mut self, material: Material) {
        if self.material == material {
            return;
        }
        self.material = material;
        self.document = Revision::mint();
    }
    pub fn set_opacity(&mut self, opacity: f32) {
        let opacity = opacity.clamp(0.0, 1.0);
        if self.opacity == opacity {
            return;
        }
        self.opacity = opacity;
        self.document = Revision::mint();
    }

    pub fn classifier(&self) -> Option<&Classifier> {
        self.classifier.as_ref()
    }

    pub fn object_coloc(&self) -> Option<&ObjectColoc> {
        self.object_coloc.as_deref()
    }

    pub fn shared_object_coloc(&self) -> Option<Arc<ObjectColoc>> {
        self.object_coloc.clone()
    }

    pub fn label_export(&self) -> Option<&LabelExport> {
        self.label_export.as_ref()
    }

    pub fn localization(&self) -> Option<&LocalizationSource> {
        self.localization.as_ref()
    }

    pub(crate) fn set_localization(&mut self, source: LocalizationSource, stamp: Revision) -> bool {
        if self.kind != ObjectKind::Spots {
            return false;
        }
        assert_eq!(
            source.applied().channel(),
            self.channel,
            "a localization table belongs to the channel it was measured in"
        );
        if self.localization.as_ref() == Some(&source) {
            return false;
        }
        self.localization = Some(source);
        self.shape = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn set_localization_spec(
        &mut self,
        spec: grana_object::LocalizationSpec,
        stamp: Revision,
    ) -> bool {
        let Some(source) = self.localization.as_ref() else {
            return false;
        };
        let next = source.with_spec(spec);
        if self.localization.as_ref() == Some(&next) {
            return false;
        }
        self.localization = Some(next);
        self.shape = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn set_localization_group_colored(&mut self, group_colored: bool) -> bool {
        let Some(source) = self.localization.as_ref() else {
            return false;
        };
        let next = source.with_group_colored(group_colored);
        if self.localization.as_ref() == Some(&next) {
            return false;
        }
        self.localization = Some(next);
        self.document = Revision::mint();
        true
    }

    pub(crate) fn set_localization_projected(&mut self, projected: bool) -> bool {
        let Some(source) = self.localization.as_ref() else {
            return false;
        };
        let next = source.with_projected(projected);
        if self.localization.as_ref() == Some(&next) {
            return false;
        }
        self.localization = Some(next);
        self.document = Revision::mint();
        true
    }

    pub(crate) fn set_localization_clustered(&mut self, clustered: bool, stamp: Revision) -> bool {
        let Some(source) = self.localization.as_ref() else {
            return false;
        };
        let next = source.with_clustered(clustered);
        if self.localization.as_ref() == Some(&next) {
            return false;
        }
        self.localization = Some(next);
        self.shape = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn set_localization_clusters(
        &mut self,
        clusters: grana_object::ClusterSpec,
        stamp: Revision,
    ) -> bool {
        let Some(source) = self.localization.as_ref() else {
            return false;
        };
        let next = source.with_clusters(clusters);
        if self.localization.as_ref() == Some(&next) {
            return false;
        }
        self.localization = Some(next);
        self.shape = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn set_label_export(&mut self, export: Option<LabelExport>) -> bool {
        if self.label_export == export {
            return false;
        }
        self.label_export = export;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn set_object_coloc(&mut self, coloc: Option<ObjectColoc>, stamp: Revision) -> bool {
        if self.object_coloc.as_deref() == coloc.as_ref() {
            return false;
        }
        let pair = coloc.as_ref().map(ObjectColoc::pair);
        self.object_coloc = coloc.map(Arc::new);
        match pair {
            Some(pair) => {
                if matches!(
                    self.object_coloring.metric(),
                    Some(ObjectColor::Coloc { pair: held, .. }) if held != pair
                ) {
                    self.object_coloring.set_metric(None);
                }
                if matches!(
                    self.label,
                    Some(ObjectColor::Coloc { pair: held, .. }) if held != pair
                ) {
                    self.label = None;
                }
            }
            None => {
                self.clear_coloc_metrics();
            }
        }
        self.revision = stamp;
        self.document = Revision::mint();
        true
    }

    fn clear_coloc_metrics(&mut self) -> bool {
        let mut cleared = false;
        if matches!(
            self.object_coloring.metric(),
            Some(ObjectColor::Coloc { .. })
        ) {
            self.object_coloring.set_metric(None);
            cleared = true;
        }
        if matches!(self.label, Some(ObjectColor::Coloc { .. })) {
            self.label = None;
            cleared = true;
        }
        cleared
    }

    fn discard_object_coloc(&mut self) -> bool {
        if self.object_coloc.take().is_none() {
            return false;
        }
        self.clear_coloc_metrics();
        self.document = Revision::mint();
        true
    }

    pub(crate) fn set_classifier(
        &mut self,
        classifier: Option<Classifier>,
        stamp: Revision,
    ) -> bool {
        if self.classifier == classifier {
            return false;
        }
        self.classifier = classifier;
        if self.classifier.is_none() {
            if self.object_coloring.metric() == Some(ObjectColor::Score) {
                self.object_coloring.set_metric(None);
            }
            if self.label == Some(ObjectColor::Score) {
                self.label = None;
            }
        }
        self.revision = stamp;
        self.document = Revision::mint();
        true
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

    pub fn child_fields(&self) -> Vec<ChildField> {
        let metrics = self
            .kind
            .measurements(self.channels)
            .into_iter()
            .filter_map(ChildMetric::of)
            .collect::<Vec<_>>();
        ChildField::fields(&metrics)
    }

    pub(crate) fn set_object_coloring(&mut self, coloring: Coloring<ObjectColor>) -> bool {
        if self.object_coloring == coloring {
            return false;
        }
        self.object_coloring = coloring;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn color_by_label(&mut self) {
        let mut coloring = self.object_coloring.clone();
        if coloring.set_metric(Some(ObjectColor::Identity)) {
            self.set_object_coloring(coloring);
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
                    RelationField::CONTINUOUS.contains(&field),
                    "a relation field colors the tracks of this set"
                );
            }
        }
        if self.track_coloring == coloring {
            return false;
        }
        self.track_coloring = coloring;
        self.document = Revision::mint();
        true
    }

    pub fn track_extent(&self) -> Option<Extent> {
        let metric = self.track_coloring.metric()?;
        if let Some(manual) = self.track_coloring.manual() {
            return Some(manual);
        }
        if let Some(cached) = self.cached_track_bounds()
            && cached.revision == self.revision
            && cached.metric == metric
        {
            return cached.extent;
        }
        let extent = self.track_bounds_of(metric);
        *self.lock_track_bounds() = Some(TrackBounds {
            revision: self.revision,
            metric,
            extent,
        });
        extent
    }

    pub fn track_color(&self, track: TrackId) -> [f32; 3] {
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

    fn track_value(&self, track: TrackId, metric: TrackColor) -> f32 {
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

    pub fn labels(&self) -> bool {
        self.labels
    }

    pub fn label_metric(&self) -> Option<ObjectColor> {
        self.label
    }

    pub fn set_labels(&mut self, labels: bool) {
        if self.labels == labels {
            return;
        }
        self.labels = labels;
        self.document = Revision::mint();
    }

    pub(crate) fn set_label_metric(&mut self, metric: Option<ObjectColor>) -> bool {
        if self.label == metric {
            return false;
        }
        self.label = metric;
        self.document = Revision::mint();
        true
    }

    pub fn hidden_object(&self, time: u32, object: usize) -> bool {
        let Some(hidden) = self.hidden.get(time as usize) else {
            return false;
        };
        self.frame_shared(time)
            .and_then(|frame| frame.id_of(object))
            .is_some_and(|id| hidden.contains(id))
    }

    pub fn hidden_count(&self) -> usize {
        self.hidden_total
    }

    pub(crate) fn set_hidden(
        &mut self,
        time: u32,
        ids: &[ObjectId],
        hidden: bool,
        stamp: Revision,
    ) -> bool {
        let changed = self.hide(time, ids, hidden, stamp);
        if changed {
            self.record_hidden(time);
        }
        changed
    }

    fn hide(&mut self, time: u32, ids: &[ObjectId], hidden: bool, stamp: Revision) -> bool {
        let Some(frame) = self
            .frames
            .get(time as usize)
            .and_then(|slot| slot.shared())
        else {
            return false;
        };
        if self.hidden.is_empty() {
            if !hidden {
                return false;
            }
            self.hidden = Arc::new(vec![ObjectIds::empty(); self.frames.len()]);
        }
        let held = self
            .hidden
            .get(time as usize)
            .cloned()
            .unwrap_or_else(ObjectIds::empty);
        let mut next = held.to_vec();
        for id in ids {
            if frame.index_of(*id).is_none() {
                continue;
            }
            match next.binary_search(id) {
                Ok(at) if !hidden => {
                    next.remove(at);
                }
                Ok(_) => {}
                Err(at) if hidden => next.insert(at, *id),
                Err(_) => {}
            }
        }
        if next.len() == held.len() {
            return false;
        }
        let next = ObjectIds::new(next);
        self.hidden_total = self
            .hidden_total
            .saturating_sub(held.len())
            .saturating_add(next.len());
        Arc::make_mut(&mut self.hidden)[time as usize] = next;
        self.shape = stamp;
        self.document = Revision::mint();
        if let Some(slot) = self.frames.get_mut(time as usize) {
            slot.appearance = Revision::mint();
        }
        true
    }

    fn record_hidden(&mut self, time: u32) {
        let Some(frame) = self.frame_shared(time) else {
            return;
        };
        let ids = self
            .hidden
            .get(time as usize)
            .cloned()
            .unwrap_or_else(ObjectIds::empty);
        let anchors = frame.anchors();
        let records = ids
            .as_slice()
            .iter()
            .filter_map(|id| frame.index_of(*id))
            .filter_map(|object| anchors.get(object).copied())
            .collect::<Vec<_>>();
        assert_eq!(
            records.len(),
            ids.len(),
            "a hidden object lives in the frame that hides it"
        );
        let counts = self
            .frames
            .iter()
            .map(|slot| slot.objects())
            .collect::<Vec<_>>();
        let curation = Arc::make_mut(&mut self.curation);
        curation.seed(counts.len());
        curation.record_hidden(time, records);
    }

    pub(crate) fn clear_hidden(&mut self, stamp: Revision) -> bool {
        if self.hidden.is_empty() {
            return false;
        }
        Arc::make_mut(&mut self.hidden).clear();
        self.hidden_total = 0;
        Arc::make_mut(&mut self.curation).clear_hidden();
        self.shape = stamp;
        self.document = Revision::mint();
        self.refresh_appearance();
        true
    }

    fn recount_hidden(&mut self) {
        self.hidden_total = self.hidden.iter().map(ObjectIds::len).sum();
    }

    pub(crate) fn restore_hidden(&mut self, hidden: Arc<Vec<ObjectIds>>) {
        if hidden.is_empty() {
            self.hidden = hidden;
            self.hidden_total = 0;
            return;
        }
        assert_eq!(
            hidden.len(),
            self.frames.len(),
            "the hidden objects cover every frame of the set"
        );
        for (ids, slot) in hidden.iter().zip(&self.frames) {
            let frame = slot.shared();
            let live = ids.as_slice().iter().all(|id| {
                frame
                    .as_ref()
                    .is_some_and(|frame| frame.index_of(*id).is_some())
            });
            assert!(live, "a hidden object lives in the frame that hides it");
        }
        self.hidden = hidden;
        self.recount_hidden();
        self.refresh_appearance();
    }

    fn refresh_appearance(&mut self) {
        for slot in &mut self.frames {
            slot.appearance = Revision::mint();
        }
    }

    pub fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        self.document = Revision::mint();
    }

    pub fn params(&self) -> ObjectParams {
        self.params
    }

    pub fn set_segment_params(&mut self, params: crate::segment::SegmentParams) -> bool {
        let Provenance::Segmented { params: held } = &mut self.provenance else {
            return false;
        };
        if *held == params {
            return false;
        }
        *held = params;
        self.document = Revision::mint();
        true
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
        self.document = Revision::mint();
    }

    pub fn paged(&self) -> bool {
        self.store.is_some()
    }

    pub fn paged_bytes(&self) -> u64 {
        self.store
            .as_ref()
            .map_or(0, |store| store.stats().resident())
    }

    pub fn object_count(&self, time: u32) -> usize {
        self.frames.get(time as usize).map_or(0, FrameSlot::objects)
    }

    pub fn frame_shared(&self, time: u32) -> Option<Arc<ObjectFrame>> {
        self.frames.get(time as usize)?.shared()
    }

    pub fn frame_handle(&self, time: u32) -> Option<Frame> {
        self.frames.get(time as usize)?.frame.clone()
    }

    pub fn spot_frame(&self, time: u32) -> Option<Arc<SpotSet>> {
        self.frames.get(time as usize)?.frame.as_ref()?.spot_frame()
    }

    pub fn source(&self) -> ObjectSource {
        ObjectSource::live(
            self.frame_handles(),
            self.visibility.clone(),
            Arc::clone(&self.hidden),
            self.localization_filter(),
            self.tracks.as_ref().map(TrackLayer::shared_set),
        )
    }

    pub(crate) fn plain_source(&self) -> ObjectSource {
        ObjectSource::plain(self.frame_handles())
    }

    fn frame_handles(&self) -> Vec<Option<Frame>> {
        self.frames
            .iter()
            .map(|slot| slot.frame.clone())
            .collect::<Vec<_>>()
    }

    pub fn frame_stamp(&self, time: u32) -> Option<Revision> {
        self.frames
            .get(time as usize)
            .and_then(|slot| slot.frame.as_ref().map(|_| slot.stamp))
    }

    pub fn frame_geometry(&self, time: u32) -> Option<GeometryId> {
        self.frames
            .get(time as usize)
            .filter(|slot| slot.frame.is_some())
            .map(|slot| slot.geometry)
    }

    pub fn frame_appearance(&self, time: u32) -> Option<Revision> {
        self.frames
            .get(time as usize)
            .filter(|slot| slot.frame.is_some())
            .map(|slot| slot.appearance)
    }

    pub fn frames(&self) -> impl Iterator<Item = (u32, Arc<ObjectFrame>)> + '_ {
        self.frames.iter().enumerate().filter_map(|(time, slot)| {
            slot.frame
                .as_ref()
                .map(|frame| (time as u32, frame.shared()))
        })
    }

    pub fn relations(&self) -> &[Relation] {
        &self.relations
    }

    pub fn relation(&self, target: ObjectSetId, shift: i32) -> Option<&Relation> {
        self.relations
            .iter()
            .find(|relation| relation.target() == target && relation.shift() == shift)
    }

    pub(crate) fn add_relation(
        &mut self,
        target: ObjectSetId,
        shift: i32,
        stamp: Revision,
    ) -> bool {
        self.add_relation_with_key(target, shift, crate::relation::key_of(target, shift), stamp)
    }

    pub(crate) fn add_relation_with_key(
        &mut self,
        target: ObjectSetId,
        shift: i32,
        key: String,
        stamp: Revision,
    ) -> bool {
        if self.relation(target, shift).is_some()
            || self.relations.iter().any(|relation| relation.key() == key)
        {
            return false;
        }
        self.relations.push(Relation::new(
            target,
            shift,
            key,
            vec![None; self.frame_count() as usize],
        ));
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn move_relation(
        &mut self,
        target: ObjectSetId,
        from: i32,
        to: i32,
        stamp: Revision,
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
        }
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn remove_relation(
        &mut self,
        target: ObjectSetId,
        shift: i32,
        stamp: Revision,
    ) -> bool {
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
        }
        if self.track_coloring.metric().and_then(TrackColor::relation) == Some((target, shift)) {
            self.track_coloring.set_metric(None);
        }
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn remove_relations_to(&mut self, target: ObjectSetId, stamp: Revision) -> bool {
        let count = self.relations.len();
        self.relations
            .retain(|relation| relation.target() != target);
        if self.relations.len() == count {
            return false;
        }
        if self.object_coloring.metric().and_then(ObjectColor::target) == Some(target) {
            self.object_coloring.set_metric(None);
        }
        if self.track_coloring.metric().and_then(TrackColor::target) == Some(target) {
            self.track_coloring.set_metric(None);
        }
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn install_relation(
        &mut self,
        target: ObjectSetId,
        shift: i32,
        time: u32,
        frame: RelationFrame,
        stamp: Revision,
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
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
    }

    pub(crate) fn clear_relations(&mut self, stamp: Revision) {
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
        }
        if self
            .track_coloring
            .metric()
            .and_then(TrackColor::relation)
            .is_some()
        {
            self.track_coloring.set_metric(None);
        }
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
    }

    pub fn detected(&self) -> usize {
        self.frames
            .iter()
            .filter(|slot| slot.frame.is_some())
            .count()
    }

    pub fn frame_count(&self) -> u32 {
        self.frames.len() as u32
    }

    pub fn ids(&self, time: u32) -> ObjectIds {
        self.frames
            .get(time as usize)
            .and_then(FrameSlot::shared)
            .map_or_else(ObjectIds::empty, |frame| frame.ids().clone())
    }

    pub fn id_frames(&self) -> Vec<ObjectIds> {
        (0..self.frames.len())
            .map(|time| self.ids(time as u32))
            .collect()
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn shape(&self) -> Revision {
        self.shape
    }

    pub fn visibility(&self) -> &Visibility {
        &self.visibility
    }

    pub fn document(&self) -> Revision {
        self.document
    }

    pub(crate) fn state(&self) -> SetState {
        SetState {
            id: self.id.raw(),
            key: self.key.clone(),
            kind: self.kind,
            name: self.name.clone(),
            channel: self.channel,
            color: self.color,
            visible: self.visible,
            labels: self.labels,
            label: self.label,
            hidden: Arc::clone(&self.hidden),
            opacity: self.opacity,
            material: self.material,
            params: self.params,
            track_style: self.track_style,
            tracks: self.tracks.as_ref().map(TrackLayer::shared_set),
            object_color: self.object_coloring.state(),
            track_color: self.track_coloring.state(),
            classifier: self.classifier.as_ref().map(Classifier::state),
            object_coloc: self.object_coloc.clone(),
            label_export: self.label_export.clone(),
            localization: self.localization.clone(),
            seeds: self.seeds.clone(),
            frames: self.frames.iter().map(|slot| slot.frame.clone()).collect(),
            relations: self
                .relations
                .iter()
                .map(|relation| RelationState {
                    target: relation.target().raw(),
                    shift: relation.shift(),
                    key: relation.key().to_string(),
                    frames: relation.shared_frames().to_vec(),
                })
                .collect(),
            visibility: self.visibility.clone(),
            provenance: self.provenance.clone(),
            version: self.version,
            curation: Arc::clone(&self.curation),
            document: self.document,
        }
    }

    pub(crate) fn restore(state: SetState, dataset: &Dataset, stamps: &mut Revisions) -> Self {
        let SetState {
            id,
            key,
            kind,
            name,
            channel,
            color,
            visible,
            labels,
            label,
            hidden,
            opacity,
            material,
            params,
            track_style,
            tracks,
            object_color,
            track_color,
            classifier,
            object_coloc,
            label_export,
            localization,
            seeds,
            frames,
            relations,
            visibility,
            provenance,
            version,
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
        assert!(
            tracks.is_none() || track_style.is_some(),
            "a restored track layer carries track parameters"
        );
        let frame_count = frames.len();
        let mut set = Self {
            id: ObjectSetId::new(id),
            key,
            kind,
            name,
            channel,
            channels: dataset.channels().len(),
            color,
            visible,
            labels,
            label,
            hidden: Arc::new(Vec::new()),
            hidden_total: 0,
            opacity,
            material,
            voxel: dataset.voxel_size().min_element(),
            params,
            track_style,
            frames: (0..frame_count).map(|_| FrameSlot::new()).collect(),
            store: None,
            installed_spots: 0,
            object_coloring: Coloring::new(),
            track_coloring: Coloring::new(),
            classifier: classifier
                .map(Classifier::restore)
                .transpose()
                .expect("a restored classifier keeps its coefficients"),
            object_coloc: None,
            label_export,
            localization,
            track_bounds: Mutex::new(None),
            revision: stamps.mint(),
            shape: stamps.mint(),
            document: Revision::FIRST,
            visibility: Visibility::All,
            seeds: Seeds::points(frame_count),
            tracks: None,
            relations: Vec::new(),
            provenance,
            version,
            curation,
            source: ObjectIdSource::new(),
            replaced: None,
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
                assert!(
                    matches!(kind, ObjectKind::Surfaces | ObjectKind::Cells),
                    "only a surface or a cell set seeds from another object set"
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
                set.install_frame(time as u32, frame, stamps.mint());
            }
        }
        for slot in &set.frames {
            if let Some(frame) = slot.shared() {
                set.source.observe(frame.ids());
            }
        }
        set.settle_pages();
        let curation = Arc::make_mut(&mut set.curation);
        for (time, frame) in set.frames.iter().enumerate() {
            curation.set_applied(time as u32, frame.stamp);
        }
        if let Some(coloc) = object_coloc {
            assert_eq!(
                coloc.frames().len(),
                frame_count,
                "an object colocalization state covers every frame of its set"
            );
            assert!(
                coloc
                    .frames()
                    .iter()
                    .zip(&set.frames)
                    .all(|(values, slot)| values.len() == slot.objects()),
                "an object colocalization state measures every object of its set"
            );
            set.object_coloc = Some(coloc);
        }
        set.restore_hidden(hidden);
        if let Some(channels) = set
            .frames
            .iter()
            .find_map(|slot| slot.shared().map(|frame| frame.objects().channels()))
        {
            assert!(
                channels <= dataset.channels().len(),
                "a restored object set measures the channels of the dataset"
            );
            set.channels = channels;
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
            set.relations.push(Relation::new(
                target,
                relation.shift,
                relation.key,
                relation.frames,
            ));
        }
        if let Some(tracks) = tracks {
            let thickness = set
                .track_style
                .expect("a restored track layer carries track parameters")
                .thickness();
            set.tracks = Some(TrackLayer::shared(tracks, thickness, stamps.mint()));
        }
        set.set_visibility(visibility, stamps.mint());
        set.restore_coloring(object_color, track_color);
        set.document = document;
        set
    }

    fn restore_coloring(&mut self, object: ColorState<ObjectColor>, track: ColorState<TrackColor>) {
        let object = Coloring::restore(object);
        self.object_coloring = object;
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
                        RelationField::CONTINUOUS.contains(&field),
                        "a restored track color names a relation field"
                    );
                }
            }
        }
        self.track_coloring = track;
    }

    pub fn set_visibility(&mut self, visibility: Visibility, stamp: Revision) {
        if self.visibility == visibility {
            return;
        }
        match &visibility {
            Visibility::All | Visibility::Objects(_) => {}
            Visibility::Tracks(ids) => {
                assert!(
                    self.tracks.as_ref().is_some_and(|layer| ids
                        .as_slice()
                        .iter()
                        .all(|id| layer.set().contains(*id))),
                    "the visibility names the tracks of its set"
                );
            }
            Visibility::Molecules { mask, rows } => {
                assert_eq!(
                    mask.rows(),
                    rows.len(),
                    "the visibility covers every molecule"
                );
            }
        }
        self.visibility = visibility;
        self.shape = stamp;
        self.document = Revision::mint();
    }

    pub fn filtered(&self) -> bool {
        self.restricted() || self.localization_filter().active()
    }

    pub fn restricted(&self) -> bool {
        !matches!(self.visibility, Visibility::All) || self.hidden_count() > 0
    }

    pub fn localization_filter(&self) -> grana_object::LocalizationFilter {
        self.localization
            .as_ref()
            .map_or(grana_object::LocalizationFilter::NONE, |source| {
                source.spec().filter()
            })
    }

    pub fn filtered_object(&self, time: u32, object: usize) -> bool {
        match &self.visibility {
            Visibility::All => true,
            Visibility::Objects(ids) => self
                .frame_shared(time)
                .and_then(|frame| frame.id_of(object))
                .is_some_and(|id| ids.contains(id)),
            Visibility::Tracks(ids) => self
                .frame_shared(time)
                .and_then(|frame| frame.id_of(object))
                .and_then(|id| self.track_at(time, id))
                .is_some_and(|track| ids.contains(track)),
            Visibility::Molecules { mask, rows } => self
                .frames
                .get(time as usize)
                .and_then(FrameSlot::spot_frame)
                .and_then(|spots| spots.spots().get(object).and_then(|spot| spot.group()))
                .and_then(|group| rows.get(&group).copied())
                .is_some_and(|row| mask.contains(row as usize)),
        }
    }

    pub fn visible_object(&self, time: u32, object: usize) -> bool {
        self.filtered_object(time, object)
            && !self.hidden_object(time, object)
            && self.localized_object(time, object)
    }

    fn localized_object(&self, time: u32, object: usize) -> bool {
        let filter = self.localization_filter();
        if !filter.active() {
            return true;
        }
        match self
            .frames
            .get(time as usize)
            .and_then(|slot| slot.spot_frame())
        {
            Some(set) => set
                .spots()
                .get(object)
                .is_none_or(|spot| filter.keeps(spot.quality(), spot.sigma())),
            None => true,
        }
    }

    pub fn visible_track(&self, track: TrackId) -> bool {
        match &self.visibility {
            Visibility::Tracks(ids) => ids.contains(track),
            Visibility::All | Visibility::Objects(_) | Visibility::Molecules { .. } => true,
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

    pub(crate) fn add_seed(&mut self, time: u32, position: [f32; 3], stamp: Revision) -> bool {
        if self.kind != ObjectKind::Surfaces {
            return false;
        }
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
        self.revision = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn remove_seed(&mut self, time: u32, index: usize, stamp: Revision) -> bool {
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
        self.revision = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn clear_seeds(&mut self, time: u32, stamp: Revision) -> bool {
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
        self.revision = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn manual_seed_frames(&self) -> Option<&[Vec<[f32; 3]>]> {
        self.seeds.manual()
    }

    pub(crate) fn set_seed_frames(&mut self, seeds: Vec<Vec<[f32; 3]>>, stamp: Revision) -> bool {
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
        self.revision = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn retain_objects(
        &mut self,
        plan: &[(u32, Vec<bool>)],
        stamp: Revision,
    ) -> Result<usize, String> {
        let mut pruned = Vec::with_capacity(plan.len());
        for (time, keep) in plan {
            let Some(frame) = self
                .frames
                .get(*time as usize)
                .and_then(|slot| slot.shared())
            else {
                return Err(format!("the object set holds no frame {time}"));
            };
            if keep.len() != frame.object_count() {
                return Err(format!(
                    "the frame {time} holds {} objects and {} are named",
                    frame.object_count(),
                    keep.len()
                ));
            }
            if keep.iter().all(|kept| *kept) {
                continue;
            }
            let retained = frame.retain(keep).map_err(|error| {
                format!("the objects of frame {time} cannot be dropped: {error}")
            })?;
            pruned.push((*time, retained, keep, frame));
        }
        if pruned.is_empty() {
            return Ok(0);
        }
        if !self.curation.is_empty() {
            self.apply_curation(stamp)?;
        }
        {
            let curation = Arc::make_mut(&mut self.curation);
            curation.seed(self.frames.len());
            for (time, _, keep, frame) in &pruned {
                curation.record(*time, keep, &frame.anchors());
            }
        }
        let mut removed = 0usize;
        for (time, retained, keep, frame) in pruned {
            let ids = removed_ids(&frame, keep);
            removed += ids.len();
            self.install_retained(time, retained, &ids, stamp);
        }
        Ok(removed)
    }

    pub fn curation(&self) -> &Curation {
        &self.curation
    }

    pub(crate) fn set_curation(&mut self, curation: Curation) -> bool {
        if self.curation.as_ref() == &curation {
            return false;
        }
        self.curation = Arc::new(curation);
        self.document = Revision::mint();
        true
    }

    pub(crate) fn apply_curation(&mut self, stamp: Revision) -> Result<usize, String> {
        let mut keeps = Vec::new();
        let mut pending = 0;
        for time in 0..self.frames.len() {
            let slot = &self.frames[time];
            if self.curation.applied(time as u32) == slot.stamp {
                continue;
            }
            let anchors = slot.shared().map_or_else(Vec::new, |frame| frame.anchors());
            if let Some(kept) = self.curation.keep(time as u32, &anchors, self.voxel) {
                pending += kept.pending;
                if kept.keep.iter().any(|kept| !kept) {
                    keeps.push((time as u32, kept.keep));
                }
            }
        }
        let mut pruned = Vec::with_capacity(keeps.len());
        for (time, keep) in keeps {
            let Some(frame) = self.frames[time as usize].shared() else {
                continue;
            };
            let retained = frame.retain(&keep).map_err(|error| {
                format!("the edits of frame {time} cannot drop the objects they hide: {error}")
            })?;
            let ids = removed_ids(&frame, &keep);
            pruned.push((time, ids, retained));
        }
        let mut applied = 0usize;
        for (time, ids, retained) in pruned {
            applied += ids.len();
            self.install_retained(time, retained, &ids, stamp);
        }
        let curation = Arc::make_mut(&mut self.curation);
        for (time, frame) in self.frames.iter().enumerate() {
            curation.set_applied(time as u32, frame.stamp);
        }
        self.reapply_hidden(stamp);
        Arc::make_mut(&mut self.curation).set_pending(pending);
        Ok(applied)
    }

    pub(crate) fn reapply_hidden(&mut self, stamp: Revision) -> usize {
        let mut hidden = 0;
        for time in 0..self.frames.len() {
            let Some(frame) = self.frame_shared(time as u32) else {
                continue;
            };
            let targets = self
                .curation
                .hidden_targets(time as u32, &frame.anchors(), self.voxel);
            let ids = targets
                .into_iter()
                .filter_map(|object| frame.id_of(object))
                .collect::<Vec<_>>();
            if ids.is_empty() {
                continue;
            }
            hidden += ids.len();
            self.hide(time as u32, &ids, true, stamp);
        }
        hidden
    }

    pub(crate) fn apply_edits(&mut self, stamp: Revision) -> usize {
        if self.curation.edits().is_empty() {
            return 0;
        }
        if self.tracks.is_none() {
            let dropped = Arc::make_mut(&mut self.curation).forget_edits();
            self.document = Revision::mint();
            return dropped;
        }
        let Some(thickness) = self.track_thickness() else {
            return 0;
        };
        let edits = self.curation.edits().to_vec();
        let mut applied = 0usize;
        let mut dropped = Vec::new();
        for (index, edit) in edits.into_iter().enumerate() {
            let Some(edit) = self.resolve_edit(edit) else {
                dropped.push(index);
                continue;
            };
            let Some(layer) = self.tracks.as_mut() else {
                break;
            };
            let done = match edit {
                TrackEdit::Cut(node) => layer.cut(node.time(), node.id(), thickness, stamp),
                TrackEdit::Join { source, target } => layer.join(
                    (source.time(), source.id()),
                    (target.time(), target.id()),
                    thickness,
                    stamp,
                ),
                TrackEdit::Divide {
                    parent,
                    first,
                    second,
                } => layer.divide(
                    (parent.time(), parent.id()),
                    (first.time(), first.id()),
                    (second.time(), second.id()),
                    thickness,
                    stamp,
                ),
                TrackEdit::Merge {
                    first,
                    second,
                    child,
                } => layer.merge(
                    (first.time(), first.id()),
                    (second.time(), second.id()),
                    (child.time(), child.id()),
                    thickness,
                    stamp,
                ),
                TrackEdit::Reparent { child, parent } => layer.reparent(
                    (child.time(), child.id()),
                    (parent.time(), parent.id()),
                    thickness,
                    stamp,
                ),
                TrackEdit::Detach { child } => {
                    layer.detach((child.time(), child.id()), thickness, stamp)
                }
            };
            if done {
                applied += 1;
            } else {
                dropped.push(index);
            }
        }
        if !dropped.is_empty() {
            let curation = Arc::make_mut(&mut self.curation);
            for index in dropped.into_iter().rev() {
                curation.remove_edit(index);
            }
        }
        if applied > 0 || self.curation.dropped() > 0 {
            self.shape = stamp;
            self.document = Revision::mint();
        }
        applied
    }

    fn resolve_edit(&self, edit: TrackEdit) -> Option<TrackEdit> {
        let resolve = |node: TrackAnchor| -> Option<TrackAnchor> {
            let frame = self.frames.get(node.time() as usize)?.shared()?;
            let id = match frame.index_of(node.id()) {
                Some(_) => node.id(),
                None => {
                    frame.id_of(match_anchor(&frame.anchors(), node.position(), self.voxel)?)?
                }
            };
            Some(TrackAnchor::new(node.time(), id, node.position()))
        };
        match edit {
            TrackEdit::Cut(node) => Some(TrackEdit::Cut(resolve(node)?)),
            TrackEdit::Join { source, target } => Some(TrackEdit::Join {
                source: resolve(source)?,
                target: resolve(target)?,
            }),
            TrackEdit::Divide {
                parent,
                first,
                second,
            } => Some(TrackEdit::Divide {
                parent: resolve(parent)?,
                first: resolve(first)?,
                second: resolve(second)?,
            }),
            TrackEdit::Merge {
                first,
                second,
                child,
            } => Some(TrackEdit::Merge {
                first: resolve(first)?,
                second: resolve(second)?,
                child: resolve(child)?,
            }),
            TrackEdit::Reparent { child, parent } => Some(TrackEdit::Reparent {
                child: resolve(child)?,
                parent: resolve(parent)?,
            }),
            TrackEdit::Detach { child } => Some(TrackEdit::Detach {
                child: resolve(child)?,
            }),
        }
    }

    pub(crate) fn edit_track(&mut self, edit: TrackEdit, stamp: Revision) -> bool {
        let Some(thickness) = self.track_thickness() else {
            return false;
        };
        let Some(layer) = self.tracks.as_mut() else {
            return false;
        };
        let done = match edit {
            TrackEdit::Cut(node) => layer.cut(node.time(), node.id(), thickness, stamp),
            TrackEdit::Join { source, target } => layer.join(
                (source.time(), source.id()),
                (target.time(), target.id()),
                thickness,
                stamp,
            ),
            TrackEdit::Divide {
                parent,
                first,
                second,
            } => layer.divide(
                (parent.time(), parent.id()),
                (first.time(), first.id()),
                (second.time(), second.id()),
                thickness,
                stamp,
            ),
            TrackEdit::Merge {
                first,
                second,
                child,
            } => layer.merge(
                (first.time(), first.id()),
                (second.time(), second.id()),
                (child.time(), child.id()),
                thickness,
                stamp,
            ),
            TrackEdit::Reparent { child, parent } => layer.reparent(
                (child.time(), child.id()),
                (parent.time(), parent.id()),
                thickness,
                stamp,
            ),
            TrackEdit::Detach { child } => {
                layer.detach((child.time(), child.id()), thickness, stamp)
            }
        };
        if !done {
            return false;
        }
        let curation = Arc::make_mut(&mut self.curation);
        curation.seed(self.frames.len());
        curation.record_edit(edit);
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        self.retain_track_visibility();
        true
    }

    fn install_retained(
        &mut self,
        time: u32,
        pruned: Option<ObjectFrame>,
        removed: &[ObjectId],
        stamp: Revision,
    ) {
        self.discard_object_coloc();
        let empty = pruned.as_ref().is_none_or(ObjectFrame::is_empty);
        let pruned_tracks = self
            .tracks
            .as_ref()
            .map(|layer| layer.set().without_objects(time, removed));
        let kept = pruned
            .as_ref()
            .map_or_else(ObjectIds::empty, |frame| frame.ids().clone());
        let placed = if empty {
            None
        } else {
            pruned.map(|frame| self.manage(Arc::new(frame)))
        };
        let slot = self
            .frames
            .get_mut(time as usize)
            .expect("the pruned object lies inside the time series");
        slot.stamp = stamp;
        slot.geometry = GeometryId::mint();
        slot.appearance = Revision::mint();
        slot.frame = placed;
        self.hide_retained(time, &kept);
        if let Some((thickness, pruned)) = self.track_thickness().zip(pruned_tracks.flatten()) {
            self.tracks = (!pruned.is_empty()).then(|| TrackLayer::new(pruned, thickness, stamp));
        }
        let id = self.id;
        for relation in &mut self.relations {
            relation.invalidate(time);
            if relation.target() == id
                && let Some(source) = relation.source_of(time)
            {
                relation.invalidate(source);
            }
        }
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        self.reset_object_visibility();
    }

    fn hide_retained(&mut self, time: u32, kept: &ObjectIds) {
        let hidden = Arc::make_mut(&mut self.hidden);
        if let Some(held) = hidden.get_mut(time as usize) {
            *held = ObjectIds::new(
                held.as_slice()
                    .iter()
                    .copied()
                    .filter(|id| kept.contains(*id))
                    .collect::<Vec<_>>(),
            );
        }
        self.recount_hidden();
    }

    pub(crate) fn duplicate(
        &self,
        id: ObjectSetId,
        key: ObjectSetKey,
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
                    relation.key().to_string(),
                    vec![None; self.frames.len()],
                )
            })
            .collect::<Vec<_>>();
        let mut object_coloring = self.object_coloring.clone();
        if matches!(object_coloring.metric(), Some(ObjectColor::Coloc { .. })) {
            object_coloring.set_metric(None);
        }
        let label = self
            .label
            .filter(|metric| !matches!(metric, ObjectColor::Coloc { .. }));
        let mut duplicate = Self {
            id,
            key,
            kind: self.kind,
            name,
            channel: self.channel,
            channels: self.channels,
            color,
            visible: self.visible,
            labels: self.labels,
            label,
            hidden: Arc::clone(&self.hidden),
            hidden_total: self.hidden_total,
            opacity: self.opacity,
            material: self.material,
            voxel: self.voxel,
            params: self.params,
            track_style: self.track_style,
            frames: self
                .frames
                .iter()
                .map(|slot| FrameSlot {
                    stamp: Revision::ZERO,
                    geometry: slot.geometry,
                    appearance: slot.appearance,
                    frame: slot.frame.clone(),
                })
                .collect(),
            store: self.store.clone(),
            installed_spots: self.installed_spots,
            object_coloring,
            track_coloring: self.track_coloring.clone(),
            classifier: self.classifier.clone(),
            object_coloc: None,
            label_export: None,
            localization: self.localization.clone(),
            track_bounds: Mutex::new(None),
            revision: Revision::FIRST,
            shape: Revision::FIRST,
            document: Revision::FIRST,
            visibility: Visibility::All,
            seeds: self.seeds.clone(),
            tracks: None,
            relations,
            version: provenance.algorithm().version(),
            provenance,
            curation: Arc::new(Curation::default()),
            source: ObjectIdSource::new(),
            replaced: None,
        };
        for slot in &duplicate.frames {
            if let Some(frame) = slot.shared() {
                duplicate.source.observe(frame.ids());
            }
        }
        if duplicate.hidden.iter().any(|hidden| !hidden.is_empty()) {
            for time in 0..duplicate.frames.len() {
                duplicate.record_hidden(time as u32);
            }
        }
        duplicate
    }

    pub(crate) fn merged(&self, other: &Self, stamp: Revision) -> Result<Self, String> {
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
        let mut renumber = HashMap::new();
        let mut absorbed_frames = Vec::with_capacity(other.frames.len());
        for slot in &other.frames {
            let Some(frame) = slot.frame.as_ref().map(Frame::shared) else {
                absorbed_frames.push(None);
                continue;
            };
            let ids = self.source.allocate(frame.object_count());
            for (held, next) in frame.ids().as_slice().iter().zip(ids.as_slice()) {
                renumber.insert(*held, *next);
            }
            absorbed_frames.push(Some(Arc::new(frame.reidentified(ids))));
        }
        let mut frames = Vec::with_capacity(self.frames.len());
        for ((left, right), absorbed) in self.frames.iter().zip(&other.frames).zip(absorbed_frames)
        {
            let frame = match (left.frame.as_ref().map(Frame::shared), absorbed) {
                (Some(carried), Some(absorbed)) => {
                    if carried.level() != absorbed.level() {
                        return Err(format!(
                            "the object set {} detects another resolution",
                            other.name
                        ));
                    }
                    Some(Frame::resident(Arc::new(carried.merge(&absorbed)?)))
                }
                (Some(_), None) => left.frame.clone(),
                (None, Some(absorbed)) => Some(Frame::resident(absorbed)),
                (None, None) => None,
            };
            let _ = right;
            frames.push(FrameSlot {
                stamp,
                geometry: GeometryId::mint(),
                appearance: Revision::mint(),
                frame,
            });
        }
        let absorbed_hidden = other
            .hidden
            .iter()
            .map(|hidden| {
                ObjectIds::new(
                    hidden
                        .as_slice()
                        .iter()
                        .map(|id| {
                            renumber.get(id).copied().unwrap_or_else(|| {
                                panic!("a hidden object stands in the frames it names")
                            })
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let tracks = match (&self.tracks, &other.tracks) {
            (None, None) => None,
            (Some(layer), None) => Some(layer.set().clone()),
            (None, Some(layer)) => Some(layer.set().renumbered(|id| {
                renumber
                    .get(&id)
                    .copied()
                    .unwrap_or_else(|| panic!("a tracked object stands in the frames it names"))
            })),
            (Some(layer), Some(absorbed)) => {
                Some(layer.set().append(&absorbed.set().renumbered(|id| {
                    renumber
                        .get(&id)
                        .copied()
                        .unwrap_or_else(|| panic!("a tracked object stands in the frames it names"))
                })))
            }
        };
        let tracks = self
            .track_thickness()
            .zip(tracks)
            .map(|(thickness, set)| TrackLayer::new(set, thickness, stamp));
        let mut object_coloring = self.object_coloring.clone();
        if object_coloring.metric().and_then(ObjectColor::target) == Some(other.id)
            || matches!(object_coloring.metric(), Some(ObjectColor::Coloc { .. }))
        {
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
                    relation.key().to_string(),
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
            let mut key = relation.key().to_string();
            let base = key.clone();
            let mut ordinal = 2;
            while relations.iter().any(|existing| existing.key() == key) {
                key = format!("{base}_{ordinal}");
                ordinal += 1;
            }
            relations.push(Relation::new(
                relation.target(),
                relation.shift(),
                key,
                vec![None; self.frames.len()],
            ));
        }
        let seeds = self.seeds.clone();
        let hidden = self.combined_hidden(&absorbed_hidden, &frames);
        let hidden_total = hidden.iter().map(ObjectIds::len).sum();
        let label = self
            .label
            .filter(|metric| !matches!(metric, ObjectColor::Coloc { .. }));
        let mut merged = Self {
            id: self.id,
            key: self.key.clone(),
            kind: self.kind,
            name: self.name.clone(),
            channel: self.channel,
            channels: self.channels,
            color: self.color,
            visible: self.visible,
            labels: self.labels,
            label,
            hidden: Arc::new(hidden),
            hidden_total,
            opacity: self.opacity,
            material: self.material,
            voxel: self.voxel,
            params: self.params,
            track_style: self.track_style.or(other.track_style),
            frames,
            store: self.store.clone().or_else(|| other.store.clone()),
            installed_spots: self.installed_spots + other.installed_spots,
            object_coloring,
            track_coloring,
            classifier: self.classifier.clone(),
            object_coloc: None,
            label_export: None,
            localization: None,
            track_bounds: Mutex::new(None),
            revision: stamp,
            shape: stamp,
            document: Revision::mint(),
            visibility: Visibility::All,
            seeds,
            tracks,
            relations,
            provenance: self.provenance.clone(),
            version: self.version,
            curation: Arc::new(self.curation.combined(&other.curation)),
            source: self.source.clone(),
            replaced: None,
        };
        if let Visibility::Tracks(ids) = &self.visibility {
            merged.visibility = Visibility::Tracks(ids.clone());
            merged.retain_track_visibility();
        }
        Ok(merged)
    }

    fn combined_hidden(&self, absorbed: &[ObjectIds], frames: &[FrameSlot]) -> Vec<ObjectIds> {
        if self.hidden.is_empty()
            && absorbed.is_empty()
            && frames.iter().all(|slot| slot.objects() == 0)
        {
            return Vec::new();
        }
        let mut hidden = Vec::with_capacity(frames.len());
        for time in 0..frames.len() {
            let mut ids = self
                .hidden
                .get(time)
                .cloned()
                .unwrap_or_else(ObjectIds::empty)
                .as_slice()
                .to_vec();
            if let Some(source) = absorbed.get(time) {
                ids.extend_from_slice(source.as_slice());
            }
            hidden.push(ObjectIds::new(ids));
        }
        hidden
    }

    pub(crate) fn invalidate_relation(
        &mut self,
        target: ObjectSetId,
        time: u32,
        stamp: Revision,
    ) -> bool {
        let mut cleared = false;
        for relation in self
            .relations
            .iter_mut()
            .filter(|relation| relation.target() == target)
        {
            let Some(source) = relation.source_of(time) else {
                continue;
            };
            cleared |= relation.invalidate(source);
        }
        if !cleared {
            return false;
        }
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn invalidate_relations(&mut self, time: u32, stamp: Revision) -> bool {
        let id = self.id;
        let mut cleared = false;
        for relation in &mut self.relations {
            cleared |= relation.invalidate(time);
            if relation.target() == id
                && let Some(source) = relation.source_of(time)
            {
                cleared |= relation.invalidate(source);
            }
        }
        if !cleared {
            return false;
        }
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn regroup(&mut self, groups: &[Vec<Option<u32>>], stamps: &mut Revisions) -> bool {
        if self.kind != ObjectKind::Spots || groups.len() != self.frames.len() {
            return false;
        }
        let store = self.store.clone();
        let mut changed = false;
        for (time, slot) in self.frames.iter_mut().enumerate() {
            let Some(spots) = slot.spot_frame() else {
                continue;
            };
            let Some(held) = groups.get(time) else {
                continue;
            };
            if held.len() != spots.len() {
                continue;
            }
            if spots
                .spots()
                .iter()
                .zip(held)
                .all(|(spot, group)| spot.group() == *group)
            {
                continue;
            }
            let stamp = stamps.mint();
            slot.frame = Some(place(
                store.as_ref(),
                Arc::new(ObjectFrame::Spots(Arc::new(spots.regrouped(held)))),
            ));
            slot.stamp = stamp;
            slot.geometry = GeometryId::mint();
            slot.appearance = Revision::mint();
            self.revision = stamp;
            self.shape = stamp;
            self.document = Revision::mint();
            changed = true;
        }
        changed
    }

    pub(crate) fn realign(&mut self, next: &LocalizationAlignment, stamps: &mut Revisions) -> bool {
        let Some(applied) = self
            .localization
            .as_ref()
            .map(|source| source.applied().clone())
        else {
            return false;
        };
        assert_eq!(
            next.channel(),
            self.channel,
            "a localization realigns inside the channel it was measured in"
        );
        if self.kind != ObjectKind::Spots || &applied == next {
            return false;
        }
        let store = self.store.clone();
        let mut changed = false;
        for (time, slot) in self.frames.iter_mut().enumerate() {
            let time = time as u32;
            let previous = applied.warp(time);
            let next = next.warp(time);
            if previous == next {
                continue;
            }
            let Some(spots) = slot.spot_frame() else {
                continue;
            };
            let aligned = spots
                .spots()
                .iter()
                .map(|spot| {
                    let center = spot.center();
                    let offset = previous.sample(center);
                    let raw = [0, 1, 2].map(|axis| center[axis] + offset[axis]);
                    let offset = next.sample(raw);
                    spot.moved([0, 1, 2].map(|axis| raw[axis] - offset[axis]))
                })
                .collect::<Vec<_>>();
            let stamp = stamps.mint();
            slot.frame = Some(place(
                store.as_ref(),
                Arc::new(ObjectFrame::Spots(Arc::new(SpotSet::new(
                    spots.level(),
                    spots.ids().clone(),
                    aligned,
                    spots.intensities().clone(),
                )))),
            ));
            slot.stamp = stamp;
            slot.geometry = GeometryId::mint();
            slot.appearance = Revision::mint();
            self.revision = stamp;
            self.shape = stamp;
            self.document = Revision::mint();
            changed = true;
        }
        if !changed {
            return false;
        }
        if let Some(source) = self.localization.as_mut() {
            source.set_applied(next.clone());
        }
        self.move_tracks(stamps);
        true
    }

    fn move_tracks(&mut self, stamps: &mut Revisions) {
        let Some(thickness) = self.track_thickness() else {
            return;
        };
        let Some(layer) = self.tracks.take() else {
            return;
        };
        let frames = &self.frames;
        let remapped = layer.set().remapped(|time, id, centroid| {
            let Some(spots) = frames.get(time as usize).and_then(|slot| slot.spot_frame()) else {
                return centroid;
            };
            spots
                .index_of(id)
                .map_or(centroid, |object| spots.spots()[object].center())
        });
        let stamp = stamps.mint();
        self.tracks = Some(TrackLayer::new(remapped, thickness, stamp));
        self.revision = stamp;
        self.shape = stamp;
    }

    pub(crate) fn install_shared(
        &mut self,
        time: u32,
        frame: Arc<ObjectFrame>,
        stamp: Revision,
    ) -> Readoption {
        assert_eq!(
            frame.kind(),
            self.kind,
            "an object frame matches its object set"
        );
        let (frame, report) = self.adopt(time, Some(frame));
        let placed = frame.map(|frame| self.manage(frame));
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        self.reset_object_visibility();
        self.refresh_appearance();
        let slot = self
            .frames
            .get_mut(time as usize)
            .expect("the object frame lies inside the time series");
        slot.stamp = stamp;
        slot.geometry = GeometryId::mint();
        slot.frame = placed;
        report
    }

    pub(crate) fn install_optional(
        &mut self,
        time: u32,
        frame: Option<Arc<ObjectFrame>>,
        stamp: Revision,
    ) -> Readoption {
        if let Some(frame) = frame.as_ref() {
            assert_eq!(
                frame.kind(),
                self.kind,
                "an object frame matches its object set"
            );
        }
        let (frame, report) = self.adopt(time, frame);
        let placed = frame.map(|frame| self.manage(frame));
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        self.reset_object_visibility();
        self.refresh_appearance();
        let slot = self
            .frames
            .get_mut(time as usize)
            .expect("the object frame lies inside the time series");
        slot.stamp = stamp;
        slot.geometry = GeometryId::mint();
        slot.frame = placed;
        report
    }

    fn adopt(
        &mut self,
        time: u32,
        next: Option<Arc<ObjectFrame>>,
    ) -> (Option<Arc<ObjectFrame>>, Readoption) {
        let held = self
            .replaced
            .as_ref()
            .and_then(|frames| frames.get(time as usize).cloned().flatten())
            .or_else(|| self.frames.get(time as usize).and_then(FrameSlot::shared));
        let held_ids = held
            .as_ref()
            .map_or_else(ObjectIds::empty, |held| held.ids().clone());
        let (next, carried) = match next {
            Some(next) => {
                let inherits = held
                    .as_ref()
                    .filter(|held| !held.is_empty() && !next.is_empty())
                    .map_or_else(
                        || vec![None; next.object_count()],
                        |held| match_objects(held, &next, self.voxel),
                    );
                let mut ids = Vec::with_capacity(next.object_count());
                let mut carried = 0;
                for (object, inherits) in inherits.iter().enumerate() {
                    match inherits {
                        Some(id) => {
                            ids.push(*id);
                            carried += 1;
                        }
                        None => ids.push(
                            next.id_of(object)
                                .expect("an object of a frame carries its id"),
                        ),
                    }
                }
                let ids =
                    ObjectIds::checked(ids).expect("a readopted frame names every object once");
                self.source.observe(&ids);
                (Some(Arc::new(next.reidentified(ids))), carried)
            }
            None => (None, 0),
        };
        let next_ids = next
            .as_ref()
            .map_or_else(ObjectIds::empty, |next| next.ids().clone());
        let mut gone = held_ids
            .as_slice()
            .iter()
            .copied()
            .filter(|id| !next_ids.contains(*id))
            .collect::<Vec<_>>();
        gone.sort_unstable();
        let purged = self.purge(time, &gone);
        (
            next,
            Readoption {
                carried,
                fresh: next_ids.len() - carried,
                vanished: gone.len(),
                purged,
            },
        )
    }

    fn purge(&mut self, time: u32, gone: &[ObjectId]) -> usize {
        let mut purged = 0;
        if let Some(held) = Arc::make_mut(&mut self.hidden).get_mut(time as usize) {
            let before = held.len();
            *held = ObjectIds::new(
                held.as_slice()
                    .iter()
                    .copied()
                    .filter(|id| gone.binary_search(id).is_err())
                    .collect::<Vec<_>>(),
            );
            purged += before - held.len();
        }
        self.recount_hidden();
        if gone.is_empty() {
            return purged;
        }
        let pruned = self
            .tracks
            .as_ref()
            .and_then(|layer| layer.set().without_objects(time, gone));
        if let Some((thickness, pruned)) = self.track_thickness().zip(pruned) {
            self.tracks =
                (!pruned.is_empty()).then(|| TrackLayer::new(pruned, thickness, self.revision));
        }
        purged
    }
    fn manage(&mut self, frame: Arc<ObjectFrame>) -> Frame {
        let spots = match frame.as_ref() {
            ObjectFrame::Spots(spots) => spots.len(),
            ObjectFrame::Surfaces(_) | ObjectFrame::Filaments(_) | ObjectFrame::Cells(_) => 0,
        };
        self.installed_spots += spots;
        if self.store.is_none() && self.installed_spots <= SPOT_PAGE_THRESHOLD {
            return Frame::resident(frame);
        }
        let store = match &self.store {
            Some(store) => Arc::clone(store),
            None => {
                let store = open_store();
                self.store = Some(Arc::clone(&store));
                self.spill(&store);
                store
            }
        };
        place(Some(&store), frame)
    }

    fn settle_pages(&mut self) {
        if self.kind != ObjectKind::Spots || self.store.is_some() {
            return;
        }
        let total = self.frames.iter().map(FrameSlot::objects).sum::<usize>();
        self.installed_spots = total;
        if total <= SPOT_PAGE_THRESHOLD {
            return;
        }
        let store = self
            .frames
            .iter()
            .find_map(|slot| slot.frame.as_ref().and_then(Frame::store))
            .unwrap_or_else(open_store);
        self.store = Some(Arc::clone(&store));
        self.spill(&store);
    }

    fn install_frame(&mut self, time: u32, frame: Frame, stamp: Revision) {
        let slot = self
            .frames
            .get_mut(time as usize)
            .expect("the restored object frame lies inside the time series");
        slot.stamp = stamp;
        slot.geometry = GeometryId::mint();
        slot.appearance = Revision::mint();
        slot.frame = Some(frame);
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
    }

    fn spill(&mut self, store: &Arc<SpillStore>) {
        for slot in &mut self.frames {
            if let Some(Frame::Resident(frame)) = &slot.frame {
                slot.frame = Some(place(Some(store), Arc::clone(frame)));
            }
        }
    }

    pub(crate) fn clear(&mut self, stamp: Revision) {
        self.discard_object_coloc();
        for slot in &mut self.frames {
            slot.frame = None;
            slot.geometry = GeometryId::NONE;
            slot.appearance = Revision::ZERO;
        }
        Arc::make_mut(&mut self.hidden).clear();
        self.hidden_total = 0;
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        self.visibility = Visibility::All;
        self.tracks = None;
    }

    pub fn tracks(&self) -> Option<&TrackLayer> {
        self.tracks.as_ref()
    }

    pub fn track_style(&self) -> Option<TrackStyle> {
        self.track_style
    }

    fn track_thickness(&self) -> Option<f32> {
        self.track_style.map(TrackStyle::thickness)
    }

    fn styled(params: TrackParams, thickness: f32, visible: bool) -> TrackStyle {
        let mut style = TrackStyle::new(params, thickness);
        style.set_visible(visible);
        style
    }

    pub fn set_track_params(&mut self, params: TrackParams) {
        if let Some(style) = self.track_style
            && style.params() != params
        {
            self.track_style = Some(Self::styled(params, style.thickness(), style.visible()));
            self.document = Revision::mint();
        }
    }

    pub fn set_track_style(
        &mut self,
        params: TrackParams,
        thickness: f32,
        stamp: Revision,
    ) -> bool {
        let Some(style) = self.track_style else {
            return false;
        };
        let next = Self::styled(
            params,
            thickness.clamp(self.min_thickness(), self.max_thickness()),
            style.visible(),
        );
        if style == next {
            return false;
        }
        self.track_style = Some(next);
        if let Some(layer) = self.tracks.as_mut() {
            layer.set_thickness(next.thickness(), stamp);
        }
        self.revision = stamp;
        self.document = Revision::mint();
        true
    }

    pub(crate) fn install_tracks(&mut self, set: TrackSet, stamp: Revision) {
        let thickness = self
            .track_thickness()
            .expect("only trackable objects install tracks");
        let set = match &self.tracks {
            Some(layer) => {
                let owner = track_owners(layer.set());
                set.inherited(|track| inherited_track(track, &owner))
            }
            None => set,
        };
        self.tracks = Some(TrackLayer::new(set, thickness, stamp));
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        self.apply_edits(stamp);
        self.retain_track_visibility();
    }

    fn retain_track_visibility(&mut self) {
        let Visibility::Tracks(ids) = &self.visibility else {
            return;
        };
        let Some(set) = self.tracks.as_ref().map(TrackLayer::set) else {
            self.visibility = Visibility::All;
            return;
        };
        let kept = ids
            .as_slice()
            .iter()
            .copied()
            .filter(|id| set.contains(*id))
            .collect::<Vec<_>>();
        if kept.len() == ids.len() {
            return;
        }
        self.visibility = Visibility::Tracks(TrackIds::new(kept));
    }

    fn reset_object_visibility(&mut self) {
        match &self.visibility {
            Visibility::Tracks(_) => self.retain_track_visibility(),
            Visibility::All | Visibility::Objects(_) | Visibility::Molecules { .. } => {
                self.visibility = Visibility::All
            }
        }
    }

    pub(crate) fn clear_tracks(&mut self, stamp: Revision) {
        self.revision = stamp;
        self.shape = stamp;
        self.document = Revision::mint();
        if matches!(self.visibility, Visibility::Tracks(_)) {
            self.visibility = Visibility::All;
        }
        self.tracks = None;
    }

    pub(crate) fn cut_track(&mut self, time: u32, id: ObjectId, stamp: Revision) -> bool {
        let Some(position) = self.anchor_at(time, id) else {
            return false;
        };
        self.edit_track(TrackEdit::Cut(TrackAnchor::new(time, id, position)), stamp)
    }

    pub(crate) fn join_tracks(
        &mut self,
        source: (u32, ObjectId),
        target: (u32, ObjectId),
        stamp: Revision,
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

    pub(crate) fn divide_tracks(
        &mut self,
        parent: (u32, ObjectId),
        first: (u32, ObjectId),
        second: (u32, ObjectId),
        stamp: Revision,
    ) -> bool {
        let (Some(parent_position), Some(first_position), Some(second_position)) = (
            self.anchor_at(parent.0, parent.1),
            self.anchor_at(first.0, first.1),
            self.anchor_at(second.0, second.1),
        ) else {
            return false;
        };
        self.edit_track(
            TrackEdit::Divide {
                parent: TrackAnchor::new(parent.0, parent.1, parent_position),
                first: TrackAnchor::new(first.0, first.1, first_position),
                second: TrackAnchor::new(second.0, second.1, second_position),
            },
            stamp,
        )
    }

    pub(crate) fn merge_tracks(
        &mut self,
        first: (u32, ObjectId),
        second: (u32, ObjectId),
        child: (u32, ObjectId),
        stamp: Revision,
    ) -> bool {
        let (Some(first_position), Some(second_position), Some(child_position)) = (
            self.anchor_at(first.0, first.1),
            self.anchor_at(second.0, second.1),
            self.anchor_at(child.0, child.1),
        ) else {
            return false;
        };
        self.edit_track(
            TrackEdit::Merge {
                first: TrackAnchor::new(first.0, first.1, first_position),
                second: TrackAnchor::new(second.0, second.1, second_position),
                child: TrackAnchor::new(child.0, child.1, child_position),
            },
            stamp,
        )
    }

    pub(crate) fn reparent_track(
        &mut self,
        child: (u32, ObjectId),
        parent: (u32, ObjectId),
        stamp: Revision,
    ) -> bool {
        let (Some(child_position), Some(parent_position)) = (
            self.anchor_at(child.0, child.1),
            self.anchor_at(parent.0, parent.1),
        ) else {
            return false;
        };
        self.edit_track(
            TrackEdit::Reparent {
                child: TrackAnchor::new(child.0, child.1, child_position),
                parent: TrackAnchor::new(parent.0, parent.1, parent_position),
            },
            stamp,
        )
    }

    pub(crate) fn detach_track(&mut self, child: (u32, ObjectId), stamp: Revision) -> bool {
        let Some(child_position) = self.anchor_at(child.0, child.1) else {
            return false;
        };
        self.edit_track(
            TrackEdit::Detach {
                child: TrackAnchor::new(child.0, child.1, child_position),
            },
            stamp,
        )
    }

    fn anchor_at(&self, time: u32, id: ObjectId) -> Option<[f32; 3]> {
        let frame = self.frames.get(time as usize)?.shared()?;
        let object = frame.index_of(id)?;
        Some(frame.anchors().get(object)?.position())
    }

    fn cached_track_bounds(&self) -> Option<TrackBounds> {
        *self.lock_track_bounds()
    }

    fn lock_track_bounds(&self) -> MutexGuard<'_, Option<TrackBounds>> {
        self.track_bounds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn track_bounds_of(&self, metric: TrackColor) -> Option<Extent> {
        let set = self.tracks.as_ref()?.set();
        set.tracks().iter().fold(None, |accumulated, track| {
            accumulate(accumulated, self.track_value(track.id(), metric))
        })
    }

    pub fn min_thickness(&self) -> f32 {
        MIN_THICKNESS * self.voxel
    }

    pub fn max_thickness(&self) -> f32 {
        MAX_THICKNESS * self.voxel
    }

    pub(crate) fn set_track_thickness(&mut self, thickness: f32, stamp: Revision) {
        let Some(style) = self.track_style else {
            return;
        };
        let next = Self::styled(
            style.params(),
            thickness.clamp(self.min_thickness(), self.max_thickness()),
            style.visible(),
        );
        if style == next {
            return;
        }
        self.track_style = Some(next);
        if let Some(layer) = self.tracks.as_mut() {
            layer.set_thickness(next.thickness(), stamp);
        }
        self.revision = stamp;
        self.document = Revision::mint();
    }

    pub fn track_visible(&self) -> bool {
        self.track_style.is_none_or(TrackStyle::visible)
    }

    pub fn set_track_visible(&mut self, visible: bool) {
        let Some(style) = self.track_style.as_mut() else {
            return;
        };
        let previous = style.visible();
        style.set_visible(visible);
        if style.visible() != previous {
            self.document = Revision::mint();
        }
    }

    pub fn track_at(&self, time: u32, id: ObjectId) -> Option<TrackId> {
        self.tracks.as_ref()?.set().track_at(time, id)
    }
}

fn track_owners(set: &TrackSet) -> HashMap<ObjectId, TrackId> {
    let mut owner = HashMap::with_capacity(set.node_count());
    for track in set.tracks() {
        for node in track.nodes() {
            owner.insert(node.id(), track.id());
        }
    }
    owner
}

fn inherited_track(
    track: &grana_object::Track,
    owner: &HashMap<ObjectId, TrackId>,
) -> Option<TrackId> {
    let mut counts = HashMap::<TrackId, usize>::new();
    for node in track.nodes() {
        if let Some(id) = owner.get(&node.id()) {
            *counts.entry(*id).or_default() += 1;
        }
    }
    let mut best = None;
    let mut tied = false;
    for (id, count) in counts {
        match best {
            None => best = Some((count, id)),
            Some((held, _)) if count > held => {
                best = Some((count, id));
                tied = false;
            }
            Some((held, _)) if count == held => tied = true,
            Some(_) => {}
        }
    }
    match best {
        Some((_, id)) if !tied => Some(id),
        _ => None,
    }
}

fn match_objects(held: &ObjectFrame, next: &ObjectFrame, voxel: f32) -> Vec<Option<ObjectId>> {
    let anchors = held.anchors();
    let reach = |anchor: &grana_object::ObjectAnchor| {
        let size = anchor.bounds().size();
        [0, 1, 2].map(|axis| (0.5 * size[axis]).max(0.5 * voxel))
    };
    let bvh = grana_geometry::Bvh::new(anchors.iter().enumerate().map(|(object, anchor)| {
        (
            object as u32,
            grana_geometry::Primitive::ellipsoid(grana_geometry::Ellipsoid::new(
                anchor.position(),
                reach(anchor),
            )),
        )
    }));
    let mut claims = Vec::new();
    for (object, anchor) in next.anchors().iter().enumerate() {
        let Some((distance, owner)) = bvh.nearest(glam::Vec3::from(anchor.position())) else {
            continue;
        };
        if distance > voxel {
            continue;
        }
        let Some(id) = held.id_of(owner as usize) else {
            continue;
        };
        claims.push((distance, object, id));
    }
    claims.sort_unstable_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    let mut matched = vec![None; next.object_count()];
    let mut taken = std::collections::BTreeSet::new();
    for (_, object, id) in claims {
        if taken.insert(id) {
            matched[object] = Some(id);
        }
    }
    matched
}

fn removed_ids(frame: &ObjectFrame, keep: &[bool]) -> Vec<ObjectId> {
    let mut ids = keep
        .iter()
        .enumerate()
        .filter(|(_, kept)| !**kept)
        .filter_map(|(object, _)| frame.id_of(object))
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
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
