use std::path::Path;
use std::sync::Arc;

use grana_image::Image;
use grana_project::ViewState;
use grana_scene::Scene;

pub struct SnapshotRequest {
    pub view: Option<ViewState>,
    pub size: [u32; 2],
    pub transparent: bool,
    pub time: u32,
}

pub trait Snapshotter: Send + Sync {
    fn open(&self) -> Result<(), String>;

    fn snapshot(
        &self,
        reader: &Image,
        scene: &Scene,
        request: &SnapshotRequest,
        path: &Path,
    ) -> Result<(), String>;
}

pub type SnapshotFactory = Arc<dyn Fn() -> Arc<dyn Snapshotter> + Send + Sync>;
