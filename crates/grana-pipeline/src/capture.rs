use std::path::Path;
use std::sync::Arc;

use grana_project::ViewState;
use grana_scene::Scene;

use crate::channels::Channels;

pub enum Kind {
    Still { time: u32 },
    Movie,
    Sequence,
}

pub struct CaptureRequest {
    pub view: ViewState,
    pub size: [u32; 2],
    pub transparent: bool,
    pub kind: Kind,
}

pub trait Capture: Send + Sync {
    fn open(&self) -> Result<(), String>;

    fn capture(
        &self,
        channels: &Channels,
        scene: &Scene,
        request: &CaptureRequest,
        path: &Path,
    ) -> Result<(), String>;
}

pub type CaptureFactory = Arc<dyn Fn() -> Arc<dyn Capture> + Send + Sync>;
