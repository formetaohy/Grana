use grana_session::Command;
use grana_ui::Icon;

use crate::message::Message;
use crate::panel::Panel;

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
pub enum Action {
    OpenImage,
    OpenTiles,
    OpenFolder,
    OpenRemote,
    OpenProject,
    OpenPackage,
    SaveProject,
    SaveProjectAs,
    SaveSnapshot,
    SaveFigure,
    ExportLabels,
    Undo,
    Redo,
    Fit,
    Panel(Panel),
    ClosePanel,
}

impl Action {
    pub fn message(self) -> Message {
        match self {
            Self::OpenImage => Message::Command(Command::OpenImage),
            Self::OpenTiles => Message::Command(Command::OpenTiles),
            Self::OpenFolder => Message::Command(Command::OpenDataset),
            Self::OpenRemote => Message::BeginRemote,
            Self::OpenProject => Message::Command(Command::OpenProject),
            Self::OpenPackage => Message::Command(Command::OpenPackage),
            Self::SaveProject => Message::Command(Command::SaveProject),
            Self::SaveProjectAs => Message::Command(Command::SaveProjectAs),
            Self::SaveSnapshot => Message::Command(Command::SaveSnapshot),
            Self::SaveFigure => Message::Command(Command::SaveFigure),
            Self::ExportLabels => Message::Command(Command::ExportAllLabels),
            Self::Undo => Message::Command(Command::Undo),
            Self::Redo => Message::Command(Command::Redo),
            Self::Fit => Message::FitView,
            Self::Panel(panel) => Message::SetPanel(Some(panel)),
            Self::ClosePanel => Message::SetPanel(None),
        }
    }
}

pub struct Spec {
    pub id: &'static str,
    pub symbol: Icon,
    pub shortcut: Option<Shortcut>,
    pub action: Action,
}

pub const SPECS: &[Spec] = &[
    Spec {
        id: "open.image",
        symbol: Icon::Cube,
        shortcut: None,
        action: Action::OpenImage,
    },
    Spec {
        id: "open.tiles",
        symbol: Icon::Grid,
        shortcut: None,
        action: Action::OpenTiles,
    },
    Spec {
        id: "open.folder",
        symbol: Icon::Folder,
        shortcut: None,
        action: Action::OpenFolder,
    },
    Spec {
        id: "open.remote",
        symbol: Icon::Globe,
        shortcut: None,
        action: Action::OpenRemote,
    },
    Spec {
        id: "open.project",
        symbol: Icon::Folder,
        shortcut: Some(Shortcut::new('o', false)),
        action: Action::OpenProject,
    },
    Spec {
        id: "package.open",
        symbol: Icon::Folder,
        shortcut: None,
        action: Action::OpenPackage,
    },
    Spec {
        id: "save.project",
        symbol: Icon::Save,
        shortcut: Some(Shortcut::new('s', false)),
        action: Action::SaveProject,
    },
    Spec {
        id: "save.as",
        symbol: Icon::Save,
        shortcut: Some(Shortcut::new('s', true)),
        action: Action::SaveProjectAs,
    },
    Spec {
        id: "snapshot",
        symbol: Icon::Camera,
        shortcut: None,
        action: Action::SaveSnapshot,
    },
    Spec {
        id: "figure",
        symbol: Icon::Export,
        shortcut: None,
        action: Action::SaveFigure,
    },
    Spec {
        id: "labels",
        symbol: Icon::Export,
        shortcut: None,
        action: Action::ExportLabels,
    },
    Spec {
        id: "undo",
        symbol: Icon::Undo,
        shortcut: Some(Shortcut::new('z', false)),
        action: Action::Undo,
    },
    Spec {
        id: "redo",
        symbol: Icon::Redo,
        shortcut: Some(Shortcut::new('z', true)),
        action: Action::Redo,
    },
    Spec {
        id: "view.fit",
        symbol: Icon::Fit,
        shortcut: Some(Shortcut::new('f', false)),
        action: Action::Fit,
    },
    Spec {
        id: "panel.close",
        symbol: Icon::Close,
        shortcut: Some(Shortcut::new('0', false)),
        action: Action::ClosePanel,
    },
    Spec {
        id: "panel.statistics",
        symbol: Icon::Table,
        shortcut: Some(Shortcut::new('1', false)),
        action: Action::Panel(Panel::Statistics),
    },
    Spec {
        id: "panel.tracks",
        symbol: Icon::Columns,
        shortcut: Some(Shortcut::new('2', false)),
        action: Action::Panel(Panel::Tracks),
    },
    Spec {
        id: "panel.colocalization",
        symbol: Icon::Target,
        shortcut: Some(Shortcut::new('3', false)),
        action: Action::Panel(Panel::Colocalization),
    },
    Spec {
        id: "panel.relations",
        symbol: Icon::Grid,
        shortcut: Some(Shortcut::new('4', false)),
        action: Action::Panel(Panel::Relations),
    },
    Spec {
        id: "panel.annotations",
        symbol: Icon::Pencil,
        shortcut: Some(Shortcut::new('5', false)),
        action: Action::Panel(Panel::Annotations),
    },
    Spec {
        id: "panel.plate",
        symbol: Icon::Grid,
        shortcut: Some(Shortcut::new('6', false)),
        action: Action::Panel(Panel::Plate),
    },
    Spec {
        id: "panel.batch",
        symbol: Icon::Layers,
        shortcut: Some(Shortcut::new('7', false)),
        action: Action::Panel(Panel::Batch),
    },
    Spec {
        id: "panel.animation",
        symbol: Icon::Film,
        shortcut: Some(Shortcut::new('8', false)),
        action: Action::Panel(Panel::Animation),
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
