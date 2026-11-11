use grana_ui::{Border, Color, Corners, Fill, Painter, Rect, Shadow, Vec2};

const CANVAS: f32 = 512.0;
const RADIUS: f32 = 112.0;
const BORDER: f32 = 2.0;
const BAR_HEIGHT: f32 = 36.8;
const BAR_RADIUS: f32 = 18.4;
const BARS: [(f32, f32, f32); 5] = [
    (145.6, 131.8, 138.0),
    (145.6, 184.7, 158.7),
    (145.6, 237.6, 179.4),
    (145.6, 290.5, 200.1),
    (145.6, 343.4, 220.8),
];
const INK: Color = Color::rgb(0.043137, 0.105882, 0.074510);
const PHOTON: Color = Color::rgb(0.839216, 0.909804, 0.360784);
const PAPER: Color = Color::rgb(0.945098, 0.952941, 0.925490);
const INK_RGB: [f32; 3] = [0.043137, 0.105882, 0.074510];
const PHOTON_RGB: [f32; 3] = [0.839216, 0.909804, 0.360784];
const PAPER_RGB: [f32; 3] = [0.945098, 0.952941, 0.925490];
const EDGE_ALPHA: f32 = 0.22;
const SAMPLES: u32 = 4;

pub fn paint(painter: &mut Painter<'_>, area: Rect) {
    let size = area.width().min(area.height());
    let origin = area.min + (area.size() - Vec2::splat(size)) * 0.5;
    let scale = size / CANVAS;
    painter.outlined(
        Rect::from_min_size(origin, Vec2::splat(size)),
        Corners::all(RADIUS * scale),
        Fill::Solid(INK),
        Border {
            width: BORDER * scale,
            color: PAPER.with_alpha(EDGE_ALPHA),
        },
        Shadow::NONE,
    );
    for (left, top, width) in BARS {
        painter.rect(
            Rect::from_min_size(
                origin + Vec2::new(left * scale, top * scale),
                Vec2::new(width * scale, BAR_HEIGHT * scale),
            ),
            Corners::all(BAR_RADIUS * scale),
            PHOTON,
        );
    }
}

pub fn icon(size: u32) -> Vec<u8> {
    assert!(size > 0, "the brand icon covers at least one pixel");
    let stride = size as usize * 4;
    let mut pixels = vec![0u8; stride * size as usize];
    for (row, line) in pixels.chunks_exact_mut(stride).enumerate() {
        for (column, pixel) in line.chunks_mut(4).enumerate() {
            let mut sum = [0.0f32; 3];
            let mut covered = 0u32;
            for sub_row in 0..SAMPLES {
                for sub_column in 0..SAMPLES {
                    let x = (column as f32 + (sub_column as f32 + 0.5) / SAMPLES as f32)
                        / size as f32
                        * CANVAS;
                    let y = (row as f32 + (sub_row as f32 + 0.5) / SAMPLES as f32) / size as f32
                        * CANVAS;
                    if let Some(color) = shade(x, y) {
                        sum[0] += color[0];
                        sum[1] += color[1];
                        sum[2] += color[2];
                        covered += 1;
                    }
                }
            }
            if covered > 0 {
                let coverage = covered as f32 / (SAMPLES * SAMPLES) as f32;
                pixel[0] = channel(sum[0] / covered as f32);
                pixel[1] = channel(sum[1] / covered as f32);
                pixel[2] = channel(sum[2] / covered as f32);
                pixel[3] = channel(coverage);
            }
        }
    }
    pixels
}

fn shade(x: f32, y: f32) -> Option<[f32; 3]> {
    if BARS
        .iter()
        .any(|&(left, top, width)| inside_round(x, y, left, top, width, BAR_HEIGHT, BAR_RADIUS))
    {
        return Some(PHOTON_RGB);
    }
    if !inside_round(x, y, 0.0, 0.0, CANVAS, CANVAS, RADIUS) {
        return None;
    }
    if inside_round(
        x,
        y,
        BORDER,
        BORDER,
        CANVAS - 2.0 * BORDER,
        CANVAS - 2.0 * BORDER,
        RADIUS - BORDER,
    ) {
        return Some(INK_RGB);
    }
    Some([
        INK_RGB[0] * (1.0 - EDGE_ALPHA) + PAPER_RGB[0] * EDGE_ALPHA,
        INK_RGB[1] * (1.0 - EDGE_ALPHA) + PAPER_RGB[1] * EDGE_ALPHA,
        INK_RGB[2] * (1.0 - EDGE_ALPHA) + PAPER_RGB[2] * EDGE_ALPHA,
    ])
}

fn inside_round(x: f32, y: f32, left: f32, top: f32, width: f32, height: f32, radius: f32) -> bool {
    let radius = radius.min(0.5 * width).min(0.5 * height);
    let across = (x - (left + 0.5 * width)).abs() - (0.5 * width - radius);
    let down = (y - (top + 0.5 * height)).abs() - (0.5 * height - radius);
    let across = across.max(0.0);
    let down = down.max(0.0);
    across * across + down * down <= radius * radius
}

fn channel(value: f32) -> u8 {
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}
