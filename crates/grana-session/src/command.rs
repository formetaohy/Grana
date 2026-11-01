use grana_analysis::TrackParams;
use grana_display::{Coloring, Shading, TransferFunction};
use grana_measure::{
    Aggregate, ColocPair, ColocThresholds, Filter, Group, Measurement, TrackMeasurement,
};
use grana_scene::{ObjectKind, ObjectParams, ObjectSetId, Selection};
use grana_volume::{Intensity, Region};

pub enum Command {
    OpenDataset,
    OpenDemo,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    SaveSnapshot,
    SetTime(u32),
    Select(Option<Selection>),
    AddObject(ObjectKind),
    RemoveObject(ObjectSetId),
    Detect(ObjectSetId),
    CancelDetect(ObjectSetId),
    ClearObject(ObjectSetId),
    Track(ObjectSetId),
    CancelTrack(ObjectSetId),
    ClearTracks(ObjectSetId),
    SelectChannel(usize),
    SetChannelEnabled {
        channel: usize,
        enabled: bool,
    },
    SetChannelOpacity {
        channel: usize,
        opacity: f32,
    },
    SetWindow {
        channel: usize,
        window: Intensity,
    },
    ResetWindow(usize),
    SetTransfer {
        channel: usize,
        transfer: TransferFunction,
    },
    SetObjectVisible {
        set: ObjectSetId,
        visible: bool,
    },
    SetObjectColor {
        set: ObjectSetId,
        color: [f32; 3],
    },
    SetObjectParams {
        set: ObjectSetId,
        params: ObjectParams,
    },
    SetObjectColoring {
        set: ObjectSetId,
        coloring: Coloring<Measurement>,
    },
    SetTrackParams {
        set: ObjectSetId,
        params: TrackParams,
    },
    SetTrackVisible {
        set: ObjectSetId,
        visible: bool,
    },
    SetTrackThickness {
        set: ObjectSetId,
        thickness: f32,
    },
    SetTrackColoring {
        set: ObjectSetId,
        coloring: Coloring<TrackMeasurement>,
    },
    SelectTrack {
        set: ObjectSetId,
        index: Option<usize>,
    },
    SetRegion(Region),
    SetExposure(f32),
    SetStepScale(f32),
    SetShading(Shading),
    ResetRendering,
    SetMeasureSet(Option<ObjectSetId>),
    SetMeasureGroup(Group),
    SetMeasureAggregate(Aggregate),
    AddFilter,
    RemoveFilter(usize),
    SetFilter {
        index: usize,
        filter: Filter,
    },
    RetargetFilter {
        index: usize,
        column: usize,
    },
    SetColumnVisible {
        index: usize,
        visible: bool,
    },
    ShowAllColumns,
    SetMeasureVisibility(bool),
    ToggleSort(usize),
    SetColocPair(ColocPair),
    SetColocThresholds(ColocThresholds),
    ComputeColoc,
    CancelColoc,
    ExportColoc,
    ExportMeasure,
    CaptureProtocol,
    LoadProtocol,
    SaveProtocol,
    AddBatchDatasets,
    ClearBatchDatasets,
    ChooseBatchOutput,
    RunBatch,
    CancelBatch,
    SetKeyframe,
    ClearKeyframes,
    DeleteKeyframe(usize),
    SetTimelineFps(u32),
    SetTimelineDuration(f32),
    ExportMovie {
        width: u32,
    },
    CancelMovie,
}
