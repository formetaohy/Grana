use grana_animation::Timeline;
use grana_scene::{Measure, Progress, Scene};

use crate::command::Action;

pub struct Message(String);

impl Message {
    pub fn error(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn text(&self) -> &str {
        &self.0
    }
}

pub struct BatchDataset {
    pub name: String,
    pub state: String,
    pub detail: String,
}

pub struct BatchView<'a> {
    pub protocol: Option<&'a str>,
    pub datasets: &'a [BatchDataset],
    pub output: Option<String>,
    pub jobs: usize,
    pub running: bool,
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
    pub store: StoreView,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HistoryView {
    pub undo: Option<Action>,
    pub redo: Option<Action>,
}

pub struct SessionView<'a> {
    pub scene: &'a Scene,
    pub measure: &'a Measure,
    pub timeline: &'a Timeline,
    pub jobs: &'a [Progress],
    pub io: IoView<'a>,
    pub batch: Option<BatchView<'a>>,
    pub history: HistoryView,
}
