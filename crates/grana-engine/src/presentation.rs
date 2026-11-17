use std::time::Duration;

use grana_display::Camera;
use grana_render::Sampling;
use grana_scene::Scene;

const CONVERGE_FLOOR: u32 = 8;
pub const SAMPLE_CEILING: u32 = 64;
const CONVERGE_ERROR: f32 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    Playback,
    Converge,
}

impl Presentation {
    pub fn awaits(self) -> bool {
        self == Self::Playback
    }

    pub(crate) fn sampling(self) -> Sampling {
        match self {
            Self::Converge => Sampling::new(CONVERGE_FLOOR, SAMPLE_CEILING, CONVERGE_ERROR),
            Self::Playback => Sampling::single(),
        }
    }

    pub(crate) fn delivery(self) -> grana_render::Delivery {
        match self {
            Self::Playback => grana_render::Delivery::Awaited,
            Self::Converge => grana_render::Delivery::Immediate,
        }
    }
}

#[derive(Clone, Copy)]
pub struct View<'a> {
    pub scene: &'a Scene,
    pub camera: &'a Camera,
    pub size: [u32; 2],
    pub presentation: Presentation,
    pub budget: Option<Duration>,
    pub level_cap: usize,
    pub overlays: &'a [grana_render::Overlay<'a>],
}
