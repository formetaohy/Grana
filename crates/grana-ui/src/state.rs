use grana_render::FrameInfo;
use grana_scene::{Measure, Progress, Scene};

use crate::panel::Panel;
use crate::viewport::Viewport;

pub struct Message(String);

impl Message {
    pub fn error(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn text(&self) -> &str {
        &self.0
    }
}

pub struct State<'a> {
    pub scene: &'a mut Scene,
    pub measure: &'a mut Measure,
    pub panel: &'a mut Option<Panel>,
    pub viewport: &'a mut Viewport,
    pub adapter: &'a str,
    pub loading: Option<&'a str>,
    pub message: Option<&'a Message>,
    pub project: Option<&'a str>,
    pub dirty: bool,
    pub frame: FrameInfo,
    pub pending: usize,
    pub playing: bool,
    pub analysis: &'a [Progress],
}
