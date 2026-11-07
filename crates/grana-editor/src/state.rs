use std::collections::{HashMap, HashSet};

use glam::Vec2;
use grana_animation::Timeline;
use grana_display::Viewport;
use grana_measure::{MeasureView, Measurement};
use grana_scene::{Measure, ObjectSet, ObjectSetId, Progress, Scene};
use grana_session::{BatchView, Export, HistoryView, IoView, SeriesView};
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
}

#[derive(Clone, Copy, Debug)]
pub struct Playback {
    pub playing: bool,
    pub position: f32,
    pub previewing: bool,
    pub selected: Option<usize>,
    pub width: u32,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            playing: false,
            position: 0.0,
            previewing: false,
            selected: None,
            width: 1920,
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

pub struct Workspace {
    pub panel: Option<Panel>,
    pub last_panel: Panel,
    pub sidebar_hidden: bool,
    pub sidebar_tab: usize,
    pub dock_height: f32,
    pub playback: Playback,
    pub export: Export,
    pub column_widths: HashMap<usize, f32>,
    pub track_set: Option<ObjectSetId>,
    pub join: Option<(u32, usize)>,
    pub seed: Option<ObjectSetId>,
    pub annotation_tool: Option<AnnotationTool>,
    pub annotation_pending: Option<[f32; 3]>,
    pub annotation_drag: Option<(usize, grana_figure::AnnotationPart)>,
    pub renaming: Option<ObjectSetId>,
    pub expanded: HashSet<ObjectSetId>,
    pub sections: Sections,
    pub classifier: HashMap<ObjectSetId, Vec<Measurement>>,
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
            export: Export::NATIVE,
            column_widths: HashMap::new(),
            track_set: None,
            join: None,
            seed: None,
            annotation_tool: None,
            annotation_pending: None,
            annotation_drag: None,
            renaming: None,
            expanded: HashSet::new(),
            sections: Sections::default(),
            classifier: HashMap::new(),
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
    pub labels: &'a [String],
    pub viewport: &'a Viewport,
    pub stage: &'a Stage,
    pub workspace: &'a Workspace,
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
}
