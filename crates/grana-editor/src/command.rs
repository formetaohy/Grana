use grana_display::CameraProjection;
use grana_session::Command;
use grana_ui::Icon;

use crate::message::Message;
use crate::panel::Panel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    File,
    Edit,
    View,
    Time,
    Protocol,
    Export,
    Batch,
    Analysis,
}

impl Group {
    pub const ALL: [Group; 8] = [
        Group::File,
        Group::Edit,
        Group::View,
        Group::Time,
        Group::Protocol,
        Group::Export,
        Group::Batch,
        Group::Analysis,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::View => "View",
            Self::Time => "Time",
            Self::Protocol => "Protocol",
            Self::Export => "Export",
            Self::Batch => "Batch",
            Self::Analysis => "Analysis",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    key: char,
    shift: bool,
}

impl Shortcut {
    pub const fn new(key: char, shift: bool) -> Self {
        Self { key, shift }
    }

    pub fn matches(self, key: char, shift: bool) -> bool {
        self.key.eq_ignore_ascii_case(&key) && self.shift == shift
    }

    pub fn label(self) -> String {
        let key = self.key.to_ascii_uppercase();
        if cfg!(target_os = "macos") {
            format!("{}{key}", if self.shift { "⇧⌘" } else { "⌘" })
        } else if self.shift {
            format!("Ctrl+Shift+{key}")
        } else {
            format!("Ctrl+{key}")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Always,
    Scene,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    OpenPalette,
    OpenImage,
    OpenFolder,
    OpenRemote,
    OpenProject,
    OpenRecent(usize),
    SaveProject,
    SaveProjectAs,
    SaveSnapshot,
    SaveFigure,
    ExportLabels,
    Undo,
    Redo,
    Appearance(grana_ui::Appearance),
    ToggleContrast,
    ToggleSidebar,
    Fit,
    Panel(Panel),
    ClosePanel,
    Projection(CameraProjection),
    FocusCamera,
    SaveView,
    ApplyView(usize),
    Play,
    Pause,
    Keyframe,
    ClearKeyframes,
    CaptureProtocol,
    LoadProtocol,
    SaveProtocol,
    ApplyProtocol,
    ExportMovie,
    ExportMovieTransparent,
    ExportSequence,
    ExportMeasure,
    ExportProvenance,
    ExportDataset,
    ExportPlate,
    ComputeColoc,
    ComputeDrift,
    RunBatch,
    PauseBatch,
    ResumeBatch,
    CancelBatch,
}

impl Action {
    pub fn message(self) -> Message {
        match self {
            Self::OpenPalette => Message::OpenPalette,
            Self::OpenImage => Message::Command(Command::OpenImage),
            Self::OpenFolder => Message::Command(Command::OpenDataset),
            Self::OpenRemote => Message::BeginRemote,
            Self::OpenProject => Message::Command(Command::OpenProject),
            Self::OpenRecent(index) => Message::Command(Command::OpenRecent(index)),
            Self::SaveProject => Message::Command(Command::SaveProject),
            Self::SaveProjectAs => Message::Command(Command::SaveProjectAs),
            Self::SaveSnapshot => Message::Command(Command::SaveSnapshot),
            Self::SaveFigure => Message::Command(Command::SaveFigure),
            Self::ExportLabels => Message::Command(Command::ExportAllLabels),
            Self::Undo => Message::Command(Command::Undo),
            Self::Redo => Message::Command(Command::Redo),
            Self::Appearance(appearance) => Message::SetAppearance(appearance),
            Self::ToggleContrast => Message::ToggleContrast,
            Self::ToggleSidebar => Message::ToggleSidebar,
            Self::Fit => Message::FitView,
            Self::Panel(panel) => Message::SetPanel(Some(panel)),
            Self::ClosePanel => Message::SetPanel(None),
            Self::Projection(projection) => Message::SetCameraProjection(projection.index()),
            Self::FocusCamera => Message::FocusCamera,
            Self::SaveView => Message::Command(Command::SaveView),
            Self::ApplyView(index) => Message::Command(Command::ApplyView(index)),
            Self::Play => Message::SetPlaying(true),
            Self::Pause => Message::SetPlaying(false),
            Self::Keyframe => Message::Command(Command::SetKeyframe),
            Self::ClearKeyframes => Message::Command(Command::ClearKeyframes),
            Self::CaptureProtocol => Message::Command(Command::CaptureProtocol),
            Self::LoadProtocol => Message::Command(Command::LoadProtocol),
            Self::SaveProtocol => Message::Command(Command::SaveProtocol),
            Self::ApplyProtocol => Message::Command(Command::ApplyProtocol),
            Self::ExportMovie => Message::Command(Command::ExportMovie { transparent: false }),
            Self::ExportMovieTransparent => {
                Message::Command(Command::ExportMovie { transparent: true })
            }
            Self::ExportSequence => Message::Command(Command::ExportSequence),
            Self::ExportMeasure => Message::Command(Command::ExportMeasure),
            Self::ExportProvenance => Message::Command(Command::ExportProvenance),
            Self::ExportDataset => Message::Command(Command::ExportDataset),
            Self::ExportPlate => Message::Command(Command::ExportPlate),
            Self::ComputeColoc => Message::Command(Command::ComputeColoc),
            Self::ComputeDrift => Message::Command(Command::ComputeDrift),
            Self::RunBatch => Message::Command(Command::RunBatch),
            Self::PauseBatch => Message::Command(Command::PauseBatch),
            Self::ResumeBatch => Message::Command(Command::ResumeBatch),
            Self::CancelBatch => Message::Command(Command::CancelBatch),
        }
    }
}

pub struct Spec {
    pub id: &'static str,
    pub title: &'static str,
    pub group: Group,
    pub symbol: Icon,
    pub scope: Scope,
    pub shortcut: Option<Shortcut>,
    pub action: Action,
}

pub const SPECS: &[Spec] = &[
    Spec {
        id: "palette.open",
        title: "Search Commands…",
        group: Group::File,
        symbol: Icon::Search,
        scope: Scope::Always,
        shortcut: Some(Shortcut::new('k', false)),
        action: Action::OpenPalette,
    },
    Spec {
        id: "open.image",
        title: "Open Image…",
        group: Group::File,
        symbol: Icon::Cube,
        scope: Scope::Always,
        shortcut: None,
        action: Action::OpenImage,
    },
    Spec {
        id: "open.folder",
        title: "Open OME-Zarr Folder…",
        group: Group::File,
        symbol: Icon::Folder,
        scope: Scope::Always,
        shortcut: None,
        action: Action::OpenFolder,
    },
    Spec {
        id: "open.remote",
        title: "Open Remote OME-Zarr…",
        group: Group::File,
        symbol: Icon::Globe,
        scope: Scope::Always,
        shortcut: None,
        action: Action::OpenRemote,
    },
    Spec {
        id: "open.project",
        title: "Open Project…",
        group: Group::File,
        symbol: Icon::Folder,
        scope: Scope::Always,
        shortcut: Some(Shortcut::new('o', false)),
        action: Action::OpenProject,
    },
    Spec {
        id: "save.project",
        title: "Save Project",
        group: Group::File,
        symbol: Icon::Save,
        scope: Scope::Always,
        shortcut: Some(Shortcut::new('s', false)),
        action: Action::SaveProject,
    },
    Spec {
        id: "save.as",
        title: "Save Project As…",
        group: Group::File,
        symbol: Icon::Save,
        scope: Scope::Always,
        shortcut: Some(Shortcut::new('s', true)),
        action: Action::SaveProjectAs,
    },
    Spec {
        id: "snapshot",
        title: "Save Snapshot…",
        group: Group::File,
        symbol: Icon::Camera,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::SaveSnapshot,
    },
    Spec {
        id: "figure",
        title: "Save Vector Figure…",
        group: Group::Export,
        symbol: Icon::Export,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::SaveFigure,
    },
    Spec {
        id: "labels",
        title: "Export Labels Only…",
        group: Group::Export,
        symbol: Icon::Export,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportLabels,
    },
    Spec {
        id: "undo",
        title: "Undo",
        group: Group::Edit,
        symbol: Icon::Undo,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('z', false)),
        action: Action::Undo,
    },
    Spec {
        id: "redo",
        title: "Redo",
        group: Group::Edit,
        symbol: Icon::Redo,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('z', true)),
        action: Action::Redo,
    },
    Spec {
        id: "sidebar.toggle",
        title: "Toggle Sidebar",
        group: Group::View,
        symbol: Icon::Sidebar,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ToggleSidebar,
    },
    Spec {
        id: "appearance.system",
        title: "Appearance: Follow System",
        group: Group::View,
        symbol: Icon::Sparkles,
        scope: Scope::Always,
        shortcut: None,
        action: Action::Appearance(grana_ui::Appearance::System),
    },
    Spec {
        id: "appearance.light",
        title: "Appearance: Light",
        group: Group::View,
        symbol: Icon::Sparkles,
        scope: Scope::Always,
        shortcut: None,
        action: Action::Appearance(grana_ui::Appearance::Light),
    },
    Spec {
        id: "appearance.dark",
        title: "Appearance: Dark",
        group: Group::View,
        symbol: Icon::Sparkles,
        scope: Scope::Always,
        shortcut: None,
        action: Action::Appearance(grana_ui::Appearance::Dark),
    },
    Spec {
        id: "appearance.contrast",
        title: "Toggle Increased Contrast",
        group: Group::View,
        symbol: Icon::Eye,
        scope: Scope::Always,
        shortcut: None,
        action: Action::ToggleContrast,
    },
    Spec {
        id: "view.fit",
        title: "Frame the Volume",
        group: Group::View,
        symbol: Icon::Fit,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('f', false)),
        action: Action::Fit,
    },
    Spec {
        id: "panel.close",
        title: "Close Panel",
        group: Group::View,
        symbol: Icon::Close,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('0', false)),
        action: Action::ClosePanel,
    },
    Spec {
        id: "panel.statistics",
        title: "Show Statistics",
        group: Group::View,
        symbol: Icon::Table,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('1', false)),
        action: Action::Panel(Panel::Statistics),
    },
    Spec {
        id: "panel.tracks",
        title: "Show Tracks",
        group: Group::View,
        symbol: Icon::Columns,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('2', false)),
        action: Action::Panel(Panel::Tracks),
    },
    Spec {
        id: "panel.colocalization",
        title: "Show Colocalization",
        group: Group::View,
        symbol: Icon::Target,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('3', false)),
        action: Action::Panel(Panel::Colocalization),
    },
    Spec {
        id: "panel.relations",
        title: "Show Relations",
        group: Group::View,
        symbol: Icon::Grid,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('4', false)),
        action: Action::Panel(Panel::Relations),
    },
    Spec {
        id: "panel.annotations",
        title: "Show Annotations",
        group: Group::View,
        symbol: Icon::Pencil,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('5', false)),
        action: Action::Panel(Panel::Annotations),
    },
    Spec {
        id: "panel.plate",
        title: "Show Plate",
        group: Group::View,
        symbol: Icon::Grid,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('6', false)),
        action: Action::Panel(Panel::Plate),
    },
    Spec {
        id: "panel.batch",
        title: "Show Batch",
        group: Group::View,
        symbol: Icon::Layers,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('7', false)),
        action: Action::Panel(Panel::Batch),
    },
    Spec {
        id: "panel.animation",
        title: "Show Animation",
        group: Group::View,
        symbol: Icon::Film,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('8', false)),
        action: Action::Panel(Panel::Animation),
    },
    Spec {
        id: "camera.perspective",
        title: "Perspective Projection",
        group: Group::View,
        symbol: Icon::Cube,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::Projection(CameraProjection::Perspective),
    },
    Spec {
        id: "camera.orthographic",
        title: "Orthographic Projection",
        group: Group::View,
        symbol: Icon::Cube,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::Projection(CameraProjection::Orthographic),
    },
    Spec {
        id: "camera.focus",
        title: "Focus Camera on Pivot",
        group: Group::View,
        symbol: Icon::Target,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::FocusCamera,
    },
    Spec {
        id: "view.save",
        title: "Save View",
        group: Group::View,
        symbol: Icon::Camera,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::SaveView,
    },
    Spec {
        id: "keyframe.save",
        title: "Save Keyframe",
        group: Group::Time,
        symbol: Icon::Record,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::Keyframe,
    },
    Spec {
        id: "keyframes.clear",
        title: "Clear Keyframes",
        group: Group::Time,
        symbol: Icon::Trash,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ClearKeyframes,
    },
    Spec {
        id: "protocol.capture",
        title: "Capture Protocol",
        group: Group::Protocol,
        symbol: Icon::Record,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::CaptureProtocol,
    },
    Spec {
        id: "protocol.load",
        title: "Load Protocol…",
        group: Group::Protocol,
        symbol: Icon::Folder,
        scope: Scope::Always,
        shortcut: None,
        action: Action::LoadProtocol,
    },
    Spec {
        id: "protocol.save",
        title: "Save Protocol…",
        group: Group::Protocol,
        symbol: Icon::Save,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::SaveProtocol,
    },
    Spec {
        id: "protocol.apply",
        title: "Apply Protocol",
        group: Group::Protocol,
        symbol: Icon::Play,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ApplyProtocol,
    },
    Spec {
        id: "movie",
        title: "Export Movie…",
        group: Group::Export,
        symbol: Icon::Film,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportMovie,
    },
    Spec {
        id: "movie.transparent",
        title: "Export Transparent Movie…",
        group: Group::Export,
        symbol: Icon::Film,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportMovieTransparent,
    },
    Spec {
        id: "sequence",
        title: "Export Frame Sequence…",
        group: Group::Export,
        symbol: Icon::Film,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportSequence,
    },
    Spec {
        id: "measure.export",
        title: "Export Measurements…",
        group: Group::Export,
        symbol: Icon::Table,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportMeasure,
    },
    Spec {
        id: "provenance.export",
        title: "Export Provenance Crate…",
        group: Group::Export,
        symbol: Icon::Info,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportProvenance,
    },
    Spec {
        id: "dataset.export",
        title: "Export Dataset…",
        group: Group::Export,
        symbol: Icon::Export,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportDataset,
    },
    Spec {
        id: "plate.export",
        title: "Export Plate…",
        group: Group::Export,
        symbol: Icon::Grid,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportPlate,
    },
    Spec {
        id: "batch.run",
        title: "Run Batch",
        group: Group::Batch,
        symbol: Icon::Play,
        scope: Scope::Always,
        shortcut: None,
        action: Action::RunBatch,
    },
    Spec {
        id: "batch.pause",
        title: "Pause Batch",
        group: Group::Batch,
        symbol: Icon::Pause,
        scope: Scope::Always,
        shortcut: None,
        action: Action::PauseBatch,
    },
    Spec {
        id: "batch.resume",
        title: "Resume Batch",
        group: Group::Batch,
        symbol: Icon::Play,
        scope: Scope::Always,
        shortcut: None,
        action: Action::ResumeBatch,
    },
    Spec {
        id: "batch.cancel",
        title: "Cancel Batch",
        group: Group::Batch,
        symbol: Icon::Close,
        scope: Scope::Always,
        shortcut: None,
        action: Action::CancelBatch,
    },
    Spec {
        id: "coloc.compute",
        title: "Compute Colocalization",
        group: Group::Analysis,
        symbol: Icon::Target,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ComputeColoc,
    },
    Spec {
        id: "drift.compute",
        title: "Correct Drift",
        group: Group::Analysis,
        symbol: Icon::Ruler,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ComputeDrift,
    },
];

pub fn spec(id: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|spec| spec.id == id)
}

const ALIASES: &[(Shortcut, Action)] = &[(Shortcut::new('y', false), Action::Redo)];

pub fn shortcut(key: char, shift: bool) -> Option<Message> {
    SPECS
        .iter()
        .find(|spec| spec.shortcut.is_some_and(|value| value.matches(key, shift)))
        .map(|spec| spec.action.message())
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(value, _)| value.matches(key, shift))
                .map(|(_, action)| action.message())
        })
}
