use grana_analysis::TrackParams;
use grana_display::{Coloring, Shading, TransferFunction};
use grana_measure::{
    Aggregate, ColocPair, ColocThresholds, Filter, Group, Measurement, TrackMeasurement,
};
use grana_scene::{ObjectKind, ObjectParams, ObjectSetId, Selection};
use grana_volume::{Intensity, Region};

pub enum Command {
    OpenDataset,
    OpenImage,
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
    CutTrack {
        set: ObjectSetId,
        time: u32,
        object: usize,
    },
    JoinTracks {
        set: ObjectSetId,
        source: (u32, usize),
        target: (u32, usize),
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
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    AddObject,
    RemoveObject,
    ClearObject,
    Detect,
    Track,
    ClearTracks,
    CutTrack,
    JoinTracks,
    ChannelEnabled,
    ChannelOpacity,
    Window,
    ResetWindow,
    Transfer,
    ObjectVisible,
    ObjectColor,
    ObjectParameters,
    ObjectColoring,
    TrackVisible,
    TrackThickness,
    TrackColoring,
    TrackParameters,
    Region,
    Exposure,
    StepScale,
    Shading,
    ResetRendering,
    MeasureGroup,
    MeasureAggregate,
    MeasureFilter,
    AddFilter,
    RemoveFilter,
    MeasureVisibility,
    ColocPair,
    ColocThresholds,
    ComputeColoc,
    Keyframe,
    TimelineFps,
    TimelineDuration,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::AddObject => "add an object set",
            Self::RemoveObject => "remove an object set",
            Self::ClearObject => "clear objects",
            Self::Detect => "detection",
            Self::Track => "tracking",
            Self::ClearTracks => "clear tracks",
            Self::CutTrack => "track cut",
            Self::JoinTracks => "track join",
            Self::ChannelEnabled => "channel visibility",
            Self::ChannelOpacity => "channel opacity",
            Self::Window => "intensity window",
            Self::ResetWindow => "window reset",
            Self::Transfer => "transfer function",
            Self::ObjectVisible => "object visibility",
            Self::ObjectColor => "object color",
            Self::ObjectParameters => "object parameters",
            Self::ObjectColoring => "object colors",
            Self::TrackVisible => "track visibility",
            Self::TrackThickness => "track width",
            Self::TrackColoring => "track colors",
            Self::TrackParameters => "track parameters",
            Self::Region => "region",
            Self::Exposure => "exposure",
            Self::StepScale => "sampling",
            Self::Shading => "lighting",
            Self::ResetRendering => "rendering reset",
            Self::MeasureGroup => "statistics group",
            Self::MeasureAggregate => "statistics aggregate",
            Self::MeasureFilter => "statistics filter",
            Self::AddFilter => "filter addition",
            Self::RemoveFilter => "filter removal",
            Self::MeasureVisibility => "statistics visibility",
            Self::ColocPair => "colocalization channels",
            Self::ColocThresholds => "colocalization thresholds",
            Self::ComputeColoc => "colocalization",
            Self::Keyframe => "keyframes",
            Self::TimelineFps => "frame rate",
            Self::TimelineDuration => "duration",
        }
    }

    fn continuous(self) -> bool {
        matches!(
            self,
            Self::ChannelOpacity
                | Self::Window
                | Self::Transfer
                | Self::ObjectColor
                | Self::ObjectParameters
                | Self::ObjectColoring
                | Self::TrackThickness
                | Self::TrackColoring
                | Self::TrackParameters
                | Self::Region
                | Self::Exposure
                | Self::StepScale
                | Self::Shading
                | Self::MeasureFilter
                | Self::ColocThresholds
                | Self::TimelineFps
                | Self::TimelineDuration
        )
    }

    fn asynchronous(self) -> bool {
        matches!(self, Self::Detect | Self::Track | Self::ComputeColoc)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Edit {
    action: Action,
    target: u64,
}

impl Edit {
    pub(crate) fn new(action: Action) -> Self {
        Self { action, target: 0 }
    }

    fn targeted(action: Action, target: u64) -> Self {
        Self { action, target }
    }

    pub(crate) fn action(self) -> Action {
        self.action
    }

    pub(crate) fn target(self) -> u64 {
        self.target
    }

    pub(crate) fn continuous(self) -> bool {
        self.action.continuous()
    }

    pub(crate) fn asynchronous(self) -> bool {
        self.action.asynchronous()
    }
}

impl Command {
    pub(crate) fn edit(&self) -> Option<Edit> {
        let edit = match self {
            Self::AddObject(_) => Edit::new(Action::AddObject),
            Self::RemoveObject(_) => Edit::new(Action::RemoveObject),
            Self::ClearObject(_) => Edit::new(Action::ClearObject),
            Self::Detect(set) => Edit::targeted(Action::Detect, set.raw()),
            Self::Track(set) => Edit::targeted(Action::Track, set.raw()),
            Self::ClearTracks(_) => Edit::new(Action::ClearTracks),
            Self::CutTrack { .. } => Edit::new(Action::CutTrack),
            Self::JoinTracks { .. } => Edit::new(Action::JoinTracks),
            Self::SetChannelEnabled { channel, .. } => {
                Edit::targeted(Action::ChannelEnabled, *channel as u64)
            }
            Self::SetChannelOpacity { channel, .. } => {
                Edit::targeted(Action::ChannelOpacity, *channel as u64)
            }
            Self::SetWindow { channel, .. } => Edit::targeted(Action::Window, *channel as u64),
            Self::ResetWindow(channel) => Edit::targeted(Action::ResetWindow, *channel as u64),
            Self::SetTransfer { channel, .. } => Edit::targeted(Action::Transfer, *channel as u64),
            Self::SetObjectVisible { set, .. } => Edit::targeted(Action::ObjectVisible, set.raw()),
            Self::SetObjectColor { set, .. } => Edit::targeted(Action::ObjectColor, set.raw()),
            Self::SetObjectParams { set, .. } => {
                Edit::targeted(Action::ObjectParameters, set.raw())
            }
            Self::SetObjectColoring { set, .. } => {
                Edit::targeted(Action::ObjectColoring, set.raw())
            }
            Self::SetTrackVisible { set, .. } => Edit::targeted(Action::TrackVisible, set.raw()),
            Self::SetTrackThickness { set, .. } => {
                Edit::targeted(Action::TrackThickness, set.raw())
            }
            Self::SetTrackColoring { set, .. } => Edit::targeted(Action::TrackColoring, set.raw()),
            Self::SetTrackParams { set, .. } => Edit::targeted(Action::TrackParameters, set.raw()),
            Self::SetRegion(_) => Edit::new(Action::Region),
            Self::SetExposure(_) => Edit::new(Action::Exposure),
            Self::SetStepScale(_) => Edit::new(Action::StepScale),
            Self::SetShading(_) => Edit::new(Action::Shading),
            Self::ResetRendering => Edit::new(Action::ResetRendering),
            Self::SetMeasureGroup(_) => Edit::new(Action::MeasureGroup),
            Self::SetMeasureAggregate(_) => Edit::new(Action::MeasureAggregate),
            Self::AddFilter => Edit::new(Action::AddFilter),
            Self::RemoveFilter(_) => Edit::new(Action::RemoveFilter),
            Self::SetFilter { index, .. } | Self::RetargetFilter { index, .. } => {
                Edit::targeted(Action::MeasureFilter, *index as u64)
            }
            Self::SetMeasureVisibility(_) => Edit::new(Action::MeasureVisibility),
            Self::SetColocPair(_) => Edit::new(Action::ColocPair),
            Self::SetColocThresholds(_) => Edit::new(Action::ColocThresholds),
            Self::ComputeColoc => Edit::new(Action::ComputeColoc),
            Self::ClearKeyframes | Self::DeleteKeyframe(_) => Edit::new(Action::Keyframe),
            Self::SetTimelineFps(_) => Edit::new(Action::TimelineFps),
            Self::SetTimelineDuration(_) => Edit::new(Action::TimelineDuration),
            _ => return None,
        };
        Some(edit)
    }
}
