use std::io;
use std::path::Path;

use grana_ui::Palette;

use crate::art::suffix;
use crate::canvas::Canvas;
use crate::icon;

const WIDTH: f32 = 660.0;
const HEIGHT: f32 = 360.0;
const MARK: f32 = 68.0;
const ICONS: f32 = 190.0;

pub fn write(dir: &Path) -> io::Result<()> {
    let palette = Palette::dark();
    for scale in [1u32, 2] {
        background(scale, &palette)
            .write(&dir.join(format!("dmg-background{}.png", suffix(scale))))?;
    }
    icon::write_icns(
        &dir.join("Grana.icns"),
        &[
            (b"icp4", 16),
            (b"icp5", 32),
            (b"icp6", 64),
            (b"ic07", 128),
            (b"ic08", 256),
            (b"ic09", 512),
            (b"ic10", 1024),
            (b"ic11", 32),
            (b"ic12", 64),
            (b"ic13", 256),
            (b"ic14", 512),
        ],
    )
}

fn background(scale: u32, palette: &Palette) -> Canvas {
    let size = scale as f32;
    let mut canvas = Canvas::new((WIDTH * size) as u32, (HEIGHT * size) as u32);
    canvas.fill(palette.surface);
    canvas.glow(
        (WIDTH * size * 0.5, 0.0),
        340.0 * size,
        palette.primary.with_alpha(0.10),
    );
    canvas.glow(
        (WIDTH * size * 0.92, HEIGHT * size * 1.05),
        300.0 * size,
        palette.tertiary.with_alpha(0.05),
    );
    let mark_size = (MARK * size) as u32;
    let mark = grana_brand::icon(mark_size);
    canvas.blit(
        &mark,
        mark_size,
        (((WIDTH - MARK) * 0.5 * size) as i32, (38.0 * size) as i32),
    );
    let center = (WIDTH * 0.5 * size, ICONS * size);
    let reach = 7.0 * size;
    canvas.line(
        (center.0 - reach, center.1 - reach),
        (center.0 + reach, center.1),
        2.6 * size,
        palette.on_surface_variant.with_alpha(0.5),
    );
    canvas.line(
        (center.0 + reach, center.1),
        (center.0 - reach, center.1 + reach),
        2.6 * size,
        palette.on_surface_variant.with_alpha(0.5),
    );
    canvas
}
