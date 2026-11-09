use grana_display::{Coloring, Shading, TransferFunction};
use grana_measure::{
    Aggregate, ColocPair, ColocSettings, ColocThresholds, Filter, Group, Lasso, MeasureView,
    Measurement, PlotAxis, Scope,
};
use grana_mesh::Format as MeshFormat;
use grana_object::{MaskMode, MaskTool, ObjectAnchor, TrackParams};
use grana_optics::Psf;

use grana_scene::{
    MaskFilter, ObjectColor, ObjectKind, ObjectParams, ObjectSetId, SegmentParams, Selection,
    TrackColor,
};
use grana_volume::{Clip, Intensity, Region};

pub use grana_project::Export;

#[derive(Clone)]
pub enum Command {
    OpenDataset,
    OpenImage,
    OpenRemote(String),
    OpenDemo,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    SaveSnapshot,
    ClearCache,
    SetTime(u32),
    SelectSeries(usize),
    SelectField(usize),
    Select(Option<Selection>),
    ToggleSelect(Selection),
    RenameObject {
        set: ObjectSetId,
        name: String,
    },
    ReorderObject {
        set: ObjectSetId,
        index: usize,
    },
    DeleteObject {
        set: ObjectSetId,
        time: u32,
        object: usize,
    },
    DeleteFiltered,
    ExtractFiltered,
    DuplicateObject(ObjectSetId),
    MergeObjects {
        source: ObjectSetId,
        target: ObjectSetId,
    },
    MergeSurfaces {
        set: ObjectSetId,
        time: u32,
        objects: Vec<usize>,
    },
    SplitSurface {
        set: ObjectSetId,
        time: u32,
        object: usize,
    },
    PaintMask {
        set: ObjectSetId,
        frame: u32,
        anchor: ObjectAnchor,
        tool: MaskTool,
        mode: MaskMode,
        radius: f32,
        tolerance: f32,
        points: Vec<[f32; 3]>,
    },
    ClearMask(ObjectSetId),
    AddObject(ObjectKind),
    AddSegment,
    SetSegment {
        set: ObjectSetId,
        params: SegmentParams,
    },
    AddMaskChannel {
        set: ObjectSetId,
    },
    AddRestoreChannel {
        source: usize,
        psf: Psf,
        iterations: u32,
    },
    AddSmoothChannel {
        source: usize,
        sigma: f32,
    },
    AddBackgroundChannel {
        source: usize,
        radius: f32,
    },
    AddRatioChannel {
        numerator: usize,
        denominator: usize,
    },
    SetSmoothChannel {
        channel: usize,
        sigma: f32,
    },
    SetBackgroundChannel {
        channel: usize,
        radius: f32,
    },
    SetRatioChannel {
        channel: usize,
        denominator: usize,
    },
    CalibratePsf {
        source: usize,
        threshold: f32,
        separation: f32,
    },
    SetRestore {
        channel: usize,
        psf: Psf,
        iterations: u32,
    },
    RemoveDerivedChannel {
        channel: usize,
    },
    ImportLabels {
        label: String,
    },
    ImportTracks,
    ExportTracks(ObjectSetId),
    ExportGeff(ObjectSetId),
    RemoveObject(ObjectSetId),
    AddSeed {
        set: ObjectSetId,
        position: [f32; 3],
    },
    RemoveSeed {
        set: ObjectSetId,
        index: usize,
    },
    ClearSeeds(ObjectSetId),
    SetSeedSource {
        set: ObjectSetId,
        source: Option<ObjectSetId>,
    },
    Detect(ObjectSetId),
    CancelDetect(ObjectSetId),
    ClearObject(ObjectSetId),
    AddRelation {
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    },
    MoveRelation {
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
        next: i32,
    },
    RemoveRelation {
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    },
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
    SetObjectLabels {
        set: ObjectSetId,
        labels: bool,
    },
    SetLabelMetric {
        set: ObjectSetId,
        metric: Option<ObjectColor>,
    },
    SetObjectHidden {
        set: ObjectSetId,
        time: u32,
        objects: Vec<usize>,
        hidden: bool,
    },
    ClearHidden(ObjectSetId),
    SetObjectColor {
        set: ObjectSetId,
        color: [f32; 3],
    },
    SetObjectOpacity {
        set: ObjectSetId,
        opacity: f32,
    },
    SetObjectParams {
        set: ObjectSetId,
        params: ObjectParams,
    },
    SetObjectColoring {
        set: ObjectSetId,
        coloring: Coloring<ObjectColor>,
    },
    SetMask(Option<ObjectSetId>),
    SetMaskRadius(f32),
    SetMaskFilter(Option<MaskFilter>),
    AddPoint {
        at: [f32; 3],
    },
    AddDistance {
        from: [f32; 3],
        to: [f32; 3],
    },
    SelectAnnotation(Option<usize>),
    SetAnnotationVisible {
        index: usize,
        visible: bool,
    },
    SetAnnotationKind {
        index: usize,
        kind: grana_scene::AnnotationKind,
    },
    SetAnnotationFrame {
        index: usize,
        frame: Option<u32>,
    },
    RemoveAnnotation(usize),
    ClearAnnotations,
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
        coloring: Coloring<TrackColor>,
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
    SetClip(Clip),
    SetExposure(f32),
    SetStepScale(f32),
    SetShading(Shading),
    ResetRendering,
    SetMeasureSet(Option<ObjectSetId>),
    TrainClassifier {
        set: ObjectSetId,
        features: Vec<Measurement>,
    },
    ClearClassifier(ObjectSetId),
    SetMeasureGroup(Group),
    SetMeasureAggregate(Aggregate),
    SetMeasureScope(Scope),
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
    SetLasso(Option<Lasso>),
    SetColumnVisible {
        index: usize,
        visible: bool,
    },
    ShowAllColumns,
    SetMeasureVisibility(bool),
    SetMeasureView(MeasureView),
    SetPlotAxis {
        axis: PlotAxis,
        column: usize,
    },
    SetGate {
        column: usize,
        minimum: f32,
        maximum: f32,
    },
    ToggleSort(usize),
    SetColocPair(ColocPair),
    SetColocThresholds(ColocThresholds),
    SetColocSettings(ColocSettings),
    RemoveColocResult(ColocPair),
    ComputeColoc,
    CancelColoc,
    ComputeObjectColoc(ObjectSetId),
    CancelObjectColoc(ObjectSetId),
    ClearObjectColoc(ObjectSetId),
    ComputeDrift,
    CancelDrift,
    ClearDrift,
    SetDriftReference(u32),
    SetDriftOffset {
        time: u32,
        offset: [f32; 3],
    },
    ExportColoc,
    ExportMeasure,
    ExportRelation {
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    },
    ExportMesh {
        set: ObjectSetId,
        format: MeshFormat,
    },
    ExportLabels(ObjectSetId),
    ExportAllLabels,
    ExportDataset,
    ExportPlate,
    CaptureProtocol,
    ApplyProtocol,
    LoadProtocol,
    SaveProtocol,
    AddBatchDatasets,
    ClearBatchDatasets,
    ChooseBatchOutput,
    OpenBatchRun,
    ReviewBatchDataset(usize),
    RecordBatchCorrection,
    ClearBatchCorrection,
    SetBatchJobs(u32),
    SetBatchProject(bool),
    SetBatchChannels(bool),
    RunBatch,
    PauseBatch,
    ResumeBatch,
    CancelBatch,
    SetKeyframe,
    SaveView,
    ApplyView(usize),
    RemoveView(usize),
    ClearKeyframes,
    DeleteKeyframe(usize),
    SetTimelineFps(u32),
    SetTimelineDuration(f32),
    ExportMovie(Export),
    SetExport(Export),
    ExportSequence(Export),
    CancelExport,
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    AddObject,
    AddSegment,
    RemoveObject,
    RenameObject,
    ReorderObject,
    DeleteObject,
    DeleteFiltered,
    ExtractFiltered,
    DuplicateObject,
    MergeObjects,
    EditObjects,
    MaskEdit,
    ClearMask,
    ClearObject,
    Seed,
    SeedSource,
    Detect,
    Relation,
    Track,
    ClearTracks,
    CutTrack,
    JoinTracks,
    ChannelEnabled,
    ChannelOpacity,
    Window,
    ResetWindow,
    Transfer,
    Derived,
    ObjectVisible,
    ObjectLabels,
    ObjectHidden,
    ObjectColor,
    ObjectOpacity,
    ObjectParameters,
    ObjectColoring,
    Mask,
    Annotation,
    TrackVisible,
    TrackThickness,
    TrackColoring,
    TrackParameters,
    Region,
    Clip,
    Exposure,
    StepScale,
    Shading,
    ResetRendering,
    Classifier,
    MeasureGroup,
    MeasureAggregate,
    MeasureScope,
    MeasureSet,
    MeasureColumns,
    MeasureView,
    MeasureAxes,
    MeasureSort,
    MeasureLasso,
    MeasureFilter,
    AddFilter,
    RemoveFilter,
    MeasureVisibility,
    ColocPair,
    ColocThresholds,
    ColocSettings,
    ColocResult,
    ComputeColoc,
    ObjectColoc,
    ApplyProtocol,
    Drift,
    Keyframe,
    TimelineFps,
    TimelineDuration,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::AddObject => "add an object set",
            Self::AddSegment => "add a segmentation",
            Self::RemoveObject => "remove an object set",
            Self::RenameObject => "rename an object set",
            Self::ReorderObject => "reorder object sets",
            Self::DeleteObject => "delete an object",
            Self::DeleteFiltered => "delete excluded objects",
            Self::ExtractFiltered => "filtered object set",
            Self::DuplicateObject => "copy an object set",
            Self::MergeObjects => "merge object sets",
            Self::EditObjects => "edit surfaces",
            Self::MaskEdit => "mask painting",
            Self::ClearMask => "mask clearing",
            Self::ClearObject => "clear objects",
            Self::Seed => "seed points",
            Self::SeedSource => "seed source",
            Self::Detect => "detection",
            Self::Relation => "object relation",
            Self::Track => "tracking",
            Self::ClearTracks => "clear tracks",
            Self::CutTrack => "track cut",
            Self::JoinTracks => "track join",
            Self::ChannelEnabled => "channel visibility",
            Self::ChannelOpacity => "channel opacity",
            Self::Window => "intensity window",
            Self::ResetWindow => "window reset",
            Self::Transfer => "transfer function",
            Self::Derived => "derived channel",
            Self::ObjectVisible => "object visibility",
            Self::ObjectLabels => "object labels",
            Self::ObjectHidden => "hidden objects",
            Self::ObjectColor => "object color",
            Self::ObjectOpacity => "object opacity",
            Self::ObjectParameters => "object parameters",
            Self::ObjectColoring => "object colors",
            Self::Mask => "analysis mask",
            Self::Annotation => "annotation",
            Self::TrackVisible => "track visibility",
            Self::TrackThickness => "track width",
            Self::TrackColoring => "track colors",
            Self::TrackParameters => "track parameters",
            Self::Region => "region",
            Self::Clip => "clipping plane",
            Self::Exposure => "exposure",
            Self::StepScale => "sampling",
            Self::Shading => "lighting",
            Self::ResetRendering => "rendering reset",
            Self::Classifier => "object classifier",
            Self::MeasureGroup => "statistics group",
            Self::MeasureAggregate => "statistics aggregate",
            Self::MeasureScope => "statistics range",
            Self::MeasureSet => "statistics object set",
            Self::MeasureColumns => "statistics columns",
            Self::MeasureView => "statistics view",
            Self::MeasureAxes => "statistics axes",
            Self::MeasureSort => "statistics order",
            Self::MeasureLasso => "statistics lasso",
            Self::MeasureFilter => "statistics filter",
            Self::AddFilter => "filter addition",
            Self::RemoveFilter => "filter removal",
            Self::MeasureVisibility => "statistics visibility",
            Self::ColocPair => "colocalization channels",
            Self::ColocThresholds => "colocalization thresholds",
            Self::ColocSettings => "colocalization settings",
            Self::ColocResult => "colocalization result",
            Self::ComputeColoc => "colocalization",
            Self::ObjectColoc => "object colocalization",
            Self::ApplyProtocol => "protocol replay",
            Self::Drift => "drift correction",
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
                | Self::ObjectOpacity
                | Self::ObjectParameters
                | Self::ObjectColoring
                | Self::MaskEdit
                | Self::Annotation
                | Self::TrackThickness
                | Self::TrackColoring
                | Self::TrackParameters
                | Self::Region
                | Self::Exposure
                | Self::StepScale
                | Self::Shading
                | Self::MeasureFilter
                | Self::ColocThresholds
                | Self::ColocSettings
                | Self::TimelineFps
                | Self::TimelineDuration
        )
    }

    fn asynchronous(self) -> bool {
        matches!(
            self,
            Self::Detect
                | Self::Track
                | Self::ComputeColoc
                | Self::ObjectColoc
                | Self::ApplyProtocol
        )
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
            Self::AddObject(_) | Self::AddSegment => Edit::new(Action::AddObject),
            Self::SetSegment { set, .. } => Edit::targeted(Action::ObjectParameters, set.raw()),
            Self::ImportLabels { .. } | Self::ImportTracks => Edit::new(Action::AddObject),
            Self::RemoveObject(_) => Edit::new(Action::RemoveObject),
            Self::RenameObject { set, .. } => Edit::targeted(Action::RenameObject, set.raw()),
            Self::ReorderObject { .. } => Edit::new(Action::ReorderObject),
            Self::DeleteObject { set, .. } => Edit::targeted(Action::DeleteObject, set.raw()),
            Self::DeleteFiltered => Edit::new(Action::DeleteFiltered),
            Self::ExtractFiltered => Edit::new(Action::ExtractFiltered),
            Self::DuplicateObject(_) => Edit::new(Action::DuplicateObject),
            Self::MergeObjects { target, .. } => Edit::targeted(Action::MergeObjects, target.raw()),
            Self::MergeSurfaces { set, .. } | Self::SplitSurface { set, .. } => {
                Edit::targeted(Action::EditObjects, set.raw())
            }
            Self::PaintMask { set, .. } => Edit::targeted(Action::MaskEdit, set.raw()),
            Self::ClearMask(set) => Edit::targeted(Action::ClearMask, set.raw()),
            Self::ClearObject(_) => Edit::new(Action::ClearObject),
            Self::AddSeed { set, .. } | Self::RemoveSeed { set, .. } | Self::ClearSeeds(set) => {
                Edit::targeted(Action::Seed, set.raw())
            }
            Self::SetSeedSource { set, .. } => Edit::targeted(Action::SeedSource, set.raw()),
            Self::Detect(set) => Edit::targeted(Action::Detect, set.raw()),
            Self::AddRelation { target, .. }
            | Self::MoveRelation { target, .. }
            | Self::RemoveRelation { target, .. } => Edit::targeted(Action::Relation, target.raw()),
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
            Self::AddMaskChannel { .. }
            | Self::AddRestoreChannel { .. }
            | Self::AddSmoothChannel { .. }
            | Self::AddBackgroundChannel { .. }
            | Self::AddRatioChannel { .. }
            | Self::CalibratePsf { .. } => Edit::new(Action::Derived),
            Self::SetSmoothChannel { channel, .. }
            | Self::SetBackgroundChannel { channel, .. }
            | Self::SetRatioChannel { channel, .. }
            | Self::SetRestore { channel, .. }
            | Self::RemoveDerivedChannel { channel } => {
                Edit::targeted(Action::Derived, *channel as u64)
            }
            Self::SetObjectVisible { set, .. } => Edit::targeted(Action::ObjectVisible, set.raw()),
            Self::SetObjectLabels { set, .. } => Edit::targeted(Action::ObjectLabels, set.raw()),
            Self::SetLabelMetric { set, .. } => Edit::targeted(Action::ObjectLabels, set.raw()),
            Self::SetObjectHidden { set, .. } | Self::ClearHidden(set) => {
                Edit::targeted(Action::ObjectHidden, set.raw())
            }
            Self::SetObjectColor { set, .. } => Edit::targeted(Action::ObjectColor, set.raw()),
            Self::SetObjectOpacity { set, .. } => Edit::targeted(Action::ObjectOpacity, set.raw()),
            Self::SetObjectParams { set, .. } => {
                Edit::targeted(Action::ObjectParameters, set.raw())
            }
            Self::SetObjectColoring { set, .. } => {
                Edit::targeted(Action::ObjectColoring, set.raw())
            }
            Self::SetMask(_) | Self::SetMaskRadius(_) | Self::SetMaskFilter(_) => {
                Edit::new(Action::Mask)
            }
            Self::AddPoint { .. }
            | Self::AddDistance { .. }
            | Self::SetAnnotationVisible { .. }
            | Self::RemoveAnnotation(_)
            | Self::ClearAnnotations => Edit::new(Action::Annotation),
            Self::SetAnnotationKind { index, .. } | Self::SetAnnotationFrame { index, .. } => {
                Edit::targeted(Action::Annotation, *index as u64)
            }
            Self::SetTrackVisible { set, .. } => Edit::targeted(Action::TrackVisible, set.raw()),
            Self::SetTrackThickness { set, .. } => {
                Edit::targeted(Action::TrackThickness, set.raw())
            }
            Self::SetTrackColoring { set, .. } => Edit::targeted(Action::TrackColoring, set.raw()),
            Self::SetTrackParams { set, .. } => Edit::targeted(Action::TrackParameters, set.raw()),
            Self::SetRegion(_) => Edit::new(Action::Region),
            Self::SetClip(_) => Edit::new(Action::Clip),
            Self::SetExposure(_) => Edit::new(Action::Exposure),
            Self::SetStepScale(_) => Edit::new(Action::StepScale),
            Self::SetShading(_) => Edit::new(Action::Shading),
            Self::ResetRendering => Edit::new(Action::ResetRendering),
            Self::TrainClassifier { set, .. } | Self::ClearClassifier(set) => {
                Edit::targeted(Action::Classifier, set.raw())
            }
            Self::SetMeasureGroup(_) => Edit::new(Action::MeasureGroup),
            Self::SetMeasureAggregate(_) => Edit::new(Action::MeasureAggregate),
            Self::SetMeasureScope(_) => Edit::new(Action::MeasureScope),
            Self::SetMeasureSet(_) => Edit::new(Action::MeasureSet),
            Self::SetColumnVisible { index, .. } => {
                Edit::targeted(Action::MeasureColumns, *index as u64)
            }
            Self::ShowAllColumns => Edit::new(Action::MeasureColumns),
            Self::SetMeasureView(_) => Edit::new(Action::MeasureView),
            Self::SetPlotAxis { column, .. } => Edit::targeted(Action::MeasureAxes, *column as u64),
            Self::ToggleSort(column) => Edit::targeted(Action::MeasureSort, *column as u64),
            Self::SetLasso(_) => Edit::new(Action::MeasureLasso),
            Self::AddFilter => Edit::new(Action::AddFilter),
            Self::RemoveFilter(_) => Edit::new(Action::RemoveFilter),
            Self::SetFilter { index, .. } | Self::RetargetFilter { index, .. } => {
                Edit::targeted(Action::MeasureFilter, *index as u64)
            }
            Self::SetGate { column, .. } => Edit::targeted(Action::MeasureFilter, *column as u64),
            Self::SetMeasureVisibility(_) => Edit::new(Action::MeasureVisibility),
            Self::SetColocPair(_) => Edit::new(Action::ColocPair),
            Self::SetColocThresholds(_) => Edit::new(Action::ColocThresholds),
            Self::SetColocSettings(_) => Edit::new(Action::ColocSettings),
            Self::RemoveColocResult(_) => Edit::new(Action::ColocResult),
            Self::ComputeColoc => Edit::new(Action::ComputeColoc),
            Self::ComputeObjectColoc(set) | Self::ClearObjectColoc(set) => {
                Edit::targeted(Action::ObjectColoc, set.raw())
            }
            Self::ClearDrift | Self::SetDriftReference(_) | Self::SetDriftOffset { .. } => {
                Edit::new(Action::Drift)
            }
            Self::ApplyProtocol => Edit::new(Action::ApplyProtocol),
            Self::ClearKeyframes | Self::DeleteKeyframe(_) => Edit::new(Action::Keyframe),
            Self::SetTimelineFps(_) => Edit::new(Action::TimelineFps),
            Self::SetTimelineDuration(_) => Edit::new(Action::TimelineDuration),
            _ => return None,
        };
        Some(edit)
    }
}
