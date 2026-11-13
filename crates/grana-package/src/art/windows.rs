use std::io;
use std::path::Path;

use grana_ui::{Color, Palette};

use crate::art::{SCALES, suffix};
use crate::canvas::{Area, Canvas};
use crate::icon;

const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 460.0;
const MARK: f32 = 52.0;
const PRIMARY_WIDTH: f32 = 132.0;
const BUTTON_HEIGHT: f32 = 44.0;
const GHOST_WIDTH: f32 = 110.0;
const CLOSE: f32 = 32.0;
const TRACK_WIDTH: f32 = 624.0;
const TRACK_HEIGHT: f32 = 10.0;

pub fn write(dir: &Path) -> io::Result<()> {
    let palette = Palette::dark();
    for scale in SCALES {
        let suffix = suffix(scale);
        surface(scale, &palette).write(&dir.join(format!("surface{suffix}.png")))?;
        mark(scale).write(&dir.join(format!("mark{suffix}.png")))?;
        primary(scale, palette.primary, 22.0).write(&dir.join(format!("primary{suffix}.png")))?;
        primary(scale, palette.primary.lerp(palette.on_primary, 0.08), 22.0)
            .write(&dir.join(format!("primary-hover{suffix}.png")))?;
        primary(scale, palette.primary.lerp(palette.on_primary, 0.12), 12.0)
            .write(&dir.join(format!("primary-pressed{suffix}.png")))?;
        primary_focus(scale, &palette).write(&dir.join(format!("primary-focus{suffix}.png")))?;
        ghost(scale, Color::TRANSPARENT).write(&dir.join(format!("ghost{suffix}.png")))?;
        ghost(scale, palette.on_surface.with_alpha(0.08))
            .write(&dir.join(format!("ghost-hover{suffix}.png")))?;
        ghost(scale, palette.on_surface.with_alpha(0.12))
            .write(&dir.join(format!("ghost-pressed{suffix}.png")))?;
        ghost_focus(scale, &palette).write(&dir.join(format!("ghost-focus{suffix}.png")))?;
        close(scale, &palette, None).write(&dir.join(format!("close{suffix}.png")))?;
        close(scale, &palette, Some(palette.on_surface.with_alpha(0.08)))
            .write(&dir.join(format!("close-hover{suffix}.png")))?;
        close(scale, &palette, Some(palette.on_surface.with_alpha(0.12)))
            .write(&dir.join(format!("close-pressed{suffix}.png")))?;
        track(scale, &palette).write(&dir.join(format!("track{suffix}.png")))?;
    }
    fill(&palette).write(&dir.join("fill.png"))?;
    icon::write_ico(
        &dir.join("grana.ico"),
        &[16, 20, 24, 32, 40, 48, 64, 128, 256],
    )
}

fn surface(scale: u32, palette: &Palette) -> Canvas {
    let size = scale as f32;
    let mut canvas = Canvas::new((WIDTH * size) as u32, (HEIGHT * size) as u32);
    canvas.fill(palette.surface);
    canvas.glow(
        (700.0 * size, 20.0 * size),
        520.0 * size,
        palette.primary.with_alpha(0.11),
    );
    canvas.glow(
        (760.0 * size, 470.0 * size),
        380.0 * size,
        palette.tertiary.with_alpha(0.06),
    );
    canvas.rounded(
        Area::new(540.0, 320.0, 300.0, 190.0).scaled(size),
        44.0 * size,
        Color::bytes(23, 78, 166).with_alpha(0.16),
    );
    canvas.rounded(
        Area::new(588.0, 266.0, 264.0, 168.0).scaled(size),
        40.0 * size,
        Color::bytes(76, 141, 246).with_alpha(0.12),
    );
    canvas.rounded(
        Area::new(636.0, 212.0, 228.0, 146.0).scaled(size),
        36.0 * size,
        Color::bytes(120, 243, 224).with_alpha(0.09),
    );
    canvas
}

fn mark(scale: u32) -> Canvas {
    let size = (MARK * scale as f32) as u32;
    let icon = grana_brand::icon(size);
    let mut canvas = Canvas::new(size, size);
    canvas.blit(&icon, size, (0, 0));
    canvas
}

fn primary(scale: u32, color: Color, radius: f32) -> Canvas {
    let size = scale as f32;
    let mut canvas = Canvas::new((PRIMARY_WIDTH * size) as u32, (BUTTON_HEIGHT * size) as u32);
    canvas.rounded(
        Area::new(0.0, 0.0, PRIMARY_WIDTH * size, BUTTON_HEIGHT * size),
        radius * size,
        color,
    );
    canvas
}

fn primary_focus(scale: u32, palette: &Palette) -> Canvas {
    let size = scale as f32;
    let mut canvas = primary(scale, palette.primary, 22.0);
    canvas.ring(
        Area::new(
            2.5 * size,
            2.5 * size,
            (PRIMARY_WIDTH - 5.0) * size,
            (BUTTON_HEIGHT - 5.0) * size,
        ),
        19.5 * size,
        1.6 * size,
        palette.on_primary.with_alpha(0.85),
    );
    canvas
}

fn ghost(scale: u32, color: Color) -> Canvas {
    let size = scale as f32;
    let mut canvas = Canvas::new((GHOST_WIDTH * size) as u32, (BUTTON_HEIGHT * size) as u32);
    if color.alpha > 0.0 {
        canvas.rounded(
            Area::new(0.0, 0.0, GHOST_WIDTH * size, BUTTON_HEIGHT * size),
            22.0 * size,
            color,
        );
    }
    canvas
}

fn ghost_focus(scale: u32, palette: &Palette) -> Canvas {
    let size = scale as f32;
    let mut canvas = ghost(scale, Color::TRANSPARENT);
    canvas.ring(
        Area::new(
            1.5 * size,
            1.5 * size,
            (GHOST_WIDTH - 3.0) * size,
            (BUTTON_HEIGHT - 3.0) * size,
        ),
        20.5 * size,
        1.2 * size,
        palette.outline.with_alpha(0.9),
    );
    canvas
}

fn close(scale: u32, palette: &Palette, background: Option<Color>) -> Canvas {
    let size = scale as f32;
    let mut canvas = Canvas::new((CLOSE * size) as u32, (CLOSE * size) as u32);
    let center = (CLOSE * size * 0.5, CLOSE * size * 0.5);
    if let Some(color) = background {
        canvas.circle(center, CLOSE * size * 0.5, color);
    }
    let arm = 4.5 * size;
    for (from, to) in [
        (
            (center.0 - arm, center.1 - arm),
            (center.0 + arm, center.1 + arm),
        ),
        (
            (center.0 - arm, center.1 + arm),
            (center.0 + arm, center.1 - arm),
        ),
    ] {
        canvas.line(from, to, 1.35 * size, palette.on_surface_variant);
    }
    canvas
}

fn track(scale: u32, palette: &Palette) -> Canvas {
    let size = scale as f32;
    let mut canvas = Canvas::new((TRACK_WIDTH * size) as u32, (TRACK_HEIGHT * size) as u32);
    canvas.rounded(
        Area::new(0.0, 0.0, TRACK_WIDTH * size, TRACK_HEIGHT * size),
        TRACK_HEIGHT * size * 0.5,
        palette.surface_container_highest,
    );
    canvas
}

fn fill(palette: &Palette) -> Canvas {
    let mut canvas = Canvas::new(4, 8);
    for y in 0..8 {
        canvas.set(1, y, palette.primary);
    }
    canvas
}
