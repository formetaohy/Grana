use std::collections::{HashMap, HashSet};

use glam::Vec2;
use grana_animation::Timeline;
use grana_display::Viewport;
use grana_measure::MeasureView;
use grana_scene::{Measure, ObjectSetId, Progress, Scene};
use grana_session::{BatchView, HistoryView, IoView, SeriesView};
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
    pub msaa: u32,
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
pub struct Interface {
    values: HashMap<Key, f32>,
    open: HashSet<Key>,
}

impl Interface {
    pub fn value(&self, key: Key, fallback: f32) -> f32 {
        self.values.get(&key).copied().unwrap_or(fallback)
    }

    pub fn set(&mut self, key: Key, value: f32) {
        self.values.insert(key, value);
    }

    pub fn open(&self, key: Key) -> bool {
        self.open.contains(&key)
    }

    pub fn set_open(&mut self, key: Key, open: bool) {
        if open {
            self.open.insert(key);
        } else {
            self.open.remove(&key);
        }
    }

    pub fn toggle(&mut self, key: Key) {
        if !self.open.remove(&key) {
            self.open.insert(key);
        }
    }
}

pub struct Workspace {
    pub panel: Option<Panel>,
    pub sidebar_hidden: bool,
    pub sidebar_tab: usize,
    pub dock_height: f32,
    pub dock_last: usize,
    pub playback: Playback,
    pub track_set: Option<ObjectSetId>,
    pub join: Option<(u32, usize)>,
    pub seed: Option<ObjectSetId>,
    pub renaming: Option<ObjectSetId>,
    pub expanded: HashSet<ObjectSetId>,
    pub interface: Interface,
    pub(crate) drag: PlotDrag,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            panel: None,
            sidebar_hidden: false,
            sidebar_tab: 0,
            dock_height: Self::default_height(),
            dock_last: 0,
            playback: Playback::default(),
            track_set: None,
            join: None,
            seed: None,
            renaming: None,
            expanded: HashSet::new(),
            interface: Interface::default(),
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
    pub viewport: &'a Viewport,
    pub stage: &'a Stage,
    pub workspace: &'a Workspace,
    pub frame: FrameStats,
    pub physics: bool,
    pub reduce_motion: bool,
    pub glass_on_glass: bool,
    pub light_angle: f32,
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

    pub fn batch_message(&self) -> Option<&str> {
        self.batch.and_then(|batch| batch.message.as_deref())
    }
}
