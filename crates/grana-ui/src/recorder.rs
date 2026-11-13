use glam::Vec2;

use crate::geometry::{Corners, Rect};
use crate::image::ImageAtlas;
use crate::paint::{Clip, Instances, Painter};
use crate::record::Mark;
use crate::text::Text;
use crate::theme::Theme;

pub struct Recorder {
    text: Text,
    images: ImageAtlas,
    theme: Theme,
    instances: Instances,
    marks: Vec<Mark>,
}

impl Recorder {
    pub fn new(theme: Theme) -> Self {
        Self {
            text: Text::detached(),
            images: ImageAtlas::detached(),
            theme,
            instances: Instances::default(),
            marks: Vec::new(),
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn marks(&mut self, size: Vec2, content: impl FnOnce(&mut Painter)) -> Vec<Mark> {
        assert!(
            size.x > 0.0 && size.y > 0.0,
            "a recorded surface covers at least one point"
        );
        self.text.begin(1.0);
        self.instances = Instances::default();
        self.marks.clear();
        {
            let clip = Clip::new(Rect::from_min_size(Vec2::ZERO, size), Corners::ZERO);
            let mut painter = Painter::new(
                &mut self.instances,
                &mut self.text,
                &mut self.images,
                clip,
                Some(&mut self.marks),
            );
            content(&mut painter);
        }
        std::mem::take(&mut self.marks)
    }
}
