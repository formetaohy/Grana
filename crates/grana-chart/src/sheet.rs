use glam::Vec2;
use grana_ui::{Corners, Mark, Painter, Rect, Theme};

use crate::chart::Page;
use crate::decor::Decor;
use crate::style::Style;

const GAP: f32 = 24.0;

pub struct Sheet {
    pages: Vec<Page>,
    columns: usize,
    gap: f32,
}

impl Sheet {
    pub fn new(pages: Vec<Page>, columns: usize) -> Self {
        assert!(!pages.is_empty(), "a sheet holds at least one page");
        assert!(columns > 0, "a sheet lays out at least one column");
        let cell = pages[0].size();
        assert!(
            pages.iter().all(|page| page.size() == cell),
            "a sheet lays out pages of one size"
        );
        Self {
            pages,
            columns,
            gap: GAP,
        }
    }

    pub fn size(&self) -> [u32; 2] {
        let cell = self.pages[0].size();
        let columns = self.columns.min(self.pages.len()) as f32;
        let rows = self.pages.len().div_ceil(self.columns) as f32;
        [
            (columns * cell[0] as f32 + (columns - 1.0) * self.gap).round() as u32,
            (rows * cell[1] as f32 + (rows - 1.0) * self.gap).round() as u32,
        ]
    }

    pub fn paint(&self, painter: &mut Painter, rect: Rect, style: &Style, decor: &Decor<'_>) {
        let size = self.size();
        let cell = self.pages[0].size();
        let scale = rect.width() / size[0] as f32;
        let step = Vec2::new(
            (cell[0] as f32 + self.gap) * scale,
            (cell[1] as f32 + self.gap) * scale,
        );
        let panel = Vec2::new(cell[0] as f32 * scale, cell[1] as f32 * scale);
        for (index, page) in self.pages.iter().enumerate() {
            let origin = rect.min
                + Vec2::new(
                    (index % self.columns) as f32 * step.x,
                    (index / self.columns) as f32 * step.y,
                );
            page.paint(painter, Rect::from_min_size(origin, panel), style, decor);
        }
    }

    pub(crate) fn marks(&self) -> Vec<Mark> {
        let mut recorder = grana_ui::Recorder::new(Theme::dark());
        let theme = *recorder.theme();
        let size = self.size();
        let points = Vec2::new(size[0] as f32, size[1] as f32);
        let page = Rect::from_min_size(Vec2::ZERO, points);
        let style = Style::page(&theme, self.distribution());
        recorder.marks(points, |painter| {
            painter.rect(page, Corners::ZERO, theme.palette.scene_backdrop);
            self.paint(painter, page, &style, &Decor::default());
        })
    }

    pub(crate) fn distribution(&self) -> crate::style::DistributionStyle {
        self.pages[0].distribution()
    }
}
