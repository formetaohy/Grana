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
    SetPanel(Option<Panel>),
    SetSidebarTab(usize),
    ToggleSidebar,
    SetDockHeight(f32),
    SetSplit(f32),
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
    SetPreviewing(bool),
    SetPlaybackPosition(f32),
    SetPlaybackWidth(u32),
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
    SetClassifierFeatures {
        set: ObjectSetId,
        features: Vec<grana_measure::Measurement>,
    },
    FitVolume,
    FitSection,
    Plot(PlotEvent),
    Locate(usize),
    ClearGates,
    SetExport(grana_session::Export),
    BeginRemote,
    CancelRemote,
    OpenRemote(String),
    SetSectionPreset(usize),
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
