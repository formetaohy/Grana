use std::sync::Arc;

use grana_scene::Scene;
use grana_volume::{BrickData, BrickRequest};

use crate::plan::Plan;

pub trait Bricks: Send + Sync {
    fn read(&self, request: BrickRequest, warp: &grana_warp::Warp) -> Result<BrickData, String>;
}

pub struct Task {
    pub plan: Plan,
    pub scene: Scene,
    pub bricks: Arc<dyn Bricks>,
}
