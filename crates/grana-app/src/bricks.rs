use std::sync::Arc;

use grana_offline::Bricks;
use grana_pipeline::Channels;
use grana_volume::{BrickData, BrickRequest};

pub struct ImageBricks {
    channels: Arc<Channels>,
}

impl ImageBricks {
    pub fn new(channels: Channels) -> Self {
        Self {
            channels: Arc::new(channels),
        }
    }
}

impl Bricks for ImageBricks {
    fn read(&self, request: BrickRequest, warp: &grana_warp::Warp) -> Result<BrickData, String> {
        self.channels
            .read_brick(request.time, request.level, request.coord, warp)
            .map_err(|error| error.to_string())
    }
}
