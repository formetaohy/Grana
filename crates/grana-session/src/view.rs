use grana_animation::Timeline;
use grana_image::Series;
use grana_scene::{Measure, Progress, Scene};

use crate::command::Action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Error,
    Info,
}

pub struct Message {
    text: String,
    kind: Kind,
}

impl Message {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            text: message.into(),
            kind: Kind::Error,
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            text: message.into(),
            kind: Kind::Info,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }
}

pub struct BatchDataset {
    pub name: String,
    pub state: String,
    pub detail: String,
}

pub type BatchComparison = grana_pipeline::Comparison;

pub struct BatchView<'a> {
    pub protocol: Option<&'a str>,
    pub datasets: &'a [BatchDataset],
    pub comparison: &'a [BatchComparison],
    pub output: Option<String>,
    pub jobs: usize,
    pub project: bool,
    pub running: bool,
    pub paused: bool,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StoreView {
    pub resident: u64,
    pub spilled: u64,
}

pub struct IoView<'a> {
    pub adapter: &'a str,
    pub loading: Option<&'a str>,
    pub document: Option<(&'a str, usize, usize)>,
    pub message: Option<&'a Message>,
    pub project: Option<String>,
    pub dirty: bool,
    pub pending: usize,
    pub frames: u32,
    pub frame_interval: f32,
    pub export: Option<(u32, u32)>,
    pub export_message: Option<String>,
    pub label: Option<(&'a str, usize, usize)>,
    pub store: StoreView,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HistoryView {
    pub undo: Option<Action>,
    pub redo: Option<Action>,
}

pub struct SeriesView<'a> {
    pub entries: &'a [Series],
    pub selected: usize,
}

pub struct SessionView<'a> {
    pub scene: &'a Scene,
    pub measure: &'a Measure,
    pub timeline: &'a Timeline,
    pub jobs: &'a [Progress],
    pub io: IoView<'a>,
    pub batch: Option<BatchView<'a>>,
    pub history: HistoryView,
    pub series: SeriesView<'a>,
}
