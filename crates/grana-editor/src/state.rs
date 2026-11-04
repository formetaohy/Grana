use glam::Vec2;
use grana_animation::Timeline;
use grana_display::Viewport;
use grana_measure::MeasureView;
use grana_scene::{Measure, ObjectSetId, Progress, Scene};
use grana_session::{BatchView, HistoryView, IoView, SeriesView};

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
pub struct Workspace {
    pub panel: Option<Panel>,
    pub playback: Playback,
    pub movie_size: [u32; 2],
    pub track_set: Option<ObjectSetId>,
    pub join: Option<(u32, usize)>,
    pub seed: Option<ObjectSetId>,
    pub(crate) drag: PlotDrag,
}

pub struct State<'a> {
    pub scene: &'a Scene,
    pub measure: &'a Measure,
    pub timeline: &'a Timeline,
    pub jobs: &'a [Progress],
    pub io: &'a IoView<'a>,
    pub batch: Option<&'a BatchView<'a>>,
    pub history: HistoryView,
    pub series: &'a SeriesView<'a>,
    pub viewport: &'a mut Viewport,
    pub stage: &'a mut Stage,
    pub workspace: &'a mut Workspace,
    pub frame: FrameStats,
}

impl State<'_> {
    pub fn batch_protocol(&self) -> Option<&str> {
        self.batch.and_then(|batch| batch.protocol)
    }

    pub fn batch_datasets(&self) -> &[grana_session::BatchDataset] {
        self.batch.map_or(&[], |batch| batch.datasets)
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
