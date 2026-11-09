use grana_animation::{Keyframe, Timeline};
use grana_display::{CameraState, Palette, SectionState, Shading, Stop, TransferFunction};
use grana_geometry::{Bounds, PackedMesh};
use grana_measure::{
    ChildField, ChildMetric, ClassifierState, Filter, Measurement, TrackMeasurement,
};
use grana_measure::{
    ColocCost, ColocFrame, ColocMoments, ColocPair, ColocSeries, ColocSeriesInfo, ColocSettings,
    ColocSignificance, ColocThresholds, ObjectColoc, ObjectColocValue,
};
use grana_object::{
    FilamentSet, FilamentStatistics, FilamentTopology, IntensityTable, RelationFrame,
    SampleStatistics, Spot, SpotSet, SurfaceSet, SurfaceStatistics, Track, TrackNode, TrackSet,
};
use grana_scene::{
    Annotation, AnnotationKind, AnnotationState, ChannelState, ColocResult, ColocState, ColorState,
    Criteria, Curation, DerivedSpec, Drift, FilterRange, ItemState, LabelExport, LassoPath,
    MeasureState, ObjectColor, ObjectFrame, ObjectParams, Provenance, Recipe, RelationState,
    SceneState, Seeds, SelectionState, SetState, TrackAnchor, TrackColor, TrackEdit, TrackStyle,
    Visibility,
};
use grana_volume::{Clip, Fingerprint, Intensity, Region};

use grana_codec::Encoder;

use crate::codec::Encode;
use crate::project::{Export, NamedView, Project, Shape, Source, ViewState};

pub(crate) const TAG_SPOTS: u8 = 0;
pub(crate) const TAG_SURFACES: u8 = 1;
pub(crate) const TAG_FILAMENTS: u8 = 2;

impl Encode for Project {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.shape.encode(writer);
        self.source.encode(writer);
        self.fingerprint.encode(writer);
        self.scene.encode(writer);
        self.measure.encode(writer);
        self.view.encode(writer);
        self.views.encode(writer);
    }
}

impl Encode for NamedView {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.string(&self.name);
        self.view.encode(writer);
    }
}

impl Encode for Fingerprint {
    fn encode(&self, writer: &mut Encoder<'_>) {
        for byte in self.bytes() {
            writer.u8(byte);
        }
    }
}

impl Encode for DerivedSpec {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u32(self.source() as u32);
        writer.string(self.name());
        for value in self.tint() {
            writer.f32(value);
        }
        writer.f32(self.intensity().min());
        writer.f32(self.intensity().max());
        match self.operation() {
            grana_scene::Operation::Mask { set } => {
                writer.u8(0);
                writer.u64(set.raw());
            }
            grana_scene::Operation::Restore(restore) => {
                writer.u8(1);
                match restore.psf() {
                    grana_optics::Psf::Theoretical(params) => {
                        writer.u8(0);
                        for value in [
                            params.numerical_aperture(),
                            params.wavelength(),
                            params.immersion(),
                            params.sample(),
                            params.depth(),
                        ] {
                            writer.f32(value);
                        }
                    }
                    grana_optics::Psf::Measured(psf) => {
                        writer.u8(1);
                        for value in [psf.spacing().x(), psf.spacing().y(), psf.spacing().z()] {
                            writer.f32(value);
                        }
                        for value in psf.dims() {
                            writer.u32(value);
                        }
                        writer.u32(psf.layers());
                        for kernel in psf.kernels() {
                            for value in kernel.values() {
                                writer.f32(*value);
                            }
                        }
                    }
                }
                writer.u32(restore.iterations());
            }
            grana_scene::Operation::Smooth(smooth) => {
                writer.u8(2);
                writer.f32(smooth.sigma());
            }
            grana_scene::Operation::Background(background) => {
                writer.u8(3);
                writer.f32(background.radius());
            }
            grana_scene::Operation::Ratio(ratio) => {
                writer.u8(4);
                writer.u32(ratio.denominator() as u32);
            }
        }
    }
}

impl Encode for Shape {
    fn encode(&self, writer: &mut Encoder<'_>) {
        for value in self.extent {
            writer.u32(value);
        }
        for value in self.spacing {
            writer.f32(value);
        }
    }
}

impl Encode for Source {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::Synthetic { name } => {
                writer.u8(0);
                writer.string(name);
            }
            Self::File { name, path, series } => {
                writer.u8(1);
                writer.string(name);
                writer.string(&path.to_string_lossy());
                writer.u32(u32::try_from(*series).expect("a series index fits a project"));
            }
            Self::Remote { name, url, series } => {
                writer.u8(2);
                writer.string(name);
                writer.string(url);
                writer.u32(u32::try_from(*series).expect("a series index fits a project"));
            }
        }
    }
}

impl Encode for ViewState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.camera.encode(writer);
        self.section.encode(writer);
        writer.f32(self.split);
        self.timeline.encode(writer);
        self.export.encode(writer);
    }
}

impl Encode for Export {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u32(self.width);
        writer.bool(self.transparent);
    }
}

impl Encode for Timeline {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u32(self.fps());
        writer.f32(self.duration());
        self.keys().encode(writer);
    }
}

impl Encode for Keyframe {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.f32(self.at);
        writer.u32(self.time);
        self.camera.encode(writer);
        self.section.encode(writer);
    }
}

impl Encode for CameraState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        for value in self.pivot {
            writer.f32(value);
        }
        writer.f32(self.yaw);
        writer.f32(self.pitch);
        writer.f32(self.distance);
        writer.f32(self.fov_y);
    }
}

impl Encode for SectionState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        for value in self.center {
            writer.f32(value);
        }
        writer.f32(self.azimuth);
        writer.f32(self.elevation);
        writer.f32(self.span);
        writer.u8(self.projection.index() as u8);
        writer.f32(self.thickness);
    }
}

impl Encode for SceneState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.channels.encode(writer);
        self.derived.encode(writer);
        writer.count(self.selected);
        self.shading.encode(writer);
        writer.f32(self.step_scale);
        writer.f32(self.exposure);
        self.region.encode(writer);
        self.clip.encode(writer);
        writer.u64(self.mask.unwrap_or(0));
        writer.f32(self.mask_radius);
        match self.mask_filter {
            Some(filter) => {
                writer.bool(true);
                writer.u64(filter.target().raw());
                writer.i32(filter.shift());
                writer.u8(filter.field().index());
                for value in filter.range() {
                    writer.f32(value);
                }
            }
            None => writer.bool(false),
        }
        writer.u32(self.time);
        self.selections.encode(writer);
        self.sets.encode(writer);
        self.coloc.encode(writer);
        self.drift.encode(writer);
        self.annotations.encode(writer);
    }
}

impl Encode for AnnotationState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.entries.encode(writer);
        match self.selected {
            Some(index) => {
                writer.u8(1);
                writer.count(index);
            }
            None => writer.u8(0),
        }
        for count in self.counts {
            writer.u32(count);
        }
    }
}

impl Encode for Annotation {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.string(self.name());
        writer.bool(self.visible());
        match self.frame() {
            Some(frame) => {
                writer.u8(1);
                writer.u32(frame);
            }
            None => writer.u8(0),
        }
        self.kind().encode(writer);
    }
}

impl Encode for AnnotationKind {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u8(self.tag().index() as u8);
        for value in self.anchor() {
            writer.f32(value);
        }
        if let Some(end) = self.end() {
            for value in end {
                writer.f32(value);
            }
        }
    }
}

impl Encode for Drift {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u32(self.channel() as u32);
        writer.u32(self.reference());
        writer.count(self.offsets().len());
        for offset in self.offsets() {
            for value in offset {
                writer.f32(*value);
            }
        }
    }
}

impl Encode for ChannelState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.bool(self.enabled);
        writer.f32(self.opacity);
        self.window.encode(writer);
        self.transfer.encode(writer);
    }
}

impl Encode for TransferFunction {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.stops().encode(writer);
    }
}

impl Encode for Stop {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.f32(self.position());
        for value in self.color() {
            writer.f32(value);
        }
        writer.f32(self.opacity());
    }
}

impl Encode for Shading {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.f32(self.ambient);
        writer.f32(self.diffuse);
        writer.f32(self.specular);
        writer.f32(self.shininess);
        writer.f32(self.shadows);
        writer.f32(self.azimuth);
        writer.f32(self.elevation);
    }
}

impl Encode for Intensity {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.f32(self.min());
        writer.f32(self.max());
    }
}

impl Encode for Region {
    fn encode(&self, writer: &mut Encoder<'_>) {
        for value in self.center().to_array() {
            writer.f32(value);
        }
        for index in 0..3 {
            for value in self.axis(index).to_array() {
                writer.f32(value);
            }
        }
        for value in self.half().to_array() {
            writer.f32(value);
        }
        writer.bool(self.enabled());
    }
}

impl Encode for Clip {
    fn encode(&self, writer: &mut Encoder<'_>) {
        let (normal, offset) = self.parts();
        for value in normal {
            writer.f32(value);
        }
        writer.f32(offset);
    }
}

impl Encode for SelectionState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u64(self.set);
        self.item.encode(writer);
    }
}

impl Encode for ItemState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::Object(index) => {
                writer.u8(0);
                writer.count(*index);
            }
            Self::Track(index) => {
                writer.u8(1);
                writer.count(*index);
            }
        }
    }
}

impl Encode for SetState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u64(self.id);
        writer.u8(self.kind.index() as u8);
        writer.string(self.key.as_str());
        writer.string(&self.name);
        writer.count(self.channel);
        for value in self.color {
            writer.f32(value);
        }
        writer.bool(self.visible);
        writer.bool(self.labels);
        self.label.encode(writer);
        writer.count(self.hidden.len());
        for mask in &self.hidden {
            writer.count(mask.len());
            for hidden in mask {
                writer.bool(*hidden);
            }
        }
        writer.f32(self.opacity);
        self.params.encode(writer);
        self.track_style.encode(writer);
        self.tracks.encode(writer);
        self.object_color.encode(writer);
        self.track_color.encode(writer);
        self.classifier.encode(writer);
        self.object_coloc.encode(writer);
        self.label_export.encode(writer);
        self.visibility.encode(writer);
        self.seeds.encode(writer);
        self.frames.encode(writer);
        self.relations.encode(writer);
        self.provenance.encode(writer);
        self.curation.encode(writer);
        writer.u64(self.document);
    }
}

impl Encode for LabelExport {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.string(self.path());
        writer.u32(self.frames());
        writer.u32(self.labels());
    }
}

impl Encode for Curation {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.count(self.expected().len());
        for value in self.expected() {
            match value {
                None => writer.u8(0),
                Some(count) => {
                    writer.u8(1);
                    writer.count(*count);
                }
            }
        }
        writer.count(self.removed().len());
        for frame in self.removed() {
            encode_anchors(frame, writer);
        }
        writer.count(self.hidden().len());
        for frame in self.hidden() {
            encode_anchors(frame, writer);
        }
        writer.count(self.edits().len());
        for edit in self.edits() {
            edit.encode(writer);
        }
    }
}

fn encode_anchors(anchors: &[grana_scene::ObjectAnchor], writer: &mut Encoder<'_>) {
    writer.count(anchors.len());
    for anchor in anchors {
        for value in anchor.position() {
            writer.f32(value);
        }
        for value in anchor.bounds().min() {
            writer.f32(value);
        }
        for value in anchor.bounds().max() {
            writer.f32(value);
        }
    }
}

impl Encode for TrackEdit {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::Cut(node) => {
                writer.u8(0);
                encode_node(*node, writer);
            }
            Self::Join { source, target } => {
                writer.u8(1);
                encode_node(*source, writer);
                encode_node(*target, writer);
            }
        }
    }
}

fn encode_node(node: TrackAnchor, writer: &mut Encoder<'_>) {
    writer.u32(node.time());
    writer.count(node.object());
    for value in node.position() {
        writer.f32(value);
    }
}

impl Encode for Provenance {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::Detection => writer.u8(0),
            Self::Segmented { params } => {
                writer.u8(6);
                writer.f32(params.cellprob_threshold());
                writer.f32(params.flow_threshold());
                writer.count(params.minimum_size() as usize);
                writer.count(params.tile_overlap() as usize);
                writer.f32(params.slice_overlap());
                writer.u8(params.model().index());
            }
            Self::Filtered { source, criteria } => {
                writer.u8(1);
                writer.u64(source.raw());
                criteria.encode(writer);
            }
            Self::Copied { source } => {
                writer.u8(2);
                writer.u64(source.raw());
            }
            Self::Edited { source, edits } => {
                writer.u8(7);
                writer.u64(source.raw());
                writer.count(edits.len());
                for edit in edits {
                    writer.u32(edit.frame());
                    writer.u8(edit.kind().index() as u8);
                    encode_anchors(edit.anchors(), writer);
                }
            }
            Self::Merged { parts } => {
                writer.u8(3);
                parts.encode(writer);
            }
            Self::Imported { label } => {
                writer.u8(4);
                writer.string(label);
            }
            Self::Interop { file } => {
                writer.u8(5);
                writer.string(file);
            }
            Self::Masked { source, strokes } => {
                writer.u8(8);
                writer.u64(source.raw());
                writer.count(strokes.len());
                for stroke in strokes {
                    writer.u32(stroke.frame());
                    encode_anchors(std::slice::from_ref(&stroke.anchor()), writer);
                    writer.u8(stroke.tool().index() as u8);
                    writer.u8(stroke.mode().index() as u8);
                    writer.f32(stroke.radius());
                    writer.f32(stroke.tolerance());
                    writer.count(stroke.points().len());
                    for point in stroke.points() {
                        for value in point {
                            writer.f32(*value);
                        }
                    }
                }
            }
        }
    }
}

impl Encode for Criteria {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u8(self.group().index());
        self.filters().encode(writer);
        self.lasso().encode(writer);
    }
}

impl Encode for FilterRange {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.string(self.key());
        writer.f32(self.minimum());
        writer.f32(self.maximum());
    }
}

impl Encode for LassoPath {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.string(self.x());
        writer.string(self.y());
        writer.count(self.points().len());
        for point in self.points() {
            writer.f32(point[0]);
            writer.f32(point[1]);
        }
    }
}

impl Encode for Recipe {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.string(self.name());
        writer.count(self.channel());
        self.params().encode(writer);
        self.seeds().encode(writer);
        self.curation().encode(writer);
        self.provenance().encode(writer);
    }
}

impl Encode for Seeds {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::Points(frames) => {
                writer.u8(0);
                writer.count(frames.len());
                for frame in frames {
                    writer.count(frame.len());
                    for seed in frame {
                        for value in seed {
                            writer.f32(*value);
                        }
                    }
                }
            }
            Self::Source(source) => {
                writer.u8(1);
                writer.u64(source.raw());
            }
        }
    }
}

impl Encode for RelationState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u64(self.target);
        writer.i32(self.shift);
        writer.string(&self.key);
        self.frames.encode(writer);
    }
}

impl Encode for RelationFrame {
    fn encode(&self, writer: &mut Encoder<'_>) {
        let objects = self.objects();
        writer.count(objects);
        for object in 0..objects {
            writer.f32(self.distance(object));
            writer.f32(self.center(object));
            writer.f32(self.inside(object));
            writer.f32(self.contact(object));
            writer.u32(self.overlapping(object));
            writer.u32(self.partner(object));
        }
        writer.count(self.pairs().len());
        for pair in self.pairs() {
            writer.u32(pair.source());
            writer.u32(pair.target());
            writer.f32(pair.contact());
            writer.f32(pair.distance());
            writer.f32(pair.inside());
            writer.f32(pair.center());
        }
    }
}

impl Encode for ObjectParams {
    fn encode(&self, writer: &mut Encoder<'_>) {
        let kind = self.kind();
        writer.u8(kind.index() as u8);
        for index in 0..kind.fields().len() {
            writer.f32(self.value(index));
        }
    }
}

impl Encode for TrackStyle {
    fn encode(&self, writer: &mut Encoder<'_>) {
        let params = self.params();
        writer.f32(params.max_step());
        writer.u32(params.max_gap());
        writer.bool(params.division());
        writer.f32(params.division_range());
        writer.f32(self.thickness());
    }
}

impl Encode for TrackSet {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.f32(self.frame_interval());
        self.tracks().encode(writer);
    }
}

impl Encode for Track {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.parent().encode(writer);
        self.nodes().encode(writer);
    }
}

impl Encode for TrackNode {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u32(self.time());
        writer.count(self.object());
        for value in self.centroid() {
            writer.f32(value);
        }
    }
}

impl Encode for Visibility {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::All => writer.u8(0),
            Self::Object(frames) => {
                writer.u8(1);
                frames.encode(writer);
            }
            Self::Track(tracks) => {
                writer.u8(2);
                tracks.encode(writer);
            }
        }
    }
}

impl Encode for Palette {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u8(self.index());
    }
}

impl<M: Encode + Copy> Encode for ColorState<M> {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.metric.encode(writer);
        self.palette.encode(writer);
        writer.bool(self.reversed);
        match self.manual {
            Some((minimum, maximum)) => {
                writer.bool(true);
                writer.f32(minimum);
                writer.f32(maximum);
            }
            None => writer.bool(false),
        }
    }
}

impl Encode for Measurement {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::Position(axis) => {
                writer.u8(0);
                writer.u8(*axis as u8);
            }
            Self::Diameter => writer.u8(1),
            Self::Contrast => writer.u8(2),
            Self::Volume => writer.u8(3),
            Self::Area => writer.u8(4),
            Self::Sphericity => writer.u8(5),
            Self::Voxels => writer.u8(6),
            Self::MaxDiameter => writer.u8(13),
            Self::HullVolume => writer.u8(14),
            Self::Ellipsoid(axis) => {
                writer.u8(15);
                writer.u8(*axis as u8);
            }
            Self::Length => writer.u8(7),
            Self::Branches => writer.u8(8),
            Self::Terminals => writer.u8(9),
            Self::Junctions => writer.u8(10),
            Self::Nodes => writer.u8(11),
            Self::Intensity { channel, statistic } => {
                writer.u8(12);
                writer.count(*channel);
                writer.u8(statistic.index());
            }
        }
    }
}

impl Encode for ObjectColor {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::Measurement(measurement) => {
                writer.u8(0);
                measurement.encode(writer);
            }
            Self::Relation {
                target,
                field,
                shift,
            } => {
                writer.u8(1);
                writer.u64(target.raw());
                writer.u8(field.index());
                writer.i32(*shift);
            }
            Self::Child { target, field } => {
                writer.u8(2);
                writer.u64(target.raw());
                field.encode(writer);
            }
            Self::Score => writer.u8(3),
            Self::Coloc { pair, metric } => {
                writer.u8(4);
                pair.encode(writer);
                writer.u8(metric.index());
            }
        }
    }
}

impl Encode for ClassifierState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.features.encode(writer);
        self.mean.encode(writer);
        self.scale.encode(writer);
        self.weights.encode(writer);
        writer.f64(self.bias);
        writer.count(self.positives);
        writer.count(self.negatives);
        writer.f32(self.accuracy);
    }
}

impl Encode for ChildField {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::Count => writer.u8(0),
            Self::Stat { metric, statistic } => {
                writer.u8(1);
                metric.encode(writer);
                writer.u8(statistic.index());
            }
        }
    }
}

impl Encode for ChildMetric {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::Diameter => writer.u8(0),
            Self::Contrast => writer.u8(1),
            Self::Volume => writer.u8(2),
            Self::Area => writer.u8(3),
            Self::Sphericity => writer.u8(4),
            Self::Voxels => writer.u8(5),
            Self::Length => writer.u8(6),
            Self::Branches => writer.u8(7),
            Self::Terminals => writer.u8(8),
            Self::Junctions => writer.u8(9),
            Self::Nodes => writer.u8(10),
            Self::Intensity(channel) => {
                writer.u8(11);
                writer.count(*channel);
            }
        }
    }
}

impl Encode for TrackMeasurement {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u8(self.index());
    }
}

impl Encode for TrackColor {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Self::Measurement(measurement) => {
                writer.u8(0);
                measurement.encode(writer);
            }
            Self::Relation {
                target,
                field,
                aggregate,
                shift,
            } => {
                writer.u8(1);
                writer.u64(target.raw());
                writer.u8(field.index());
                writer.u8(aggregate.index());
                writer.i32(*shift);
            }
        }
    }
}

impl Encode for ObjectFrame {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.step();
        match self {
            Self::Spots(set) => {
                writer.u8(TAG_SPOTS);
                set.encode(writer);
            }
            Self::Surfaces(set) => {
                writer.u8(TAG_SURFACES);
                set.encode(writer);
            }
            Self::Filaments(set) => {
                writer.u8(TAG_FILAMENTS);
                set.encode(writer);
            }
        }
    }
}

impl Encode for SpotSet {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.count(self.level());
        self.spots().encode(writer);
        self.intensities().encode(writer);
    }
}

impl Encode for Spot {
    fn encode(&self, writer: &mut Encoder<'_>) {
        for value in self.center() {
            writer.f32(value);
        }
        writer.f32(self.radius());
        writer.f32(self.quality());
    }
}

impl Encode for SurfaceSet {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.count(self.level());
        self.objects().encode(writer);
        match self.geometry() {
            Ok(geometry) => geometry.encode(writer),
            Err(error) => writer.fail(&error.to_string()),
        }
        self.intensities().encode(writer);
        match self.topology() {
            None => writer.u8(0),
            Some(topology) => match topology.load() {
                Ok(topology) => {
                    writer.u8(1);
                    writer.step();
                    writer.blob(&topology.to_bytes());
                    writer.count(self.leaves().len());
                    for leaves in self.leaves() {
                        writer.count(leaves.len());
                        for basin in leaves {
                            writer.u32(*basin);
                        }
                    }
                    writer.count(self.removed().len());
                    for basin in self.removed() {
                        writer.u32(*basin);
                    }
                }
                Err(error) => writer.fail(&error.to_string()),
            },
        }
    }
}

impl Encode for SurfaceStatistics {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u64(self.voxel_count());
        writer.f64(self.volume());
        writer.f64(self.area());
        for value in self.centroid() {
            writer.f32(value);
        }
        self.bounds().encode(writer);
        match self.shape() {
            Some(shape) => {
                writer.u8(1);
                for axis in shape.axes() {
                    writer.f64(axis);
                }
                writer.f64(shape.diameter());
                writer.f64(shape.hull_volume());
            }
            None => writer.u8(0),
        }
    }
}

impl Encode for FilamentSet {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.count(self.level());
        self.objects().encode(writer);
        match self.geometry() {
            Ok(geometry) => geometry.encode(writer),
            Err(error) => writer.fail(&error.to_string()),
        }
        self.intensities().encode(writer);
    }
}

impl Encode for FilamentStatistics {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.f64(self.length());
        writer.f64(self.diameter());
        writer.f64(self.volume());
        self.topology().encode(writer);
        for value in self.centroid() {
            writer.f32(value);
        }
        self.bounds().encode(writer);
    }
}

impl Encode for FilamentTopology {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u32(self.branches());
        writer.u32(self.terminals());
        writer.u32(self.junctions());
        writer.u32(self.nodes());
    }
}

impl Encode for Bounds {
    fn encode(&self, writer: &mut Encoder<'_>) {
        for value in self.min() {
            writer.f32(value);
        }
        for value in self.max() {
            writer.f32(value);
        }
    }
}

impl Encode for PackedMesh {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.step();
        writer.blob(&self.to_bytes());
    }
}
impl Encode for IntensityTable {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.count(self.channels());
        writer.count(self.objects());
        for object in 0..self.objects() {
            for channel in 0..self.channels() {
                self.object(object, channel).encode(writer);
            }
        }
    }
}

impl Encode for SampleStatistics {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.f32(self.mean());
        writer.f32(self.minimum());
        writer.f32(self.maximum());
        writer.f32(self.deviation());
        writer.f64(self.sum());
    }
}

impl Encode for MeasureState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.set.encode(writer);
        writer.u8(self.group.index());
        writer.u8(self.aggregate.index());
        writer.u8(self.scope.index());
        self.filters.encode(writer);
        match &self.lasso {
            Some(lasso) => {
                writer.u8(1);
                writer.count(lasso.x());
                writer.count(lasso.y());
                writer.count(lasso.points().len());
                for point in lasso.points() {
                    writer.f32(point[0]);
                    writer.f32(point[1]);
                }
            }
            None => writer.u8(0),
        }
        match self.sort {
            Some((column, descending)) => {
                writer.u8(1);
                writer.count(column);
                writer.bool(descending);
            }
            None => writer.u8(0),
        }
        self.columns.encode(writer);
        writer.bool(self.visibility);
        writer.u8(self.view.index());
        writer.count(self.axes[0]);
        writer.count(self.axes[1]);
    }
}

impl Encode for Filter {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.count(self.column());
        writer.f32(self.minimum());
        writer.f32(self.maximum());
    }
}

impl Encode for ColocState {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.pair.encode(writer);
        self.results.encode(writer);
        self.settings.encode(writer);
        writer.u64(self.revision);
    }
}

impl Encode for ColocResult {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.thresholds().encode(writer);
        self.series().encode(writer);
    }
}

impl Encode for ColocPair {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.count(self.a());
        writer.count(self.b());
    }
}

impl Encode for ColocThresholds {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.f32(self.a());
        writer.f32(self.b());
    }
}

impl Encode for ColocSettings {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u32(self.permutations());
        writer.u32(self.block());
    }
}

impl Encode for ColocSeries {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.pair().encode(writer);
        self.info().encode(writer);
        writer.count(self.bins());
        self.frames().encode(writer);
        self.joint().encode(writer);
        self.significance().encode(writer);
        self.cost().encode(writer);
    }
}

impl Encode for ColocSeriesInfo {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.count(self.level());
        writer.count(self.permutation_level());
        writer.u32(self.permutation_stride());
        self.mask().encode(writer);
    }
}

impl Encode for ColocSignificance {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.f64(self.p());
        writer.u32(self.randomizations());
        writer.f64(self.permutation_pearson());
    }
}

impl Encode for ColocCost {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u64(self.scan_voxels());
        writer.u64(self.permutation_voxels());
        writer.u32(self.permutation_frames());
        writer.u32(self.randomizations());
        writer.u64(self.blocks());
    }
}

impl Encode for ColocFrame {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.moments().encode(writer);
        self.a_by_b().encode(writer);
        self.b_by_a().encode(writer);
    }
}

impl Encode for ColocMoments {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u64(self.voxels());
        for value in self.sums() {
            writer.f64(value);
        }
    }
}

impl Encode for ObjectColocValue {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.u64(self.voxels());
        for value in self.sums() {
            writer.f64(value);
        }
    }
}

impl Encode for ObjectColoc {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.pair().encode(writer);
        writer.count(self.level());
        self.frames().encode(writer);
    }
}

impl<T: Encode> Encode for [T] {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.count(self.len());
        for value in self {
            value.encode(writer);
        }
    }
}

impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, writer: &mut Encoder<'_>) {
        self.as_slice().encode(writer);
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, writer: &mut Encoder<'_>) {
        match self {
            Some(value) => {
                writer.bool(true);
                value.encode(writer);
            }
            None => writer.bool(false),
        }
    }
}
