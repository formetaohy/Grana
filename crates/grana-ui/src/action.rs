use grana_scene::{ObjectKind, ObjectSetId, Selection};
use grana_volume::Axis;

use crate::panel::Panel;

pub enum Action {
    OpenDataset,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    Fit,
    FitSection,
    SetSectionAxis(Axis),
    SetSplit(f32),
    SetTime(u32),
    Select(Option<Selection>),
    TogglePlay,
    AddObject(ObjectKind),
    RemoveObject(ObjectSetId),
    Detect(ObjectSetId),
    CancelDetect(ObjectSetId),
    ClearObject(ObjectSetId),
    Track(ObjectSetId),
    CancelTrack(ObjectSetId),
    ClearTracks(ObjectSetId),
    ExportMeasure,
    SetPanel(Option<Panel>),
    ComputeColoc,
    CancelColoc,
    ExportColoc,
}
