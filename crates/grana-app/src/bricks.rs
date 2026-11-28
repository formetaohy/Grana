use std::sync::Arc;

use grana_offline::Bricks;
use grana_pipeline::Channels;
use grana_volume::{BrickData, BrickRequest, ChannelSet, NoData};

pub struct ImageBricks {
    channels: Arc<Channels>,
}

impl ImageBricks {
    pub fn new(channels: Arc<Channels>) -> Self {
        Self { channels }
    }
}

impl Bricks for ImageBricks {
    fn read(&self, request: BrickRequest, warps: &grana_warp::Warps) -> Result<BrickData, String> {
        self.channels
            .read_brick(
                request.time,
                request.level,
                request.coord,
                warps,
                ChannelSet::ALL,
            )
            .map_err(|error| error.to_string())
    }

    fn empty(
        &self,
        request: BrickRequest,
        warps: &grana_warp::Warps,
    ) -> Result<Option<NoData>, String> {
        self.channels
            .empty(request.time, request.level, request.coord, warps)
            .map_err(|error| error.to_string())
    }
}
