use grana_ui::{Border, Color, Corners, Fill, Painter, Rect, Shadow, Vec2};

const CANVAS: f32 = 512.0;
const RADIUS: f32 = 124.0;
const BORDER: f32 = 2.5;
const GAP: f32 = 16.0;
const SLAB_WIDTH: f32 = 228.0;
const SLAB_HEIGHT: f32 = 164.0;
const INK: Color = Color::bytes(17, 21, 31);
const EDGE: Color = Color::bytes(168, 199, 250);
const INK_RGB: [f32; 3] = [17.0 / 255.0, 21.0 / 255.0, 31.0 / 255.0];
const EDGE_RGB: [f32; 3] = [168.0 / 255.0, 199.0 / 255.0, 250.0 / 255.0];
const EDGE_ALPHA: f32 = 0.25;
const SAMPLES: u32 = 4;

#[derive(Clone, Copy)]
struct Slab {
    left: f32,
    top: f32,
    corners: [f32; 4],
    color: Color,
    rgb: [f32; 3],
}

const SLABS: [Slab; 3] = [
    Slab {
        left: 96.0,
        top: 240.0,
        corners: [40.0, 40.0, 40.0, 68.0],
        color: Color::bytes(23, 78, 166),
        rgb: [23.0 / 255.0, 78.0 / 255.0, 166.0 / 255.0],
    },
    Slab {
        left: 142.0,
        top: 174.0,
        corners: [40.0, 48.0, 40.0, 48.0],
        color: Color::bytes(76, 141, 246),
        rgb: [76.0 / 255.0, 141.0 / 255.0, 246.0 / 255.0],
    },
    Slab {
        left: 188.0,
        top: 108.0,
        corners: [40.0, 68.0, 40.0, 48.0],
        color: Color::bytes(120, 243, 224),
        rgb: [120.0 / 255.0, 243.0 / 255.0, 224.0 / 255.0],
    },
];

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
            color: EDGE.with_alpha(EDGE_ALPHA),
        },
        Shadow::NONE,
    );
    for (index, slab) in SLABS.iter().enumerate() {
        if index > 0 {
            let [tl, tr, br, bl] = slab.corners;
            painter.rect(
                Rect::from_min_size(
                    origin + Vec2::new((slab.left - GAP) * scale, (slab.top - GAP) * scale),
                    Vec2::new(
                        (SLAB_WIDTH + 2.0 * GAP) * scale,
                        (SLAB_HEIGHT + 2.0 * GAP) * scale,
                    ),
                ),
                Corners {
                    top_left: (tl + GAP) * scale,
                    top_right: (tr + GAP) * scale,
                    bottom_right: (br + GAP) * scale,
                    bottom_left: (bl + GAP) * scale,
                },
                INK,
            );
        }
        let [tl, tr, br, bl] = slab.corners;
        painter.rect(
            Rect::from_min_size(
                origin + Vec2::new(slab.left * scale, slab.top * scale),
                Vec2::new(SLAB_WIDTH * scale, SLAB_HEIGHT * scale),
            ),
            Corners {
                top_left: tl * scale,
                top_right: tr * scale,
                bottom_right: br * scale,
                bottom_left: bl * scale,
            },
            slab.color,
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
    if !inside_round(x, y, 0.0, 0.0, CANVAS, CANVAS, [RADIUS; 4]) {
        return None;
    }
    if !inside_round(
        x,
        y,
        BORDER,
        BORDER,
        CANVAS - 2.0 * BORDER,
        CANVAS - 2.0 * BORDER,
        [RADIUS - BORDER; 4],
    ) {
        return Some([
            INK_RGB[0] * (1.0 - EDGE_ALPHA) + EDGE_RGB[0] * EDGE_ALPHA,
            INK_RGB[1] * (1.0 - EDGE_ALPHA) + EDGE_RGB[1] * EDGE_ALPHA,
            INK_RGB[2] * (1.0 - EDGE_ALPHA) + EDGE_RGB[2] * EDGE_ALPHA,
        ]);
    }
    for (index, slab) in SLABS.iter().enumerate().rev() {
        if inside_round(
            x,
            y,
            slab.left,
            slab.top,
            SLAB_WIDTH,
            SLAB_HEIGHT,
            slab.corners,
        ) {
            return Some(slab.rgb);
        }
        if index > 0 {
            let [tl, tr, br, bl] = slab.corners;
            if inside_round(
                x,
                y,
                slab.left - GAP,
                slab.top - GAP,
                SLAB_WIDTH + 2.0 * GAP,
                SLAB_HEIGHT + 2.0 * GAP,
                [tl + GAP, tr + GAP, br + GAP, bl + GAP],
            ) {
                return Some(INK_RGB);
            }
        }
    }
    Some(INK_RGB)
}

fn inside_round(
    x: f32,
    y: f32,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    corners: [f32; 4],
) -> bool {
    if x < left || x > left + width || y < top || y > top + height {
        return false;
    }
    let cx = left + 0.5 * width;
    let cy = top + 0.5 * height;
    let radius = match (x >= cx, y >= cy) {
        (false, false) => corners[0],
        (true, false) => corners[1],
        (true, true) => corners[2],
        (false, true) => corners[3],
    }
    .min(0.5 * width)
    .min(0.5 * height);
    let across = (x - cx).abs() - (0.5 * width - radius);
    let down = (y - cy).abs() - (0.5 * height - radius);
    let across = across.max(0.0);
    let down = down.max(0.0);
    across * across + down * down <= radius * radius
}

fn channel(value: f32) -> u8 {
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}
