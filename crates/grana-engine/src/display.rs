use grana_display::Camera;
use grana_scene::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Interactive,
    Playback,
    Still,
}

impl Display {
    pub fn awaits(self) -> bool {
        self == Self::Playback
    }

    pub(crate) fn render(self) -> grana_render::Display {
        match self {
            Self::Interactive => grana_render::Display::Interactive,
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
    pub level_cap: usize,
}
