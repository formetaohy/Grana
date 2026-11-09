use std::collections::{HashMap, HashSet};

use glam::Vec2;
use grana_animation::Timeline;
use grana_display::Viewport;
use grana_measure::{MeasureView, Measurement};
use grana_scene::{Measure, ObjectSet, ObjectSetId, Progress, Scene};
use grana_session::{BatchView, Export, HistoryView, IoView, PlateView, SeriesView};
use grana_ui::Key;

use crate::panel::Panel;
use crate::stage::Stage;

#[derive(Clone, Copy, Debug, Default)]
pub struct ViewStats {
    pub level: usize,
    pub visible: usize,
    pub demand: usize,
    pub resident: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub volume: ViewStats,
    pub section: ViewStats,
    pub bytes: u64,
    pub geometry: u64,
    pub fragments: u64,
    pub indices: u64,
    pub samples: u32,
    pub oit_layers: u32,
    pub oit_dropped: u32,
    pub profile: ProfileStats,
}

pub const PROFILE_ROWS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct ProfileStats {
    pub rows: [(&'static str, f32); PROFILE_ROWS],
    pub total: f32,
    pub target: f32,
    pub penalty: f32,
    pub measured: bool,
}

impl Default for ProfileStats {
    fn default() -> Self {
        Self {
            rows: [("", 0.0); PROFILE_ROWS],
            total: 0.0,
            target: 0.0,
            penalty: 1.0,
            measured: false,
        }
    }
}

impl ProfileStats {
    pub fn over_budget(self) -> bool {
        self.measured && self.total > self.target
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Playback {
    pub playing: bool,
    pub position: f32,
    pub previewing: bool,
    pub selected: Option<usize>,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            playing: false,
            position: 0.0,
            previewing: false,
            selected: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlotGate {
    Lasso,
    Rectangle,
}

#[derive(Default)]
pub(crate) struct PlotDrag {
    pub view: Option<MeasureView>,
    pub gate: Option<PlotGate>,
    pub origin: Option<Vec2>,
    pub moved: bool,
    pub vertices: Vec<Vec2>,
}

#[derive(Default)]
pub struct Sections {
    open: HashSet<Key>,
}

impl Sections {
    pub fn is_open(&self, key: Key) -> bool {
        self.open.contains(&key)
    }

    pub fn toggle(&mut self, key: Key) {
        if !self.open.remove(&key) {
            self.open.insert(key);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationTool {
    Point,
    Distance,
    Roi,
}

impl AnnotationTool {
    pub const ALL: [Self; 3] = [Self::Point, Self::Distance, Self::Roi];

    pub fn name(self) -> &'static str {
        match self {
            Self::Point => "Point",
            Self::Distance => "Distance",
            Self::Roi => "Region",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Point => 0,
            Self::Distance => 1,
            Self::Roi => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskBrushTool {
    Brush,
    Erase,
    Wand,
}

impl MaskBrushTool {
    pub const ALL: [Self; 3] = [Self::Brush, Self::Erase, Self::Wand];

    pub fn name(self) -> &'static str {
        match self {
            Self::Brush => "Brush",
            Self::Erase => "Erase",
            Self::Wand => "Wand",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Brush => 0,
            Self::Erase => 1,
            Self::Wand => 2,
        }
    }

    pub fn erase(self) -> bool {
        self == Self::Erase
    }

    pub fn wand(self) -> bool {
        self == Self::Wand
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MaskBrush {
    pub set: ObjectSetId,
    pub tool: MaskBrushTool,
    pub radius: f32,
    pub tolerance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationMetric {
    Contact,
    Distance,
    Overlap,
    Center,
}

pub use grana_session::BatchMetric as ComparisonMetric;

impl RelationMetric {
    pub const ALL: [Self; 4] = [Self::Contact, Self::Distance, Self::Overlap, Self::Center];

    pub fn name(self) -> &'static str {
        match self {
            Self::Contact => "Contact",
            Self::Distance => "Distance",
            Self::Overlap => "Overlap",
            Self::Center => "Centre",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Contact => 0,
            Self::Distance => 1,
            Self::Overlap => 2,
            Self::Center => 3,
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Self::Contact => "碌m虏",
            Self::Distance | Self::Center => "碌m",
            Self::Overlap => "%",
        }
    }

    pub fn decimals(self) -> usize {
        match self {
            Self::Contact | Self::Distance | Self::Center => 3,
            Self::Overlap => 1,
        }
    }

    pub fn format(self, value: f32) -> String {
        format!("{value:.*} {}", self.decimals(), self.unit())
    }

    pub fn value(self, pair: grana_object::RelationPair) -> f32 {
        match self {
            Self::Contact => pair.contact(),
            Self::Distance => pair.distance(),
            Self::Overlap => 100.0 * pair.inside(),
            Self::Center => pair.center(),
        }
    }

    pub fn near(self) -> bool {
        self == Self::Distance
    }
}

pub struct Workspace {
    pub panel: Option<Panel>,
    pub last_panel: Panel,
    pub sidebar_hidden: bool,
    pub sidebar_tab: usize,
    pub dock_height: f32,
    pub playback: Playback,
    pub column_widths: HashMap<usize, f32>,
    pub track_set: Option<ObjectSetId>,
    pub join: Option<(u32, usize)>,
    pub seed: Option<ObjectSetId>,
    pub mask: Option<MaskBrush>,
    pub annotation_tool: Option<AnnotationTool>,
    pub annotation_pending: Option<[f32; 3]>,
    pub annotation_drag: Option<(usize, grana_figure::AnnotationPart)>,
    pub renaming: Option<ObjectSetId>,
    pub remote_open: bool,
    pub expanded: HashSet<ObjectSetId>,
    pub sections: Sections,
    pub psf_threshold: f32,
    pub psf_separation: f32,
    pub classifier: HashMap<ObjectSetId, Vec<Measurement>>,
    pub relation: Option<(ObjectSetId, ObjectSetId, i32)>,
    pub relation_metric: RelationMetric,
    pub relation_filter: f32,
    pub comparison_metric: ComparisonMetric,
    pub comparison_group: Option<String>,
    pub comparison_key: Option<String>,
    pub comparison_open: Option<(String, String)>,
    pub(crate) drag: PlotDrag,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            panel: None,
            last_panel: Panel::Statistics,
            sidebar_hidden: false,
            sidebar_tab: 0,
            dock_height: Self::default_height(),
            playback: Playback::default(),
            column_widths: HashMap::new(),
            track_set: None,
            join: None,
            seed: None,
            mask: None,
            annotation_tool: None,
            annotation_pending: None,
            annotation_drag: None,
            renaming: None,
            remote_open: false,
            expanded: HashSet::new(),
            sections: Sections::default(),
            psf_threshold: 8.0,
            psf_separation: 1.5,
            classifier: HashMap::new(),
            relation: None,
            relation_metric: RelationMetric::Contact,
            relation_filter: 0.0,
            comparison_metric: ComparisonMetric::default(),
            comparison_group: None,
            comparison_key: None,
            comparison_open: None,
            drag: PlotDrag::default(),
        }
    }
}

impl Workspace {
    pub fn default_height() -> f32 {
        340.0
    }

    pub fn expanded(&self, set: ObjectSetId) -> bool {
        self.expanded.contains(&set)
    }

    pub fn classifier_features(&self, set: &ObjectSet) -> Vec<Measurement> {
        self.classifier
            .get(&set.id())
            .cloned()
            .unwrap_or_else(|| set.colorable_measurements())
    }
}

pub struct View<'a> {
    pub scene: &'a Scene,
    pub measure: &'a Measure,
    pub timeline: &'a Timeline,
    pub jobs: &'a [Progress],
    pub io: &'a IoView<'a>,
    pub batch: Option<&'a BatchView<'a>>,
    pub history: HistoryView,
    pub series: &'a SeriesView<'a>,
    pub plate: Option<PlateView<'a>>,
    pub labels: &'a [String],
    pub views: &'a [grana_session::NamedView],
    pub viewport: &'a Viewport,
    pub stage: &'a Stage,
    pub workspace: &'a Workspace,
    pub export: Export,
    pub frame: FrameStats,
}

impl View<'_> {
    pub fn batch_protocol(&self) -> Option<&str> {
        self.batch.and_then(|batch| batch.protocol)
    }

    pub fn batch_datasets(&self) -> &[grana_session::BatchDataset] {
        self.batch.map_or(&[], |batch| batch.datasets)
    }

    pub fn batch_comparison(&self) -> &[grana_session::BatchComparison] {
        self.batch.map_or(&[], |batch| batch.comparison)
    }

    pub fn batch_significance(&self) -> &[grana_session::BatchSignificance] {
        self.batch.map_or(&[], |batch| batch.significance)
    }

    pub fn batch_output(&self) -> Option<&str> {
        self.batch.and_then(|batch| batch.output.as_deref())
    }

    pub fn batch_running(&self) -> bool {
        self.batch.is_some_and(|batch| batch.running)
    }

    pub fn batch_paused(&self) -> bool {
        self.batch.is_some_and(|batch| batch.paused)
    }

    pub fn batch_jobs(&self) -> usize {
        self.batch.map_or(1, |batch| batch.jobs)
    }

    pub fn batch_project(&self) -> bool {
        self.batch.is_some_and(|batch| batch.project)
    }

    pub fn batch_channels(&self) -> bool {
        self.batch.is_some_and(|batch| batch.channels)
    }

    pub fn batch_message(&self) -> Option<&str> {
        self.batch.and_then(|batch| batch.message.as_deref())
    }

    pub fn batch_review(&self) -> Option<usize> {
        self.batch.and_then(|batch| batch.review)
    }

    pub fn batch_review_ready(&self) -> bool {
        self.batch.is_some_and(|batch| batch.review_ready)
    }
}
