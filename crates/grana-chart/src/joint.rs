use glam::Vec2;
use grana_measure::{ColocCell, ColocSeries};
use grana_ui::{Corners, Mark, Painter, Rect, Theme};

use crate::axis::Axis;
use crate::chart::{HEIGHT, Heading, WIDTH};
use crate::distribution::empty_style;
use crate::grid;
use crate::style::Style;

const CELLS: usize = 160;

const DIM: f32 = 0.22;

const INSET: f32 = 0.4;

const COSTES: f32 = 2.0;

const MARKER: f32 = 0.8;

pub struct Joint {
    pub heading: Heading,
    bins: usize,
    cells: Vec<ColocCell>,
    tints: [[f32; 3]; 2],
    thresholds: [f32; 2],
    costes: Option<[[f32; 2]; 2]>,
}

impl Joint {
    pub fn of(
        heading: Heading,
        series: &ColocSeries,
        time: u32,
        tints: [[f32; 3]; 2],
        thresholds: [f32; 2],
    ) -> Self {
        let cells = series
            .joint()
            .frame(time)
            .expect("a colocalization frame covers the scene timeline")
            .to_vec();
        Self::new(
            heading,
            series.bins(),
            cells,
            tints,
            thresholds,
            costes(series),
        )
    }

    pub fn total(
        heading: Heading,
        series: &ColocSeries,
        tints: [[f32; 3]; 2],
        thresholds: [f32; 2],
    ) -> Self {
        Self::new(
            heading,
            series.bins(),
            series.joint().cells(),
            tints,
            thresholds,
            costes(series),
        )
    }

    pub fn new(
        heading: Heading,
        bins: usize,
        cells: Vec<ColocCell>,
        tints: [[f32; 3]; 2],
        thresholds: [f32; 2],
        costes: Option<[[f32; 2]; 2]>,
    ) -> Self {
        assert!(bins > 0, "a joint histogram bins its intensities");
        assert!(
            cells
                .iter()
                .all(|cell| (cell.cell() as usize) < bins * bins),
            "a joint histogram scatters every intensity bin pair"
        );
        assert!(
            tints
                .iter()
                .flatten()
                .all(|value| (0.0..=1.0).contains(value)),
            "a joint histogram tints its channels within the unit cube"
        );
        assert!(
            thresholds.iter().all(|value| (0.0..=1.0).contains(value)),
            "a joint histogram cuts its intensities within the unit interval"
        );
        assert!(
            costes
                .iter()
                .flatten()
                .flatten()
                .all(|value| (0.0..=1.0).contains(value)),
            "a costes range cuts its intensities within the unit interval"
        );
        Self {
            heading,
            bins,
            cells,
            tints,
            thresholds,
            costes,
        }
    }

    pub fn crossings(&self) -> usize {
        self.cells.len()
    }

    pub fn size(&self) -> [u32; 2] {
        [WIDTH, HEIGHT]
    }

    pub(crate) fn marks(&self) -> Vec<Mark> {
        let mut recorder = grana_ui::Recorder::new(Theme::dark());
        let theme = *recorder.theme();
        let size = self.size();
        let points = Vec2::new(size[0] as f32, size[1] as f32);
        let page = Rect::from_min_size(Vec2::ZERO, points);
        let style = Style::page(&theme, crate::style::DistributionStyle::Box);
        recorder.marks(points, |painter| {
            painter.rect(page, Corners::ZERO, theme.palette.surface_dim);
            self.paint(painter, page, &style, None);
        })
    }

    pub fn paint(&self, painter: &mut Painter, rect: Rect, style: &Style, empty: Option<&str>) {
        let heading = &self.heading;
        let style = Style {
            language: heading.language,
            ..*style
        };
        crate::chart::heading(painter, rect, heading, &style);
        let metrics = style.metrics;
        let palette = style.palette;
        let frame = metrics.frame(rect);
        let x = Axis::new(0.0, 1.0, 4);
        let y = Axis::new(0.0, 1.0, 4);
        grid::grid(painter, rect, &style, x, y);
        self.paint_cells(painter, frame);
        let marker = palette.on_surface.with_alpha(0.55);
        let column = x.x(frame, self.thresholds[0]);
        painter.segment(
            Vec2::new(column, frame.min.y),
            Vec2::new(column, frame.max.y),
            MARKER,
            marker,
        );
        let row = y.y(frame, self.thresholds[1]);
        painter.segment(
            Vec2::new(frame.min.x, row),
            Vec2::new(frame.max.x, row),
            MARKER,
            marker,
        );
        if let Some([[low_a, low_b], [high_a, high_b]]) = self.costes {
            let warning = palette.warning.with_alpha(0.85);
            let left = x.x(frame, low_a);
            let right = x.x(frame, high_a);
            painter.segment(
                Vec2::new(left, frame.max.y - COSTES),
                Vec2::new(right.max(left + COSTES), frame.max.y - COSTES),
                COSTES,
                warning,
            );
            let bottom = y.y(frame, low_b);
            let top = y.y(frame, high_b);
            painter.segment(
                Vec2::new(frame.min.x + COSTES, bottom.min(top)),
                Vec2::new(frame.min.x + COSTES, bottom.max(top) - COSTES),
                COSTES,
                warning,
            );
        }
        if self.crossings() == 0
            && let Some(empty) = empty
        {
            painter.text_in(frame, empty, empty_style(&style), grana_ui::Align::Center);
        }
    }

    fn paint_cells(&self, painter: &mut Painter, frame: Rect) {
        let cells = self.bins.clamp(1, CELLS);
        let block = self.bins.div_ceil(cells);
        let mut grid = vec![0u64; cells * cells];
        for cell in &self.cells {
            let index = cell.cell() as usize;
            let column = (index % self.bins) / block;
            let row = (index / self.bins) / block;
            let slot = column + cells * row;
            grid[slot] = grid[slot].max(cell.count());
        }
        let maximum = grid.iter().copied().max().unwrap_or(1).max(1);
        let ceiling = (maximum as f32 + 1.0).ln();
        let width = frame.width() / cells as f32;
        let height = frame.height() / cells as f32;
        for row in 0..cells {
            for column in 0..cells {
                let count = grid[column + cells * row];
                if count == 0 {
                    continue;
                }
                let weight = ((count as f32 + 1.0).ln() / ceiling).clamp(0.0, 1.0);
                let blend = (column + row) as f32 / (2.0 * (cells - 1).max(1) as f32);
                let brightness = DIM + (1.0 - DIM) * weight;
                let tint = [0, 1, 2].map(|axis| {
                    (self.tints[0][axis] + (self.tints[1][axis] - self.tints[0][axis]) * blend)
                        * brightness
                });
                let left = frame.min.x + column as f32 * width;
                let top = frame.max.y - (row + 1) as f32 * height;
                painter.rect(
                    Rect::from_min_size(
                        Vec2::new(left, top),
                        Vec2::new(width + INSET, height + INSET),
                    ),
                    Corners::ZERO,
                    grana_ui::Color::rgba(tint[0], tint[1], tint[2], 1.0),
                );
            }
        }
    }
}

fn costes(series: &ColocSeries) -> Option<[[f32; 2]; 2]> {
    series.costes().map(|range| {
        let low = range.low();
        let high = range.high();
        [[low.a(), low.b()], [high.a(), high.b()]]
    })
}
