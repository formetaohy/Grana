use grana_animation::{Keyframe, Timeline};
use grana_display::{CameraState, Palette, SectionState, Shading, Stop, TransferFunction};
use grana_geometry::{Bounds, GeometryGroup, PackedMesh, PackedVertex};
use grana_measure::{
    ColocCost, ColocFrame, ColocMoments, ColocPair, ColocSeries, ColocSeriesInfo, ColocSettings,
    ColocSignificance, ColocThresholds,
};
use grana_measure::{Filter, Measurement, TrackMeasurement};
use grana_object::{
    FilamentSet, FilamentStatistics, FilamentTopology, IntensityTable, RelationFrame,
    SampleStatistics, Spot, SpotSet, SurfaceSet, SurfaceStatistics, Track, TrackNode, TrackSet,
};
use grana_scene::{
    ChannelState, ColocResult, ColocState, ColorState, ItemState, MeasureState, ObjectFrame,
    ObjectParams, RelationState, SceneState, SelectionState, SetState, TrackStyle, Visibility,
};
use grana_volume::{Intensity, Region};

use crate::binary::{Encode, Writer};
use crate::project::{Project, Shape, Source, ViewState};

pub(crate) const TAG_SPOTS: u8 = 0;
pub(crate) const TAG_SURFACES: u8 = 1;
pub(crate) const TAG_FILAMENTS: u8 = 2;

impl Encode for Project {
    fn encode(&self, writer: &mut Writer) {
        self.shape.encode(writer);
        self.source.encode(writer);
        self.scene.encode(writer);
        self.measure.encode(writer);
        self.view.encode(writer);
    }
}

impl Encode for Shape {
    fn encode(&self, writer: &mut Writer) {
        for value in self.extent {
            writer.u32(value);
        }
        for value in self.spacing {
            writer.f32(value);
        }
    }
}

impl Encode for Source {
    fn encode(&self, writer: &mut Writer) {
        match self {
            Self::Synthetic { name } => {
                writer.u8(0);
                writer.string(name);
            }
            Self::File { name, path } => {
                writer.u8(1);
                writer.string(name);
                writer.string(&path.to_string_lossy());
            }
        }
    }
}

impl Encode for ViewState {
    fn encode(&self, writer: &mut Writer) {
        self.camera.encode(writer);
        self.section.encode(writer);
        writer.f32(self.split);
        self.timeline.encode(writer);
    }
}

impl Encode for Timeline {
    fn encode(&self, writer: &mut Writer) {
        writer.u32(self.fps());
        writer.f32(self.duration());
        self.keys().encode(writer);
    }
}

impl Encode for Keyframe {
    fn encode(&self, writer: &mut Writer) {
        writer.f32(self.at);
        writer.u32(self.time);
        self.camera.encode(writer);
        self.section.encode(writer);
    }
}

impl Encode for CameraState {
    fn encode(&self, writer: &mut Writer) {
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
    fn encode(&self, writer: &mut Writer) {
        writer.u8(self.axis.index() as u8);
        writer.f32(self.span);
    }
}

impl Encode for SceneState {
    fn encode(&self, writer: &mut Writer) {
        self.channels.encode(writer);
        writer.count(self.selected);
        self.shading.encode(writer);
        writer.f32(self.step_scale);
        writer.f32(self.exposure);
        self.region.encode(writer);
        writer.u64(self.mask.unwrap_or(0));
        writer.u32(self.time);
        self.selection.encode(writer);
        self.sets.encode(writer);
        self.coloc.encode(writer);
    }
}

impl Encode for ChannelState {
    fn encode(&self, writer: &mut Writer) {
        writer.bool(self.enabled);
        writer.f32(self.opacity);
        self.window.encode(writer);
        self.transfer.encode(writer);
    }
}

impl Encode for TransferFunction {
    fn encode(&self, writer: &mut Writer) {
        self.stops().encode(writer);
    }
}

impl Encode for Stop {
    fn encode(&self, writer: &mut Writer) {
        writer.f32(self.position());
        for value in self.color() {
            writer.f32(value);
        }
        writer.f32(self.opacity());
    }
}

impl Encode for Shading {
    fn encode(&self, writer: &mut Writer) {
        writer.f32(self.ambient);
        writer.f32(self.diffuse);
        writer.f32(self.specular);
        writer.f32(self.shininess);
        writer.f32(self.shadows);
    }
}

impl Encode for Intensity {
    fn encode(&self, writer: &mut Writer) {
        writer.f32(self.min());
        writer.f32(self.max());
    }
}

impl Encode for Region {
    fn encode(&self, writer: &mut Writer) {
        for value in self.min().to_array() {
            writer.f32(value);
        }
        for value in self.max().to_array() {
            writer.f32(value);
        }
        writer.bool(self.enabled());
    }
}

impl Encode for SelectionState {
    fn encode(&self, writer: &mut Writer) {
        writer.u64(self.set);
        self.item.encode(writer);
    }
}

impl Encode for ItemState {
    fn encode(&self, writer: &mut Writer) {
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
    fn encode(&self, writer: &mut Writer) {
        writer.u64(self.id);
        writer.u8(self.kind.index() as u8);
        writer.string(&self.name);
        writer.count(self.channel);
        for value in self.color {
            writer.f32(value);
        }
        writer.bool(self.visible);
        writer.f32(self.opacity);
        self.params.encode(writer);
        self.track_style.encode(writer);
        self.tracks.encode(writer);
        self.track_thickness.encode(writer);
        self.track_visible.encode(writer);
        self.object_color.encode(writer);
        self.track_color.encode(writer);
        self.visibility.encode(writer);
        writer.count(self.seeds.len());
        for frame in &self.seeds {
            writer.count(frame.len());
            for seed in frame {
                for value in seed {
                    writer.f32(*value);
                }
            }
        }
        self.frames.encode(writer);
        self.relations.encode(writer);
        writer.u64(self.document);
    }
}

impl Encode for RelationState {
    fn encode(&self, writer: &mut Writer) {
        writer.u64(self.target);
        self.frames.encode(writer);
    }
}

impl Encode for RelationFrame {
    fn encode(&self, writer: &mut Writer) {
        let objects = self.objects();
        writer.count(objects);
        for object in 0..objects {
            writer.f32(self.distance(object));
            writer.f32(self.inside(object));
            writer.u32(self.partner(object));
        }
    }
}

impl Encode for ObjectParams {
    fn encode(&self, writer: &mut Writer) {
        let kind = self.kind();
        writer.u8(kind.index() as u8);
        for index in 0..kind.fields().len() {
            writer.f32(self.value(index));
        }
    }
}

impl Encode for TrackStyle {
    fn encode(&self, writer: &mut Writer) {
        let params = self.params();
        writer.f32(params.max_step());
        writer.u32(params.max_gap());
        writer.bool(params.division());
        writer.f32(params.division_range());
        writer.f32(self.thickness());
    }
}

impl Encode for TrackSet {
    fn encode(&self, writer: &mut Writer) {
        writer.f32(self.frame_interval());
        self.tracks().encode(writer);
    }
}

impl Encode for Track {
    fn encode(&self, writer: &mut Writer) {
        self.parent().encode(writer);
        self.nodes().encode(writer);
    }
}

impl Encode for TrackNode {
    fn encode(&self, writer: &mut Writer) {
        writer.u32(self.time());
        writer.count(self.object());
        for value in self.centroid() {
            writer.f32(value);
        }
    }
}

impl Encode for Visibility {
    fn encode(&self, writer: &mut Writer) {
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
    fn encode(&self, writer: &mut Writer) {
        writer.u8(self.index());
    }
}

impl<M: Encode + Copy> Encode for ColorState<M> {
    fn encode(&self, writer: &mut Writer) {
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
    fn encode(&self, writer: &mut Writer) {
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

impl Encode for TrackMeasurement {
    fn encode(&self, writer: &mut Writer) {
        writer.u8(self.index());
    }
}

impl Encode for ObjectFrame {
    fn encode(&self, writer: &mut Writer) {
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
    fn encode(&self, writer: &mut Writer) {
        writer.count(self.level());
        self.spots().encode(writer);
        self.intensities().encode(writer);
    }
}

impl Encode for Spot {
    fn encode(&self, writer: &mut Writer) {
        for value in self.center() {
            writer.f32(value);
        }
        writer.f32(self.radius());
        writer.f32(self.quality());
    }
}

impl Encode for SurfaceSet {
    fn encode(&self, writer: &mut Writer) {
        writer.count(self.level());
        self.objects().encode(writer);
        self.geometry().encode(writer);
        self.intensities().encode(writer);
    }
}

impl Encode for SurfaceStatistics {
    fn encode(&self, writer: &mut Writer) {
        writer.u64(self.voxel_count());
        writer.f64(self.volume());
        writer.f64(self.area());
        for value in self.centroid() {
            writer.f32(value);
        }
        self.bounds().encode(writer);
    }
}

impl Encode for FilamentSet {
    fn encode(&self, writer: &mut Writer) {
        writer.count(self.level());
        self.objects().encode(writer);
        self.geometry().encode(writer);
        self.intensities().encode(writer);
    }
}

impl Encode for FilamentStatistics {
    fn encode(&self, writer: &mut Writer) {
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
    fn encode(&self, writer: &mut Writer) {
        writer.u32(self.branches());
        writer.u32(self.terminals());
        writer.u32(self.junctions());
        writer.u32(self.nodes());
    }
}

impl Encode for Bounds {
    fn encode(&self, writer: &mut Writer) {
        for value in self.min() {
            writer.f32(value);
        }
        for value in self.max() {
            writer.f32(value);
        }
    }
}

impl Encode for PackedMesh {
    fn encode(&self, writer: &mut Writer) {
        self.groups().encode(writer);
        self.vertices().encode(writer);
        self.narrow().encode(writer);
        self.wide().encode(writer);
    }
}

impl Encode for GeometryGroup {
    fn encode(&self, writer: &mut Writer) {
        for value in self.min() {
            writer.f32(value);
        }
        for value in self.scale() {
            writer.f32(value);
        }
        writer.u32(self.first_vertex());
        writer.u32(self.vertex_count());
        writer.u32(self.first_index());
        writer.u32(self.index_count());
        writer.bool(self.wide());
    }
}

impl Encode for PackedVertex {
    fn encode(&self, writer: &mut Writer) {
        for value in self.position() {
            writer.u16(value);
        }
        for value in self.normal() {
            writer.u16(value);
        }
    }
}

impl Encode for IntensityTable {
    fn encode(&self, writer: &mut Writer) {
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
    fn encode(&self, writer: &mut Writer) {
        writer.f32(self.mean());
        writer.f32(self.minimum());
        writer.f32(self.maximum());
        writer.f32(self.deviation());
        writer.f64(self.sum());
    }
}

impl Encode for MeasureState {
    fn encode(&self, writer: &mut Writer) {
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
    fn encode(&self, writer: &mut Writer) {
        writer.count(self.column());
        writer.f32(self.minimum());
        writer.f32(self.maximum());
    }
}

impl Encode for ColocState {
    fn encode(&self, writer: &mut Writer) {
        self.pair.encode(writer);
        self.results.encode(writer);
        self.settings.encode(writer);
        writer.u64(self.document);
    }
}

impl Encode for ColocResult {
    fn encode(&self, writer: &mut Writer) {
        self.thresholds().encode(writer);
        self.series().encode(writer);
    }
}

impl Encode for ColocPair {
    fn encode(&self, writer: &mut Writer) {
        writer.count(self.a());
        writer.count(self.b());
    }
}

impl Encode for ColocThresholds {
    fn encode(&self, writer: &mut Writer) {
        writer.f32(self.a());
        writer.f32(self.b());
    }
}

impl Encode for ColocSettings {
    fn encode(&self, writer: &mut Writer) {
        writer.u32(self.permutations());
        writer.u32(self.block());
    }
}

impl Encode for ColocSeries {
    fn encode(&self, writer: &mut Writer) {
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
    fn encode(&self, writer: &mut Writer) {
        writer.count(self.level());
        writer.count(self.permutation_level());
        writer.u32(self.permutation_stride());
        self.mask().encode(writer);
    }
}

impl Encode for ColocSignificance {
    fn encode(&self, writer: &mut Writer) {
        writer.f64(self.p());
        writer.u32(self.randomizations());
        writer.f64(self.permutation_pearson());
    }
}

impl Encode for ColocCost {
    fn encode(&self, writer: &mut Writer) {
        writer.u64(self.scan_voxels());
        writer.u64(self.permutation_voxels());
        writer.u32(self.permutation_frames());
        writer.u32(self.randomizations());
        writer.u64(self.blocks());
    }
}

impl Encode for ColocFrame {
    fn encode(&self, writer: &mut Writer) {
        self.moments().encode(writer);
        self.a_by_b().encode(writer);
        self.b_by_a().encode(writer);
    }
}

impl Encode for ColocMoments {
    fn encode(&self, writer: &mut Writer) {
        writer.u64(self.voxels());
        for value in self.sums() {
            writer.f64(value);
        }
    }
}

impl<T: Encode> Encode for [T] {
    fn encode(&self, writer: &mut Writer) {
        writer.count(self.len());
        for value in self {
            value.encode(writer);
        }
    }
}

impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, writer: &mut Writer) {
        self.as_slice().encode(writer);
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, writer: &mut Writer) {
        match self {
            Some(value) => {
                writer.bool(true);
                value.encode(writer);
            }
            None => writer.bool(false),
        }
    }
}
