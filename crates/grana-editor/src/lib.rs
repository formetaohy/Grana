mod animation_panel;
mod annotation_panel;
mod batch_panel;
mod coloc_panel;
mod coloring;
mod fitting;
mod inspector;
mod measure_panel;
mod message;
mod object_panel;
mod panel;
mod plot;
mod shell;
mod stage;
mod state;
mod track_panel;
mod transfer;
mod view;
pub use grana_ui::widget;

pub use message::{Message, PlotEvent};
pub use panel::{Panel, build};
pub use stage::{GrabDrag, Interaction, Pane, Stage, ToolDrag};
pub use state::{
    AnnotationTool, FrameStats, Playback, ProfileStats, Sections, View, ViewStats, Workspace,
};

pub(crate) fn section_preset(index: usize) -> (f32, f32) {
    match index {
        1 => (-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2),
        2 => (std::f32::consts::FRAC_PI_2, 0.0),
        _ => (0.0, 0.0),
    }
}
