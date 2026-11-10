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
    SetSplit(f32),
    SetRow(f32),
    SetLayout(grana_display::PaneLayout),
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
    SetAppearance(grana_ui::Appearance),
    SetContrast(bool),
    ToggleContrast,
    SetPreviewing(bool),
    SetPlaybackPosition(f32),
    SetPlaybackSelected(Option<usize>),
    SetTrackSet(Option<ObjectSetId>),
    SetJoin(Option<(u32, usize)>),
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
    SetComparisonMetric(usize),
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
    FitPane(usize),
    SetActivePane(usize),
    SetPaneKind {
        index: usize,
        kind: grana_display::PaneKind,
    },
    SetPaneSlice {
        index: usize,
        preset: usize,
    },
    SetPaneChannel {
        index: usize,
        channel: usize,
        shown: bool,
    },
    SetPaneQuality {
        index: usize,
        quality: grana_display::Quality,
    },
    Plot(PlotEvent),
    Locate(usize),
    ClearGates,
    SetExport(grana_session::Export),
    BeginRemote,
    CancelRemote,
    OpenRemote(String),
    SetSectionPreset(usize),
    SetCameraProjection(usize),
    SetAperture(f32),
    SetFocus(f32),
    FocusCamera,
    SetSectionProjection(usize),
    SetSectionThickness(f32),
    SetSectionOrientation {
        azimuth: f32,
        elevation: f32,
    },
    SetSectionDepth(f32),
    SetPsfThreshold(f32),
    SetPsfSeparation(f32),
    Region {
        axis: usize,
        side: usize,
        delta: f32,
    },
}
