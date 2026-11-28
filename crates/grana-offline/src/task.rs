use std::sync::Arc;
use std::sync::atomic::AtomicBool;

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

pub trait Surfaces: Send + Sync {
    fn frame(
        &self,
        scene: &Scene,
        time: u32,
        cancel: &AtomicBool,
    ) -> Result<Vec<grana_engine::Surface>, String>;

    fn prefetch(&self, _: &Scene, _: u32) {}
}

pub struct Task {
    pub plan: Plan,
    pub scene: Scene,
    pub bricks: Arc<dyn Bricks>,
    pub surfaces: Arc<dyn Surfaces>,
}

pub struct NoSurfaces;

impl Surfaces for NoSurfaces {
    fn frame(
        &self,
        _: &Scene,
        _: u32,
        _: &AtomicBool,
    ) -> Result<Vec<grana_engine::Surface>, String> {
        Ok(Vec::new())
    }
}
