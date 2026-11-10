mod animation_panel;
mod annotation_panel;
mod batch_panel;
mod coloc_panel;
mod coloring;
mod command;
mod fitting;
mod inspector;
mod measure_panel;
mod message;
mod object_panel;
mod palette;
mod panel;
mod plate_panel;
mod plot;
mod relation_panel;
mod shell;
mod stage;
mod state;
mod track_panel;
mod transfer;
mod view;
pub use grana_ui::widget;

pub use command::shortcut;
pub use message::{Message, PlotEvent};
pub use panel::{Panel, apply, build};
pub use stage::{GrabDrag, Intent, Interaction, PaintDrag, Stage, ToolDrag};
pub use state::{
    AnnotationTool, ComparisonMetric, FrameStats, MaskBrush, MaskBrushTool, Palette, Playback,
    ProfileStats, RelationMetric, Sections, View, ViewStats, Workspace,
};

pub(crate) const RECENT_LIMIT: usize = 5;
