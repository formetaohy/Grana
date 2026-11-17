use std::time::Duration;

use grana_display::Camera;
use grana_render::Sampling;
use grana_scene::Scene;

const STILL_FLOOR: u32 = 8;
pub const SAMPLE_CEILING: u32 = 64;
const STILL_ERROR: f32 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Playback,
    Still,
}

impl Display {
    pub fn awaits(self) -> bool {
        self == Self::Playback
    }

    pub(crate) fn sampling(self) -> Sampling {
        match self {
            Self::Still => Sampling::new(STILL_FLOOR, SAMPLE_CEILING, STILL_ERROR),
            Self::Playback => Sampling::single(),
        }
    }

    pub(crate) fn render(self) -> grana_render::Display {
        match self {
            Self::Playback => grana_render::Display::Playback,
            Self::Still => grana_render::Display::Still,
        }
    }
}

#[derive(Clone, Copy)]
pub struct View<'a> {
    pub scene: &'a Scene,
    pub camera: &'a Camera,
    pub size: [u32; 2],
    pub display: Display,
    pub budget: Option<Duration>,
    pub level_cap: usize,
    pub overlays: &'a [grana_render::Overlay<'a>],
}
