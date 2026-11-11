use glam::Vec2;
use grana_scene::ObjectSetId;
use grana_session::Command;
use grana_ui::Key;

use crate::panel::Panel;
use crate::state::AnnotationTool;

#[derive(Clone, Copy, Debug)]
pub struct PlotEvent {
    pub phase: grana_ui::Phase,
    pub position: Vec2,
    pub shift: bool,
}

#[derive(Clone)]
pub enum Message {
    Noop,
    Copy(String),
    Command(Command),
    OpenPalette,
    ClosePalette,
    RunSelected,
    RunPaletteIndex(usize),
    SetPanel(Option<Panel>),
    SetSidebarTab(usize),
    ToggleSidebar,
    SetDockHeight(f32),
    SetColumnWidth {
        index: usize,
        width: f32,
    },
    ToggleOpen(Key),
    ExpandObject {
        set: ObjectSetId,
        open: bool,
    },
    BeginRename(ObjectSetId),
    EndRename,
    RenameObject {
        set: ObjectSetId,
        name: String,
    },
    SetPlaying(bool),
    SetLanguage(grana_i18n::Language),
    SetContrast(bool),
    ToggleContrast,
    SetReduceTransparency(bool),
    ToggleReduceTransparency,
    SetPreviewing(bool),
    SetPlaybackPosition(f32),
    SetPlaybackSelected(Option<usize>),
    SetTrackSet(Option<ObjectSetId>),
    SetJoin(Option<(u32, usize)>),
    SetDivide(Option<(u32, usize)>),
    PickDivide {
        set: ObjectSetId,
        daughter: (u32, usize),
    },
    SetMerge(Option<(u32, usize)>),
    PickMerge {
        set: ObjectSetId,
        parent: (u32, usize),
    },
    SetReparent(Option<(u32, usize)>),
    PickReparent {
        set: ObjectSetId,
        parent: (u32, usize),
    },
    CancelLineage,
    ExportLineage(ObjectSetId),
    SetSeed(Option<ObjectSetId>),
    ArmMask(ObjectSetId),
    DisarmMask,
    SetMaskTool(usize),
    SetMaskRadius(f32),
    SetMaskTolerance(f32),
    SetAnnotationTool(Option<AnnotationTool>),
    SetRelation {
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    },
    OpenRelation {
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
    },
    SetRelationMetric(usize),
    SetRelationFilter(f32),
    SetDistributionStyle(usize),
    SetComparisonMetric(usize),
    SetComparisonAxis(usize),
    SetComparisonGroup(Option<String>),
    SetComparisonKey(Option<String>),
    ToggleComparisonDetail {
        group: String,
        key: String,
    },
    SelectPair {
        source: ObjectSetId,
        target: ObjectSetId,
        objects: (usize, usize),
    },
    SetClassifierFeatures {
        set: ObjectSetId,
        features: Vec<grana_measure::Measurement>,
    },
    FitView,
    Plot(PlotEvent),
    Locate(usize),
    ClearGates,
    SetExport(grana_session::Export),
    BeginRemote,
    CancelRemote,
    OpenRemote(String),
    SetCameraProjection(usize),
    ToggleSlice(bool),
    SetSliceAxis(usize),
    SetSliceDepth(f32),
    StepSlice(i32),
    SetAperture(f32),
    SetFocus(f32),
    FocusCamera,
    SetPsfThreshold(f32),
    SetPsfSeparation(f32),
    Region {
        axis: usize,
        side: usize,
        delta: f32,
    },
}
