use std::path::PathBuf;

use grana_display::{Coloring, Material, Shading, TransferFunction, VolumeMode};
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
    OpenAny,
    OpenDataset,
    OpenImage,
    OpenTiles,
    OpenPath(PathBuf),
    OpenRemote(String),
    OpenRecent(usize),
    RecoverAnalysis,
    DiscardAnalysis,
    OpenProject,
    OpenPackage,
    SaveProject,
    SaveProjectAs,
    SaveSnapshot,
    SaveFigure,
    SaveLineage,
    SaveChart,
    SaveComparison,
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
    AddBleachChannel {
        source: usize,
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
    ImportLocalizations,
    ExportGeff(ObjectSetId),
    ExportLocalizations(ObjectSetId),
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
    SetVolumeMode {
        channel: usize,
        mode: VolumeMode,
    },
    SetIsovalue {
        channel: usize,
        isovalue: f32,
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
    SetObjectMaterial {
        set: ObjectSetId,
        material: Material,
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
    SampleProfile {
        index: usize,
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
    DivideTracks {
        set: ObjectSetId,
        parent: (u32, usize),
        first: (u32, usize),
        second: (u32, usize),
    },
    MergeTracks {
        set: ObjectSetId,
        first: (u32, usize),
        second: (u32, usize),
        child: (u32, usize),
    },
    ReparentTrack {
        set: ObjectSetId,
        child: (u32, usize),
        parent: (u32, usize),
    },
    DetachTrack {
        set: ObjectSetId,
        child: (u32, usize),
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
    AnalyzeLocalizations(ObjectSetId),
    SetLocalizationSpec {
        set: ObjectSetId,
        spec: grana_object::LocalizationSpec,
    },
    SetLocalizationGroupColored {
        set: ObjectSetId,
        group_colored: bool,
    },
    SetLocalizationProjected {
        set: ObjectSetId,
        projected: bool,
    },
    SetLocalizationClusters {
        set: ObjectSetId,
        clusters: grana_object::ClusterSpec,
    },
    SetLocalizationLayer {
        set: ObjectSetId,
        layer: usize,
    },
    ClusterLocalizations(ObjectSetId),
    EstimateLocalizationClusters(ObjectSetId),
    CancelClusters(ObjectSetId),
    CancelLocalizations(ObjectSetId),
    ClearLocalizations(ObjectSetId),
    ExportLocalizationImage(ObjectSetId),
    ExportLocalizationComposite,
    ComputeDrift,
    ComputeLocalizationDrift(ObjectSetId),
    CancelDrift,
    ClearDrift,
    SetDriftReference(u32),
    SetDriftOffset {
        time: u32,
        offset: [f32; 3],
    },
    RegisterChannels(usize),
    RegisterFromLocalizations(usize),
    CancelRegistration,
    ClearRegistration,
    ExportColoc,
    ExportMeasure,
    ExportProvenance,
    ExportPackage,
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
    SetStitchGrid {
        rows: usize,
        columns: usize,
    },
    SetStitchChannel(usize),
    SetStitchSeries(usize),
    ConfirmStitch,
    CancelStitch,
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
    ExportMovie {
        transparent: bool,
    },
    SetExport(Export),
    ExportSequence,
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
    DivideTracks,
    MergeTracks,
    ReparentTrack,
    DetachTrack,
    ChannelEnabled,
    ChannelOpacity,
    Window,
    ResetWindow,
    Transfer,
    VolumeMode,
    Isovalue,
    Derived,
    ObjectVisible,
    ObjectLabels,
    ObjectHidden,
    ObjectColor,
    ObjectOpacity,
    ObjectMaterial,
    ObjectParameters,
    ObjectColoring,
    Mask,
    Annotation,
    TrackVisible,
    TrackThickness,
    TrackColoring,
    TrackParameters,
    Localization,
    Cluster,
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
    Registration,
    Keyframe,
    NamedView,
    Export,
    TimelineFps,
    TimelineDuration,
}

impl Action {
    fn continuous(self) -> bool {
        matches!(
            self,
            Self::ChannelOpacity
                | Self::Window
                | Self::Transfer
                | Self::Isovalue
                | Self::ObjectColor
                | Self::ObjectOpacity
                | Self::ObjectMaterial
                | Self::ObjectParameters
                | Self::ObjectColoring
                | Self::MaskEdit
                | Self::Annotation
                | Self::TrackThickness
                | Self::TrackColoring
                | Self::TrackParameters
                | Self::Localization
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
                | Self::Cluster
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

#[derive(Clone, Copy)]
pub(crate) enum Nature {
    Document,
    DocumentAuthored,
    View(Option<Edit>),
    Authored,
    Edit(Edit),
}

impl Command {
    pub(crate) fn nature(&self) -> Nature {
        match self {
            Self::OpenAny => Nature::Document,
            Self::OpenDataset => Nature::Document,
            Self::OpenImage => Nature::Document,
            Self::OpenTiles => Nature::Document,
            Self::OpenPath(..) => Nature::Document,
            Self::OpenRemote(..) => Nature::Document,
            Self::OpenRecent(..) => Nature::Document,
            Self::OpenProject => Nature::Document,
            Self::RecoverAnalysis => Nature::DocumentAuthored,
            Self::OpenPackage => Nature::DocumentAuthored,
            Self::RenameObject { set, .. } => {
                Nature::Edit(Edit::targeted(Action::RenameObject, set.raw()))
            }
            Self::ReorderObject { .. } => Nature::Edit(Edit::new(Action::ReorderObject)),
            Self::DeleteObject { set, .. } => {
                Nature::Edit(Edit::targeted(Action::DeleteObject, set.raw()))
            }
            Self::DeleteFiltered => Nature::Edit(Edit::new(Action::DeleteFiltered)),
            Self::ExtractFiltered => Nature::Edit(Edit::new(Action::ExtractFiltered)),
            Self::DuplicateObject(_) => Nature::Edit(Edit::new(Action::DuplicateObject)),
            Self::MergeObjects { target, .. } => {
                Nature::Edit(Edit::targeted(Action::MergeObjects, target.raw()))
            }
            Self::MergeSurfaces { set, .. } => {
                Nature::Edit(Edit::targeted(Action::EditObjects, set.raw()))
            }
            Self::SplitSurface { set, .. } => {
                Nature::Edit(Edit::targeted(Action::EditObjects, set.raw()))
            }
            Self::PaintMask { set, .. } => {
                Nature::Edit(Edit::targeted(Action::MaskEdit, set.raw()))
            }
            Self::ClearMask(set) => Nature::Edit(Edit::targeted(Action::ClearMask, set.raw())),
            Self::AddObject(_) => Nature::Edit(Edit::new(Action::AddObject)),
            Self::AddSegment => Nature::Edit(Edit::new(Action::AddObject)),
            Self::SetSegment { set, .. } => {
                Nature::Edit(Edit::targeted(Action::ObjectParameters, set.raw()))
            }
            Self::AddMaskChannel { .. } => Nature::Edit(Edit::new(Action::Derived)),
            Self::AddRestoreChannel { .. } => Nature::Edit(Edit::new(Action::Derived)),
            Self::AddSmoothChannel { .. } => Nature::Edit(Edit::new(Action::Derived)),
            Self::AddBackgroundChannel { .. } => Nature::Edit(Edit::new(Action::Derived)),
            Self::AddRatioChannel { .. } => Nature::Edit(Edit::new(Action::Derived)),
            Self::AddBleachChannel { .. } => Nature::Edit(Edit::new(Action::Derived)),
            Self::SetSmoothChannel { channel, .. } => {
                Nature::Edit(Edit::targeted(Action::Derived, *channel as u64))
            }
            Self::SetBackgroundChannel { channel, .. } => {
                Nature::Edit(Edit::targeted(Action::Derived, *channel as u64))
            }
            Self::SetRatioChannel { channel, .. } => {
                Nature::Edit(Edit::targeted(Action::Derived, *channel as u64))
            }
            Self::CalibratePsf { .. } => Nature::Edit(Edit::new(Action::Derived)),
            Self::SetRestore { channel, .. } => {
                Nature::Edit(Edit::targeted(Action::Derived, *channel as u64))
            }
            Self::RemoveDerivedChannel { channel } => {
                Nature::Edit(Edit::targeted(Action::Derived, *channel as u64))
            }
            Self::ImportLabels { .. } => Nature::Edit(Edit::new(Action::AddObject)),
            Self::ImportTracks => Nature::Edit(Edit::new(Action::AddObject)),
            Self::ImportLocalizations => Nature::Edit(Edit::new(Action::AddObject)),
            Self::RemoveObject(_) => Nature::Edit(Edit::new(Action::RemoveObject)),
            Self::AddSeed { set, .. } => Nature::Edit(Edit::targeted(Action::Seed, set.raw())),
            Self::RemoveSeed { set, .. } => Nature::Edit(Edit::targeted(Action::Seed, set.raw())),
            Self::ClearSeeds(set) => Nature::Edit(Edit::targeted(Action::Seed, set.raw())),
            Self::SetSeedSource { set, .. } => {
                Nature::Edit(Edit::targeted(Action::SeedSource, set.raw()))
            }
            Self::Detect(set) => Nature::Edit(Edit::targeted(Action::Detect, set.raw())),
            Self::ClearObject(_) => Nature::Edit(Edit::new(Action::ClearObject)),
            Self::AddRelation { target, .. } => {
                Nature::Edit(Edit::targeted(Action::Relation, target.raw()))
            }
            Self::MoveRelation { target, .. } => {
                Nature::Edit(Edit::targeted(Action::Relation, target.raw()))
            }
            Self::RemoveRelation { target, .. } => {
                Nature::Edit(Edit::targeted(Action::Relation, target.raw()))
            }
            Self::Track(set) => Nature::Edit(Edit::targeted(Action::Track, set.raw())),
            Self::ClearTracks(_) => Nature::Edit(Edit::new(Action::ClearTracks)),
            Self::SetObjectHidden { set, .. } => {
                Nature::Edit(Edit::targeted(Action::ObjectHidden, set.raw()))
            }
            Self::ClearHidden(set) => Nature::Edit(Edit::targeted(Action::ObjectHidden, set.raw())),
            Self::SetObjectParams { set, .. } => {
                Nature::Edit(Edit::targeted(Action::ObjectParameters, set.raw()))
            }
            Self::SetMask(_) => Nature::Edit(Edit::new(Action::Mask)),
            Self::SetMaskRadius(_) => Nature::Edit(Edit::new(Action::Mask)),
            Self::SetMaskFilter(_) => Nature::Edit(Edit::new(Action::Mask)),
            Self::AddPoint { .. } => Nature::Edit(Edit::new(Action::Annotation)),
            Self::AddDistance { .. } => Nature::Edit(Edit::new(Action::Annotation)),
            Self::SetAnnotationKind { index, .. } => {
                Nature::Edit(Edit::targeted(Action::Annotation, *index as u64))
            }
            Self::SetAnnotationFrame { index, .. } => {
                Nature::Edit(Edit::targeted(Action::Annotation, *index as u64))
            }
            Self::SampleProfile { index } => {
                Nature::Edit(Edit::targeted(Action::Annotation, *index as u64))
            }
            Self::RemoveAnnotation(_) => Nature::Edit(Edit::new(Action::Annotation)),
            Self::ClearAnnotations => Nature::Edit(Edit::new(Action::Annotation)),
            Self::SetTrackParams { set, .. } => {
                Nature::Edit(Edit::targeted(Action::TrackParameters, set.raw()))
            }
            Self::CutTrack { .. } => Nature::Edit(Edit::new(Action::CutTrack)),
            Self::JoinTracks { .. } => Nature::Edit(Edit::new(Action::JoinTracks)),
            Self::DivideTracks { .. } => Nature::Edit(Edit::new(Action::DivideTracks)),
            Self::MergeTracks { .. } => Nature::Edit(Edit::new(Action::MergeTracks)),
            Self::ReparentTrack { .. } => Nature::Edit(Edit::new(Action::ReparentTrack)),
            Self::DetachTrack { .. } => Nature::Edit(Edit::new(Action::DetachTrack)),
            Self::TrainClassifier { set, .. } => {
                Nature::Edit(Edit::targeted(Action::Classifier, set.raw()))
            }
            Self::ClearClassifier(set) => {
                Nature::Edit(Edit::targeted(Action::Classifier, set.raw()))
            }
            Self::SetColocPair(_) => Nature::Edit(Edit::new(Action::ColocPair)),
            Self::SetColocThresholds(_) => Nature::Edit(Edit::new(Action::ColocThresholds)),
            Self::SetColocSettings(_) => Nature::Edit(Edit::new(Action::ColocSettings)),
            Self::RemoveColocResult(_) => Nature::Edit(Edit::new(Action::ColocResult)),
            Self::ComputeColoc => Nature::Edit(Edit::new(Action::ComputeColoc)),
            Self::ComputeObjectColoc(set) => {
                Nature::Edit(Edit::targeted(Action::ObjectColoc, set.raw()))
            }
            Self::ClearObjectColoc(set) => {
                Nature::Edit(Edit::targeted(Action::ObjectColoc, set.raw()))
            }
            Self::ClusterLocalizations(set) => {
                Nature::Edit(Edit::targeted(Action::Cluster, set.raw()))
            }
            Self::EstimateLocalizationClusters(set) => {
                Nature::Edit(Edit::targeted(Action::Localization, set.raw()))
            }
            Self::ClearDrift => Nature::Edit(Edit::new(Action::Drift)),
            Self::SetDriftReference(_) => Nature::Edit(Edit::new(Action::Drift)),
            Self::SetDriftOffset { .. } => Nature::Edit(Edit::new(Action::Drift)),
            Self::ClearRegistration => Nature::Edit(Edit::new(Action::Registration)),
            Self::ApplyProtocol => Nature::Edit(Edit::new(Action::ApplyProtocol)),
            Self::SaveView => Nature::Edit(Edit::new(Action::NamedView)),
            Self::RemoveView(_) => Nature::Edit(Edit::new(Action::NamedView)),
            Self::ClearKeyframes => Nature::Edit(Edit::new(Action::Keyframe)),
            Self::DeleteKeyframe(_) => Nature::Edit(Edit::new(Action::Keyframe)),
            Self::SetTimelineFps(_) => Nature::Edit(Edit::new(Action::TimelineFps)),
            Self::SetTimelineDuration(_) => Nature::Edit(Edit::new(Action::TimelineDuration)),
            Self::SetChannelEnabled { channel, .. } => Nature::View(Some(Edit::targeted(
                Action::ChannelEnabled,
                *channel as u64,
            ))),
            Self::SetChannelOpacity { channel, .. } => Nature::View(Some(Edit::targeted(
                Action::ChannelOpacity,
                *channel as u64,
            ))),
            Self::SetWindow { channel, .. } => {
                Nature::View(Some(Edit::targeted(Action::Window, *channel as u64)))
            }
            Self::ResetWindow(channel) => {
                Nature::View(Some(Edit::targeted(Action::ResetWindow, *channel as u64)))
            }
            Self::SetTransfer { channel, .. } => {
                Nature::View(Some(Edit::targeted(Action::Transfer, *channel as u64)))
            }
            Self::SetVolumeMode { channel, .. } => {
                Nature::View(Some(Edit::targeted(Action::VolumeMode, *channel as u64)))
            }
            Self::SetIsovalue { channel, .. } => {
                Nature::View(Some(Edit::targeted(Action::Isovalue, *channel as u64)))
            }
            Self::SetObjectVisible { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::ObjectVisible, set.raw())))
            }
            Self::SetObjectLabels { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::ObjectLabels, set.raw())))
            }
            Self::SetLabelMetric { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::ObjectLabels, set.raw())))
            }
            Self::SetObjectColor { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::ObjectColor, set.raw())))
            }
            Self::SetObjectOpacity { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::ObjectOpacity, set.raw())))
            }
            Self::SetObjectMaterial { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::ObjectMaterial, set.raw())))
            }
            Self::SetObjectColoring { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::ObjectColoring, set.raw())))
            }
            Self::SetAnnotationVisible { .. } => Nature::View(Some(Edit::new(Action::Annotation))),
            Self::SetTrackVisible { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::TrackVisible, set.raw())))
            }
            Self::SetTrackThickness { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::TrackThickness, set.raw())))
            }
            Self::SetTrackColoring { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::TrackColoring, set.raw())))
            }
            Self::SetRegion(_) => Nature::View(Some(Edit::new(Action::Region))),
            Self::SetClip(_) => Nature::View(Some(Edit::new(Action::Clip))),
            Self::SetExposure(_) => Nature::View(Some(Edit::new(Action::Exposure))),
            Self::SetStepScale(_) => Nature::View(Some(Edit::new(Action::StepScale))),
            Self::SetShading(_) => Nature::View(Some(Edit::new(Action::Shading))),
            Self::ResetRendering => Nature::View(Some(Edit::new(Action::ResetRendering))),
            Self::SetMeasureSet(_) => Nature::View(Some(Edit::new(Action::MeasureSet))),
            Self::SetMeasureGroup(_) => Nature::View(Some(Edit::new(Action::MeasureGroup))),
            Self::SetMeasureAggregate(_) => Nature::View(Some(Edit::new(Action::MeasureAggregate))),
            Self::SetMeasureScope(_) => Nature::View(Some(Edit::new(Action::MeasureScope))),
            Self::AddFilter => Nature::View(Some(Edit::new(Action::AddFilter))),
            Self::RemoveFilter(_) => Nature::View(Some(Edit::new(Action::RemoveFilter))),
            Self::SetFilter { index, .. } => {
                Nature::View(Some(Edit::targeted(Action::MeasureFilter, *index as u64)))
            }
            Self::RetargetFilter { index, .. } => {
                Nature::View(Some(Edit::targeted(Action::MeasureFilter, *index as u64)))
            }
            Self::SetLasso(_) => Nature::View(Some(Edit::new(Action::MeasureLasso))),
            Self::SetColumnVisible { index, .. } => {
                Nature::View(Some(Edit::targeted(Action::MeasureColumns, *index as u64)))
            }
            Self::ShowAllColumns => Nature::View(Some(Edit::new(Action::MeasureColumns))),
            Self::SetMeasureVisibility(_) => {
                Nature::View(Some(Edit::new(Action::MeasureVisibility)))
            }
            Self::SetMeasureView(_) => Nature::View(Some(Edit::new(Action::MeasureView))),
            Self::SetPlotAxis { column, .. } => {
                Nature::View(Some(Edit::targeted(Action::MeasureAxes, *column as u64)))
            }
            Self::SetGate { column, .. } => {
                Nature::View(Some(Edit::targeted(Action::MeasureFilter, *column as u64)))
            }
            Self::ToggleSort(column) => {
                Nature::View(Some(Edit::targeted(Action::MeasureSort, *column as u64)))
            }
            Self::SetLocalizationSpec { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::Localization, set.raw())))
            }
            Self::SetLocalizationGroupColored { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::Localization, set.raw())))
            }
            Self::SetLocalizationProjected { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::Localization, set.raw())))
            }
            Self::SetLocalizationClusters { set, .. } => {
                Nature::View(Some(Edit::targeted(Action::Localization, set.raw())))
            }
            Self::ApplyView(_) => Nature::View(Some(Edit::new(Action::NamedView))),
            Self::SetExport(_) => Nature::View(Some(Edit::new(Action::Export))),
            Self::SaveSnapshot => Nature::View(None),
            Self::SaveFigure => Nature::View(None),
            Self::SaveLineage => Nature::View(None),
            Self::SaveChart => Nature::View(None),
            Self::SaveComparison => Nature::View(None),
            Self::SetTime(..) => Nature::View(None),
            Self::SelectSeries(..) => Nature::View(None),
            Self::SelectField(..) => Nature::View(None),
            Self::Select(..) => Nature::View(None),
            Self::ToggleSelect(..) => Nature::View(None),
            Self::ExportGeff(..) => Nature::View(None),
            Self::ExportLocalizations(..) => Nature::View(None),
            Self::CancelDetect(..) => Nature::View(None),
            Self::CancelTrack(..) => Nature::View(None),
            Self::SelectChannel(..) => Nature::View(None),
            Self::SelectAnnotation(..) => Nature::View(None),
            Self::SelectTrack { .. } => Nature::View(None),
            Self::CancelColoc => Nature::View(None),
            Self::CancelObjectColoc(..) => Nature::View(None),
            Self::SetLocalizationLayer { .. } => Nature::View(None),
            Self::CancelClusters(..) => Nature::View(None),
            Self::ExportLocalizationComposite => Nature::View(None),
            Self::CancelDrift => Nature::View(None),
            Self::CancelRegistration => Nature::View(None),
            Self::ExportColoc => Nature::View(None),
            Self::ExportMeasure => Nature::View(None),
            Self::ExportProvenance => Nature::View(None),
            Self::ExportPackage => Nature::View(None),
            Self::ExportRelation { .. } => Nature::View(None),
            Self::ExportMesh { .. } => Nature::View(None),
            Self::ExportLabels(..) => Nature::View(None),
            Self::ExportAllLabels => Nature::View(None),
            Self::ExportDataset => Nature::View(None),
            Self::ExportPlate => Nature::View(None),
            Self::CancelStitch => Nature::View(None),
            Self::ExportMovie { .. } => Nature::View(None),
            Self::ExportSequence => Nature::View(None),
            Self::CancelExport => Nature::View(None),
            Self::DiscardAnalysis => Nature::Authored,
            Self::SaveProject => Nature::Authored,
            Self::SaveProjectAs => Nature::Authored,
            Self::AnalyzeLocalizations(..) => Nature::Authored,
            Self::CancelLocalizations(..) => Nature::Authored,
            Self::ClearLocalizations(..) => Nature::Authored,
            Self::ExportLocalizationImage(..) => Nature::Authored,
            Self::ComputeDrift => Nature::Authored,
            Self::ComputeLocalizationDrift(..) => Nature::Authored,
            Self::RegisterChannels(..) => Nature::Authored,
            Self::RegisterFromLocalizations(..) => Nature::Authored,
            Self::CaptureProtocol => Nature::Authored,
            Self::LoadProtocol => Nature::Authored,
            Self::SaveProtocol => Nature::Authored,
            Self::AddBatchDatasets => Nature::Authored,
            Self::ClearBatchDatasets => Nature::Authored,
            Self::ChooseBatchOutput => Nature::Authored,
            Self::OpenBatchRun => Nature::Authored,
            Self::ReviewBatchDataset(..) => Nature::DocumentAuthored,
            Self::RecordBatchCorrection => Nature::Authored,
            Self::ClearBatchCorrection => Nature::Authored,
            Self::SetBatchJobs(..) => Nature::Authored,
            Self::SetBatchProject(..) => Nature::Authored,
            Self::SetBatchChannels(..) => Nature::Authored,
            Self::SetStitchGrid { .. } => Nature::Authored,
            Self::SetStitchChannel(..) => Nature::Authored,
            Self::SetStitchSeries(..) => Nature::Authored,
            Self::ConfirmStitch => Nature::Authored,
            Self::RunBatch => Nature::Authored,
            Self::PauseBatch => Nature::Authored,
            Self::ResumeBatch => Nature::Authored,
            Self::CancelBatch => Nature::Authored,
            Self::SetKeyframe => Nature::Authored,
            Self::Undo => Nature::Authored,
            Self::Redo => Nature::Authored,
        }
    }

    pub fn replaces_document(&self) -> bool {
        matches!(self.nature(), Nature::Document | Nature::DocumentAuthored)
    }

    pub fn authors(&self) -> bool {
        matches!(
            self.nature(),
            Nature::DocumentAuthored | Nature::Authored | Nature::Edit(_)
        )
    }

    pub(crate) fn edit(&self) -> Option<Edit> {
        match self.nature() {
            Nature::Edit(edit) | Nature::View(Some(edit)) => Some(edit),
            Nature::Document | Nature::DocumentAuthored | Nature::View(None) | Nature::Authored => {
                None
            }
        }
    }
}
