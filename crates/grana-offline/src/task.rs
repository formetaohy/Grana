use std::sync::Arc;

use grana_scene::Scene;
use grana_volume::{BrickData, BrickRequest, NoData};

use crate::plan::Plan;

pub trait Bricks: Send + Sync {
    fn read(&self, request: BrickRequest, warps: &grana_warp::Warps) -> Result<BrickData, String>;

    fn empty(
        &self,
        request: BrickRequest,
        warps: &grana_warp::Warps,
    ) -> Result<Option<NoData>, String>;
}

pub struct Task {
    pub plan: Plan,
    pub scene: Scene,
    pub bricks: Arc<dyn Bricks>,
}
