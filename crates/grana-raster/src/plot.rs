use grana_text::Fonts;

use crate::Raster;
use crate::text::measure;

const BACKGROUND: [u8; 4] = [18, 20, 24, 255];
const FIELD: [u8; 4] = [24, 26, 31, 255];
const EDGE: [u8; 4] = [38, 42, 50, 255];
const GRID: [u8; 4] = [33, 36, 43, 255];
const TITLE: [u8; 4] = [232, 236, 242, 255];
const HEADING: [u8; 4] = [246, 248, 252, 255];
const DETAIL: [u8; 4] = [138, 146, 158, 255];
const MARKER: [u8; 4] = [186, 193, 205, 255];
const COSTES: [u8; 4] = [255, 214, 112, 255];

const MARGIN: u32 = 24;
const GUTTER: u32 = 56;
const HEADER: u32 = 58;
const FOOTER: u32 = 62;
const CLEARANCE: u32 = 8;
const CELLS: usize = 160;
const DIM: f32 = 0.10;

const TITLE_SIZE: f32 = 19.0;
const HEADING_WEIGHT: f32 = 600.0;
const DETAIL_SIZE: f32 = 13.0;
const LABEL_SIZE: f32 = 12.0;
const WEIGHT: f32 = 500.0;

pub struct Axis<'a> {
    name: &'a str,
    tint: [f32; 3],
}

impl<'a> Axis<'a> {
    pub fn new(name: &'a str, tint: [f32; 3]) -> Self {
        assert!(
            tint.iter().all(|value| (0.0..=1.0).contains(value)),
            "a plotted channel carries an srgb tint"
        );
        Self { name, tint }
    }
}

pub struct Joint<'a> {
    pub x: Axis<'a>,
    pub y: Axis<'a>,
    pub bins: usize,
    pub joint: &'a [u64],
    pub threshold: [f32; 2],
    pub costes: Option<[[f32; 2]; 2]>,
    pub title: &'a str,
    pub subtitle: &'a str,
    pub caption: &'a str,
}

pub struct Series<'a> {
    pub name: &'a str,
    pub tint: [f32; 3],
    pub values: &'a [f32],
}

pub struct Line<'a> {
    pub x: Axis<'a>,
    pub y: Axis<'a>,
    pub offsets: &'a [f32],
    pub series: &'a [Series<'a>],
    pub title: &'a str,
    pub subtitle: &'a str,
    pub caption: &'a str,
}

impl Line<'_> {
    fn checked(&self) {
        assert!(
            !self.series.is_empty(),
            "a line plot draws at least one series"
        );
        assert_eq!(
            self.offsets.len(),
            self.series[0].values.len(),
            "a line plot samples every series along the same offsets"
        );
        assert!(
            self.series
                .iter()
                .all(|series| series.values.len() == self.offsets.len()),
            "every line plot series samples the same offsets"
        );
        assert!(
            self.series
                .iter()
                .all(|series| series.tint.iter().all(|value| (0.0..=1.0).contains(value))),
            "a line plot series carries an srgb tint"
        );
    }
}

impl Joint<'_> {
    fn checked(&self) {
        assert!(self.bins > 0, "a joint plot bins its intensities");
        assert_eq!(
            self.joint.len(),
            self.bins * self.bins,
            "a joint plot scatters every intensity bin pair"
        );
    }
}

pub fn joint_plot(fonts: &mut Fonts, plot: &Joint<'_>, size: [u32; 2]) -> Raster {
    plot.checked();
    assert!(
        size[0] > 2 * MARGIN + GUTTER + CLEARANCE && size[1] > 2 * MARGIN + HEADER + FOOTER,
        "a joint plot covers a readable frame"
    );
    let mut raster = Raster::solid(size[0], size[1], BACKGROUND);
    let field = Rect::new(
        MARGIN + GUTTER,
        MARGIN + HEADER - 18,
        size[0] - MARGIN - CLEARANCE,
        size[1] - MARGIN - FOOTER,
    );
    raster.text(
        fonts,
        plot.title,
        [MARGIN, MARGIN - 6],
        TITLE_SIZE,
        HEADING_WEIGHT,
        HEADING,
    );
    let subtitle = measure(fonts, plot.subtitle, DETAIL_SIZE, WEIGHT);
    raster.text(
        fonts,
        plot.subtitle,
        [size[0] - MARGIN - subtitle, MARGIN - 1],
        DETAIL_SIZE,
        WEIGHT,
        DETAIL,
    );
    fill(&mut raster, field, FIELD);
    for index in 1..4 {
        let fraction = index as f32 / 4.0;
        let x = field.left + (field.width() as f32 * fraction).round() as u32;
        let y = field.bottom - (field.height() as f32 * fraction).round() as u32;
        fill(
            &mut raster,
            Rect::new(x, field.top, x + 1, field.bottom),
            GRID,
        );
        fill(
            &mut raster,
            Rect::new(field.left, y, field.right, y + 1),
            GRID,
        );
    }
    scatter(&mut raster, plot, field);
    let vertical = field.left + (plot.threshold[0].clamp(0.0, 1.0) * field.width() as f32) as u32;
    let horizontal =
        field.bottom - (plot.threshold[1].clamp(0.0, 1.0) * field.height() as f32) as u32 - 1;
    fill(
        &mut raster,
        Rect::new(vertical, field.top, vertical + 1, field.bottom),
        MARKER,
    );
    fill(
        &mut raster,
        Rect::new(field.left, horizontal, field.right, horizontal + 1),
        MARKER,
    );
    outline(&mut raster, field, EDGE);
    let bar = field.bottom + 14;
    if let Some([low, high]) = plot.costes {
        let left = field.left + (low[0].clamp(0.0, 1.0) * field.width() as f32) as u32;
        let right = field.left + (high[0].clamp(0.0, 1.0) * field.width() as f32) as u32;
        let right = right.max(left + 2);
        fill(&mut raster, Rect::new(left, bar, right, bar + 3), COSTES);
        raster.text(
            fonts,
            "Costes",
            [right + 8, bar - 4],
            LABEL_SIZE,
            WEIGHT,
            DETAIL,
        );
    }
    let tint = |tint: [f32; 3]| -> [u8; 4] {
        [
            (tint[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (tint[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (tint[2].clamp(0.0, 1.0) * 255.0).round() as u8,
            255,
        ]
    };
    raster.text(
        fonts,
        plot.y.name,
        [MARGIN, field.top - 20],
        LABEL_SIZE,
        WEIGHT,
        tint(plot.y.tint),
    );
    let name = measure(fonts, plot.x.name, LABEL_SIZE, WEIGHT);
    raster.text(
        fonts,
        plot.x.name,
        [field.right - name, field.bottom + 4],
        LABEL_SIZE,
        WEIGHT,
        tint(plot.x.tint),
    );
    raster.text(
        fonts,
        "0",
        [field.left - 12, field.bottom - 10],
        LABEL_SIZE,
        WEIGHT,
        DETAIL,
    );
    raster.text(
        fonts,
        "1",
        [field.left - 12, field.top - 2],
        LABEL_SIZE,
        WEIGHT,
        DETAIL,
    );
    raster.text(
        fonts,
        "0",
        [field.left - 2, field.bottom + 16],
        LABEL_SIZE,
        WEIGHT,
        DETAIL,
    );
    let unit = measure(fonts, "1", LABEL_SIZE, WEIGHT);
    raster.text(
        fonts,
        "1",
        [field.right - 4 - unit, field.bottom + 16],
        LABEL_SIZE,
        WEIGHT,
        DETAIL,
    );
    raster.text(
        fonts,
        plot.caption,
        [MARGIN, field.bottom + 30],
        DETAIL_SIZE,
        WEIGHT,
        TITLE,
    );
    raster
}

pub fn line_plot(fonts: &mut Fonts, plot: &Line<'_>, size: [u32; 2]) -> Raster {
    plot.checked();
    assert!(
        size[0] > 2 * MARGIN + GUTTER + CLEARANCE && size[1] > 2 * MARGIN + HEADER + FOOTER,
        "a line plot covers a readable frame"
    );
    let mut raster = Raster::solid(size[0], size[1], BACKGROUND);
    let field = Rect::new(
        MARGIN + GUTTER,
        MARGIN + HEADER - 18,
        size[0] - MARGIN - CLEARANCE,
        size[1] - MARGIN - FOOTER,
    );
    raster.text(
        fonts,
        plot.title,
        [MARGIN, MARGIN - 6],
        TITLE_SIZE,
        HEADING_WEIGHT,
        HEADING,
    );
    let subtitle = measure(fonts, plot.subtitle, DETAIL_SIZE, WEIGHT);
    raster.text(
        fonts,
        plot.subtitle,
        [size[0] - MARGIN - subtitle, MARGIN - 1],
        DETAIL_SIZE,
        WEIGHT,
        DETAIL,
    );
    fill(&mut raster, field, FIELD);
    for index in 1..4 {
        let fraction = index as f32 / 4.0;
        let x = field.left + (field.width() as f32 * fraction).round() as u32;
        let y = field.bottom - (field.height() as f32 * fraction).round() as u32;
        fill(
            &mut raster,
            Rect::new(x, field.top, x + 1, field.bottom),
            GRID,
        );
        fill(
            &mut raster,
            Rect::new(field.left, y, field.right, y + 1),
            GRID,
        );
    }
    let low = plot.offsets.first().copied().unwrap_or(0.0);
    let high = plot
        .offsets
        .last()
        .copied()
        .unwrap_or(low)
        .max(low + 1.0e-6);
    let ceiling = plot
        .series
        .iter()
        .flat_map(|series| series.values.iter())
        .copied()
        .filter(|value| value.is_finite())
        .fold(0.0f32, f32::max)
        .max(1.0e-6)
        * 1.05;
    for series in plot.series {
        let color = tint(series.tint);
        let mut previous = None;
        for (offset, value) in plot.offsets.iter().zip(series.values) {
            if !value.is_finite() {
                previous = None;
                continue;
            }
            let x =
                field.left + ((offset - low) / (high - low) * field.width() as f32).round() as u32;
            let y = field.bottom
                - ((value / ceiling).clamp(0.0, 1.0) * field.height() as f32).round() as u32;
            let point = (
                x.min(field.right - 1) as i32,
                y.clamp(field.top, field.bottom - 1) as i32,
            );
            if let Some(previous) = previous {
                stroke(&mut raster, previous, point, color);
            } else {
                stroke(&mut raster, point, point, color);
            }
            previous = Some(point);
        }
    }
    outline(&mut raster, field, EDGE);
    raster.text(
        fonts,
        plot.y.name,
        [MARGIN, field.top - 20],
        LABEL_SIZE,
        WEIGHT,
        tint(plot.y.tint),
    );
    let name = measure(fonts, plot.x.name, LABEL_SIZE, WEIGHT);
    raster.text(
        fonts,
        plot.x.name,
        [field.right - name, field.bottom + 4],
        LABEL_SIZE,
        WEIGHT,
        tint(plot.x.tint),
    );
    raster.text(
        fonts,
        "0",
        [field.left - 12, field.bottom - 10],
        LABEL_SIZE,
        WEIGHT,
        DETAIL,
    );
    let top = format!("{ceiling:.2}");
    let top_width = measure(fonts, &top, LABEL_SIZE, WEIGHT);
    raster.text(
        fonts,
        &top,
        [field.left - 12 - top_width, field.top - 2],
        LABEL_SIZE,
        WEIGHT,
        DETAIL,
    );
    let first = format!("{low:.1}");
    raster.text(
        fonts,
        &first,
        [field.left, field.bottom + 16],
        LABEL_SIZE,
        WEIGHT,
        DETAIL,
    );
    let last = format!("{high:.1}");
    let last_width = measure(fonts, &last, LABEL_SIZE, WEIGHT);
    raster.text(
        fonts,
        &last,
        [field.right - last_width, field.bottom + 16],
        LABEL_SIZE,
        WEIGHT,
        DETAIL,
    );
    let mut pen = field.left;
    let mut row = 0u32;
    for series in plot.series {
        let label = measure(fonts, series.name, LABEL_SIZE, WEIGHT);
        if pen + label + 24 > field.right {
            pen = field.left;
            row += 1;
        }
        let y = field.top + 10 + row * 16;
        fill(
            &mut raster,
            Rect::new(pen, y + 2, pen + 14, y + 5),
            tint(series.tint),
        );
        raster.text(
            fonts,
            series.name,
            [pen + 20, y - 3],
            LABEL_SIZE,
            WEIGHT,
            TITLE,
        );
        pen += label + 34;
    }
    raster.text(
        fonts,
        plot.caption,
        [MARGIN, field.bottom + 34],
        DETAIL_SIZE,
        WEIGHT,
        TITLE,
    );
    raster
}

fn tint(tint: [f32; 3]) -> [u8; 4] {
    [
        (tint[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (tint[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (tint[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        255,
    ]
}

fn stroke(raster: &mut Raster, from: (i32, i32), to: (i32, i32), color: [u8; 4]) {
    let (mut x, mut y) = from;
    let (target_x, target_y) = to;
    let dx = (target_x - x).abs();
    let dy = -(target_y - y).abs();
    let step_x = if x < target_x { 1 } else { -1 };
    let step_y = if y < target_y { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        dot(raster, x, y, color);
        if x == target_x && y == target_y {
            break;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x += step_x;
        }
        if doubled <= dx {
            error += dx;
            y += step_y;
        }
    }
}

fn dot(raster: &mut Raster, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 {
        return;
    }
    let width = raster.width() as i32;
    let height = raster.height() as i32;
    if x >= width - 1 || y >= height - 1 {
        return;
    }
    fill(
        raster,
        Rect::new(x as u32, y as u32, x as u32 + 2, y as u32 + 2),
        color,
    );
}

fn scatter(raster: &mut Raster, plot: &Joint<'_>, field: Rect) {
    let cells = plot.bins.min(CELLS);
    let block = plot.bins.div_ceil(cells);
    let maximum = plot.joint.iter().copied().max().unwrap_or(1).max(1);
    let ceiling = (maximum as f32 + 1.0).ln();
    let width = field.width() as f32 / cells as f32;
    let height = field.height() as f32 / cells as f32;
    for row in 0..cells {
        for column in 0..cells {
            let mut count = 0u64;
            for inner_row in 0..block {
                for inner_column in 0..block {
                    let a = column * block + inner_column;
                    let b = row * block + inner_row;
                    if a < plot.bins && b < plot.bins {
                        count = count.max(plot.joint[a + plot.bins * b]);
                    }
                }
            }
            if count == 0 {
                continue;
            }
            let weight = ((count as f32 + 1.0).ln() / ceiling).clamp(0.0, 1.0);
            let blend = (column + row) as f32 / (2.0 * (cells - 1).max(1) as f32);
            let brightness = DIM + (1.0 - DIM) * weight * weight;
            let tint = [0, 1, 2].map(|axis| {
                (plot.x.tint[axis] + (plot.y.tint[axis] - plot.x.tint[axis]) * blend) * brightness
            });
            let left = field.left + (column as f32 * width).round() as u32;
            let right = field.left + ((column + 1) as f32 * width).ceil() as u32;
            let top = field.bottom - ((row + 1) as f32 * height).ceil() as u32;
            let bottom = field.bottom - (row as f32 * height).round() as u32;
            let color = [
                (tint[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                (tint[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                (tint[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                255,
            ];
            fill(
                raster,
                Rect::new(left, top, right.min(field.right), bottom.min(field.bottom)),
                color,
            );
        }
    }
}

#[derive(Clone, Copy)]
struct Rect {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl Rect {
    fn new(left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Self {
            left,
            top,
            right: right.max(left),
            bottom: bottom.max(top),
        }
    }

    fn width(self) -> u32 {
        self.right - self.left
    }

    fn height(self) -> u32 {
        self.bottom - self.top
    }
}

fn fill(raster: &mut Raster, rect: Rect, color: [u8; 4]) {
    let width = raster.width() as usize;
    let height = raster.height();
    let pixels = raster.pixels_mut();
    for y in rect.top..rect.bottom.min(height) {
        let start = (rect.left as usize + width * y as usize) * 4;
        let end = (rect.right as usize + width * y as usize) * 4;
        for pixel in pixels[start..end].as_chunks_mut::<4>().0 {
            *pixel = color;
        }
    }
}

fn outline(raster: &mut Raster, rect: Rect, color: [u8; 4]) {
    fill(
        raster,
        Rect::new(rect.left, rect.top, rect.right, rect.top + 1),
        color,
    );
    fill(
        raster,
        Rect::new(rect.left, rect.bottom - 1, rect.right, rect.bottom),
        color,
    );
    fill(
        raster,
        Rect::new(rect.left, rect.top, rect.left + 1, rect.bottom),
        color,
    );
    fill(
        raster,
        Rect::new(rect.right - 1, rect.top, rect.right, rect.bottom),
        color,
    );
}
