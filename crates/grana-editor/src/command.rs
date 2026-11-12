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

    pub fn title(self, language: grana_i18n::Language) -> &'static str {
        match self {
            Self::File => grana_i18n::palette::file(language),
            Self::Edit => grana_i18n::palette::edit(language),
            Self::View => grana_i18n::palette::view(language),
            Self::Time => grana_i18n::palette::time(language),
            Self::Protocol => grana_i18n::palette::protocol(language),
            Self::Export => grana_i18n::palette::export(language),
            Self::Batch => grana_i18n::palette::batch(language),
            Self::Analysis => grana_i18n::palette::analysis(language),
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
    OpenTiles,
    OpenFolder,
    OpenRemote,
    OpenProject,
    OpenRecent(usize),
    SaveProject,
    SaveProjectAs,
    SaveSnapshot,
    SaveFigure,
    SaveLineage,
    ExportLabels,
    Undo,
    Redo,
    Language(grana_i18n::Language),
    ToggleContrast,
    ToggleReduceMotion,
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
    pub fn authors(self) -> bool {
        matches!(
            self,
            Self::OpenTiles
                | Self::SaveProject
                | Self::SaveProjectAs
                | Self::SaveView
                | Self::Keyframe
                | Self::ClearKeyframes
                | Self::CaptureProtocol
                | Self::LoadProtocol
                | Self::SaveProtocol
                | Self::ApplyProtocol
                | Self::Undo
                | Self::Redo
                | Self::ComputeColoc
                | Self::ComputeDrift
                | Self::RunBatch
                | Self::PauseBatch
                | Self::ResumeBatch
                | Self::CancelBatch
        )
    }

    pub fn message(self) -> Message {
        match self {
            Self::OpenPalette => Message::OpenPalette,
            Self::OpenImage => Message::Command(Command::OpenImage),
            Self::OpenTiles => Message::Command(Command::OpenTiles),
            Self::OpenFolder => Message::Command(Command::OpenDataset),
            Self::OpenRemote => Message::BeginRemote,
            Self::OpenProject => Message::Command(Command::OpenProject),
            Self::OpenRecent(index) => Message::Command(Command::OpenRecent(index)),
            Self::SaveProject => Message::Command(Command::SaveProject),
            Self::SaveProjectAs => Message::Command(Command::SaveProjectAs),
            Self::SaveSnapshot => Message::Command(Command::SaveSnapshot),
            Self::SaveFigure => Message::Command(Command::SaveFigure),
            Self::SaveLineage => Message::Command(Command::SaveLineage),
            Self::ExportLabels => Message::Command(Command::ExportAllLabels),
            Self::Undo => Message::Command(Command::Undo),
            Self::Redo => Message::Command(Command::Redo),
            Self::Language(language) => Message::SetLanguage(language),
            Self::ToggleContrast => Message::ToggleContrast,
            Self::ToggleReduceMotion => Message::ToggleReduceMotion,
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
    pub group: Group,
    pub symbol: Icon,
    pub scope: Scope,
    pub shortcut: Option<Shortcut>,
    pub action: Action,
}

pub const SPECS: &[Spec] = &[
    Spec {
        id: "palette.open",
        group: Group::File,
        symbol: Icon::Search,
        scope: Scope::Always,
        shortcut: Some(Shortcut::new('k', false)),
        action: Action::OpenPalette,
    },
    Spec {
        id: "open.image",
        group: Group::File,
        symbol: Icon::Cube,
        scope: Scope::Always,
        shortcut: None,
        action: Action::OpenImage,
    },
    Spec {
        id: "open.tiles",
        group: Group::File,
        symbol: Icon::Grid,
        scope: Scope::Always,
        shortcut: None,
        action: Action::OpenTiles,
    },
    Spec {
        id: "open.folder",
        group: Group::File,
        symbol: Icon::Folder,
        scope: Scope::Always,
        shortcut: None,
        action: Action::OpenFolder,
    },
    Spec {
        id: "open.remote",
        group: Group::File,
        symbol: Icon::Globe,
        scope: Scope::Always,
        shortcut: None,
        action: Action::OpenRemote,
    },
    Spec {
        id: "open.project",
        group: Group::File,
        symbol: Icon::Folder,
        scope: Scope::Always,
        shortcut: Some(Shortcut::new('o', false)),
        action: Action::OpenProject,
    },
    Spec {
        id: "save.project",
        group: Group::File,
        symbol: Icon::Save,
        scope: Scope::Always,
        shortcut: Some(Shortcut::new('s', false)),
        action: Action::SaveProject,
    },
    Spec {
        id: "save.as",
        group: Group::File,
        symbol: Icon::Save,
        scope: Scope::Always,
        shortcut: Some(Shortcut::new('s', true)),
        action: Action::SaveProjectAs,
    },
    Spec {
        id: "snapshot",
        group: Group::File,
        symbol: Icon::Camera,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::SaveSnapshot,
    },
    Spec {
        id: "figure",
        group: Group::Export,
        symbol: Icon::Export,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::SaveFigure,
    },
    Spec {
        id: "lineage",
        group: Group::Export,
        symbol: Icon::Target,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::SaveLineage,
    },
    Spec {
        id: "labels",
        group: Group::Export,
        symbol: Icon::Export,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportLabels,
    },
    Spec {
        id: "undo",
        group: Group::Edit,
        symbol: Icon::Undo,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('z', false)),
        action: Action::Undo,
    },
    Spec {
        id: "redo",
        group: Group::Edit,
        symbol: Icon::Redo,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('z', true)),
        action: Action::Redo,
    },
    Spec {
        id: "sidebar.toggle",
        group: Group::View,
        symbol: Icon::Sidebar,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ToggleSidebar,
    },
    Spec {
        id: "language.english",
        group: Group::View,
        symbol: Icon::Globe,
        scope: Scope::Always,
        shortcut: None,
        action: Action::Language(grana_i18n::Language::English),
    },
    Spec {
        id: "language.chinese",
        group: Group::View,
        symbol: Icon::Globe,
        scope: Scope::Always,
        shortcut: None,
        action: Action::Language(grana_i18n::Language::Chinese),
    },
    Spec {
        id: "appearance.contrast",
        group: Group::View,
        symbol: Icon::Eye,
        scope: Scope::Always,
        shortcut: None,
        action: Action::ToggleContrast,
    },
    Spec {
        id: "appearance.motion",
        group: Group::View,
        symbol: Icon::Sparkles,
        scope: Scope::Always,
        shortcut: None,
        action: Action::ToggleReduceMotion,
    },
    Spec {
        id: "view.fit",
        group: Group::View,
        symbol: Icon::Fit,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('f', false)),
        action: Action::Fit,
    },
    Spec {
        id: "panel.close",
        group: Group::View,
        symbol: Icon::Close,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('0', false)),
        action: Action::ClosePanel,
    },
    Spec {
        id: "panel.statistics",
        group: Group::View,
        symbol: Icon::Table,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('1', false)),
        action: Action::Panel(Panel::Statistics),
    },
    Spec {
        id: "panel.tracks",
        group: Group::View,
        symbol: Icon::Columns,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('2', false)),
        action: Action::Panel(Panel::Tracks),
    },
    Spec {
        id: "panel.colocalization",
        group: Group::View,
        symbol: Icon::Target,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('3', false)),
        action: Action::Panel(Panel::Colocalization),
    },
    Spec {
        id: "panel.relations",
        group: Group::View,
        symbol: Icon::Grid,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('4', false)),
        action: Action::Panel(Panel::Relations),
    },
    Spec {
        id: "panel.annotations",
        group: Group::View,
        symbol: Icon::Pencil,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('5', false)),
        action: Action::Panel(Panel::Annotations),
    },
    Spec {
        id: "panel.plate",
        group: Group::View,
        symbol: Icon::Grid,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('6', false)),
        action: Action::Panel(Panel::Plate),
    },
    Spec {
        id: "panel.batch",
        group: Group::View,
        symbol: Icon::Layers,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('7', false)),
        action: Action::Panel(Panel::Batch),
    },
    Spec {
        id: "panel.animation",
        group: Group::View,
        symbol: Icon::Film,
        scope: Scope::Scene,
        shortcut: Some(Shortcut::new('8', false)),
        action: Action::Panel(Panel::Animation),
    },
    Spec {
        id: "camera.perspective",
        group: Group::View,
        symbol: Icon::Cube,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::Projection(CameraProjection::Perspective),
    },
    Spec {
        id: "camera.orthographic",
        group: Group::View,
        symbol: Icon::Cube,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::Projection(CameraProjection::Orthographic),
    },
    Spec {
        id: "camera.focus",
        group: Group::View,
        symbol: Icon::Target,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::FocusCamera,
    },
    Spec {
        id: "view.save",
        group: Group::View,
        symbol: Icon::Camera,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::SaveView,
    },
    Spec {
        id: "keyframe.save",
        group: Group::Time,
        symbol: Icon::Record,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::Keyframe,
    },
    Spec {
        id: "keyframes.clear",
        group: Group::Time,
        symbol: Icon::Trash,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ClearKeyframes,
    },
    Spec {
        id: "protocol.capture",
        group: Group::Protocol,
        symbol: Icon::Record,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::CaptureProtocol,
    },
    Spec {
        id: "protocol.load",
        group: Group::Protocol,
        symbol: Icon::Folder,
        scope: Scope::Always,
        shortcut: None,
        action: Action::LoadProtocol,
    },
    Spec {
        id: "protocol.save",
        group: Group::Protocol,
        symbol: Icon::Save,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::SaveProtocol,
    },
    Spec {
        id: "protocol.apply",
        group: Group::Protocol,
        symbol: Icon::Play,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ApplyProtocol,
    },
    Spec {
        id: "movie",
        group: Group::Export,
        symbol: Icon::Film,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportMovie,
    },
    Spec {
        id: "movie.transparent",
        group: Group::Export,
        symbol: Icon::Film,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportMovieTransparent,
    },
    Spec {
        id: "sequence",
        group: Group::Export,
        symbol: Icon::Film,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportSequence,
    },
    Spec {
        id: "measure.export",
        group: Group::Export,
        symbol: Icon::Table,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportMeasure,
    },
    Spec {
        id: "provenance.export",
        group: Group::Export,
        symbol: Icon::Info,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportProvenance,
    },
    Spec {
        id: "dataset.export",
        group: Group::Export,
        symbol: Icon::Export,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportDataset,
    },
    Spec {
        id: "plate.export",
        group: Group::Export,
        symbol: Icon::Grid,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ExportPlate,
    },
    Spec {
        id: "batch.run",
        group: Group::Batch,
        symbol: Icon::Play,
        scope: Scope::Always,
        shortcut: None,
        action: Action::RunBatch,
    },
    Spec {
        id: "batch.pause",
        group: Group::Batch,
        symbol: Icon::Pause,
        scope: Scope::Always,
        shortcut: None,
        action: Action::PauseBatch,
    },
    Spec {
        id: "batch.resume",
        group: Group::Batch,
        symbol: Icon::Play,
        scope: Scope::Always,
        shortcut: None,
        action: Action::ResumeBatch,
    },
    Spec {
        id: "batch.cancel",
        group: Group::Batch,
        symbol: Icon::Close,
        scope: Scope::Always,
        shortcut: None,
        action: Action::CancelBatch,
    },
    Spec {
        id: "coloc.compute",
        group: Group::Analysis,
        symbol: Icon::Target,
        scope: Scope::Scene,
        shortcut: None,
        action: Action::ComputeColoc,
    },
    Spec {
        id: "drift.compute",
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
