use glam::Vec2;
use grana_text::GlyphStyle;

use crate::color::Color;
use crate::paint::Painter;

mod symbol {
    pub(super) const ADD: char = '\u{e145}';
    pub(super) const AUTO_AWESOME: char = '\u{e65f}';
    pub(super) const BAR_CHART: char = '\u{e26b}';
    pub(super) const CHECK: char = '\u{e668}';
    pub(super) const CHEVRON_LEFT: char = '\u{e5cb}';
    pub(super) const CHEVRON_RIGHT: char = '\u{e5cc}';
    pub(super) const CLOSE: char = '\u{e5cd}';
    pub(super) const CONTENT_COPY: char = '\u{e14d}';
    pub(super) const CONTENT_CUT: char = '\u{e14e}';
    pub(super) const CROP_SQUARE: char = '\u{e3c6}';
    pub(super) const DELETE: char = '\u{e92e}';
    pub(super) const DEPLOYED_CODE: char = '\u{f720}';
    pub(super) const EDIT: char = '\u{f097}';
    pub(super) const EXPAND_LESS: char = '\u{e5ce}';
    pub(super) const EXPAND_MORE: char = '\u{e5cf}';
    pub(super) const FIBER_MANUAL_RECORD: char = '\u{e061}';
    pub(super) const FILE_UPLOAD: char = '\u{f09b}';
    pub(super) const FILTER_ALT: char = '\u{ef4f}';
    pub(super) const FILTER_NONE: char = '\u{e3e0}';
    pub(super) const FOLDER: char = '\u{e2c7}';
    pub(super) const GRAPHIC_EQ: char = '\u{e1b8}';
    pub(super) const GRID_VIEW: char = '\u{e9b0}';
    pub(super) const INFO: char = '\u{e88e}';
    pub(super) const LAYERS: char = '\u{e53b}';
    pub(super) const LOCK: char = '\u{e899}';
    pub(super) const MORE_HORIZ: char = '\u{e5d3}';
    pub(super) const MOVIE: char = '\u{e404}';
    pub(super) const MY_LOCATION: char = '\u{e55c}';
    pub(super) const PAUSE: char = '\u{e034}';
    pub(super) const PHOTO_CAMERA: char = '\u{e412}';
    pub(super) const PLAY_ARROW: char = '\u{e037}';
    pub(super) const PUBLIC: char = '\u{e80b}';
    pub(super) const REDO: char = '\u{e15a}';
    pub(super) const REMOVE: char = '\u{e15b}';
    pub(super) const RESTART_ALT: char = '\u{f053}';
    pub(super) const SAVE: char = '\u{e161}';
    pub(super) const SCHEDULE: char = '\u{efd6}';
    pub(super) const SEARCH: char = '\u{ef7a}';
    pub(super) const SKIP_NEXT: char = '\u{e044}';
    pub(super) const SKIP_PREVIOUS: char = '\u{e045}';
    pub(super) const STRAIGHTEN: char = '\u{e41c}';
    pub(super) const TABLE: char = '\u{f191}';
    pub(super) const TUNE: char = '\u{e429}';
    pub(super) const UNDO: char = '\u{e166}';
    pub(super) const VIEW_COLUMN: char = '\u{e8ec}';
    pub(super) const VIEW_SIDEBAR: char = '\u{f114}';
    pub(super) const VISIBILITY: char = '\u{e8f4}';
    pub(super) const VISIBILITY_OFF: char = '\u{e8f5}';
    pub(super) const WARNING: char = '\u{f083}';
    pub(super) const ZOOM_OUT_MAP: char = '\u{e56b}';
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    ChevronDown,
    ChevronUp,
    ChevronLeft,
    ChevronRight,
    Plus,
    Minus,
    Close,
    Check,
    Ellipsis,
    Play,
    Pause,
    StepBack,
    StepForward,
    Record,
    Eye,
    EyeOff,
    Folder,
    Save,
    Copy,
    Camera,
    Globe,
    Film,
    Export,
    Trash,
    Cube,
    Section,
    Fit,
    Reset,
    Layers,
    Target,
    Search,
    Sliders,
    Table,
    Chart,
    Grid,
    Sparkles,
    Info,
    Warning,
    Clock,
    Lock,
    Channel,
    Filter,
    Columns,
    Sidebar,
    Undo,
    Redo,
    Pencil,
    Ruler,
    WindowMinimize,
    WindowMaximize,
    WindowRestore,
}

impl Icon {
    pub const fn character(self) -> char {
        match self {
            Self::ChevronDown => symbol::EXPAND_MORE,
            Self::ChevronUp => symbol::EXPAND_LESS,
            Self::ChevronLeft => symbol::CHEVRON_LEFT,
            Self::ChevronRight => symbol::CHEVRON_RIGHT,
            Self::Plus => symbol::ADD,
            Self::Minus => symbol::REMOVE,
            Self::Close => symbol::CLOSE,
            Self::Check => symbol::CHECK,
            Self::Ellipsis => symbol::MORE_HORIZ,
            Self::Play => symbol::PLAY_ARROW,
            Self::Pause => symbol::PAUSE,
            Self::StepBack => symbol::SKIP_PREVIOUS,
            Self::StepForward => symbol::SKIP_NEXT,
            Self::Record => symbol::FIBER_MANUAL_RECORD,
            Self::Eye => symbol::VISIBILITY,
            Self::EyeOff => symbol::VISIBILITY_OFF,
            Self::Folder => symbol::FOLDER,
            Self::Save => symbol::SAVE,
            Self::Copy => symbol::CONTENT_COPY,
            Self::Camera => symbol::PHOTO_CAMERA,
            Self::Globe => symbol::PUBLIC,
            Self::Film => symbol::MOVIE,
            Self::Export => symbol::FILE_UPLOAD,
            Self::Trash => symbol::DELETE,
            Self::Cube => symbol::DEPLOYED_CODE,
            Self::Section => symbol::CONTENT_CUT,
            Self::Fit => symbol::ZOOM_OUT_MAP,
            Self::Reset => symbol::RESTART_ALT,
            Self::Layers => symbol::LAYERS,
            Self::Target => symbol::MY_LOCATION,
            Self::Search => symbol::SEARCH,
            Self::Sliders => symbol::TUNE,
            Self::Table => symbol::TABLE,
            Self::Chart => symbol::BAR_CHART,
            Self::Grid => symbol::GRID_VIEW,
            Self::Sparkles => symbol::AUTO_AWESOME,
            Self::Info => symbol::INFO,
            Self::Warning => symbol::WARNING,
            Self::Clock => symbol::SCHEDULE,
            Self::Lock => symbol::LOCK,
            Self::Channel => symbol::GRAPHIC_EQ,
            Self::Filter => symbol::FILTER_ALT,
            Self::Columns => symbol::VIEW_COLUMN,
            Self::Sidebar => symbol::VIEW_SIDEBAR,
            Self::Undo => symbol::UNDO,
            Self::Redo => symbol::REDO,
            Self::Pencil => symbol::EDIT,
            Self::Ruler => symbol::STRAIGHTEN,
            Self::WindowMinimize => symbol::REMOVE,
            Self::WindowMaximize => symbol::CROP_SQUARE,
            Self::WindowRestore => symbol::FILTER_NONE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconStyle {
    pub weight: f32,
    pub fill: bool,
}

impl IconStyle {
    pub const REGULAR: Self = Self {
        weight: 400.0,
        fill: false,
    };

    pub const FILLED: Self = Self {
        weight: 400.0,
        fill: true,
    };

    pub const fn glyph(self, size: f32) -> GlyphStyle {
        GlyphStyle::new(size, self.weight).with_fill(self.fill)
    }
}

pub fn paint(
    icon: Icon,
    painter: &mut Painter<'_>,
    center: Vec2,
    size: f32,
    color: Color,
    style: IconStyle,
) {
    painter.icon(icon, center, size, 0.0, color, style);
}

pub fn chevron(
    painter: &mut Painter<'_>,
    center: Vec2,
    size: f32,
    angle: f32,
    color: Color,
    style: IconStyle,
) {
    painter.icon(
        Icon::ChevronDown,
        center,
        size,
        angle - std::f32::consts::FRAC_PI_2,
        color,
        style,
    );
}
