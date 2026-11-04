use grana_text::{Fonts, GlyphStyle};

use crate::Raster;
use crate::raster::over;

pub fn measure(fonts: &mut Fonts, text: &str, size: f32, weight: f32) -> u32 {
    let mut width = 0.0f32;
    for run in runs(fonts, text) {
        width += fonts.shape(run.face, run.text, size, weight, 1.0).width;
    }
    width.round() as u32
}

impl Raster {
    pub fn text(
        &mut self,
        fonts: &mut Fonts,
        text: &str,
        at: [u32; 2],
        size: f32,
        weight: f32,
        color: [u8; 4],
    ) -> u32 {
        let mut pen = 0.0f32;
        for run in runs(fonts, text) {
            let shaped = fonts.shape(run.face, run.text, size, weight, 1.0);
            for glyph in &shaped.glyphs {
                let Some(raster) = fonts.rasterize(
                    run.face,
                    glyph.id,
                    GlyphStyle {
                        size,
                        weight,
                        hint: true,
                    },
                    1.0,
                    [0.0, 0.0],
                ) else {
                    continue;
                };
                let x = at[0] as i32 + (pen + glyph.x).round() as i32 + raster.left;
                let y = at[1] as i32 + (shaped.ascent - glyph.y - raster.top as f32).round() as i32;
                self.blend(&raster, [x, y], color);
            }
            pen += shaped.width;
        }
        pen.round() as u32
    }

    fn blend(&mut self, glyph: &grana_text::GlyphRaster, at: [i32; 2], color: [u8; 4]) {
        if glyph.width == 0 || glyph.height == 0 {
            return;
        }
        for row in 0..glyph.height {
            let y = at[1] + row as i32;
            if y < 0 || y >= self.height() as i32 {
                continue;
            }
            for column in 0..glyph.width {
                let x = at[0] + column as i32;
                if x < 0 || x >= self.width() as i32 {
                    continue;
                }
                let coverage = glyph.data[(row * glyph.width + column) as usize];
                if coverage == 0 {
                    continue;
                }
                let alpha = u32::from(coverage) * u32::from(color[3]) / 255;
                if alpha == 0 {
                    continue;
                }
                let source = [color[0], color[1], color[2], alpha as u8];
                let under = self.pixel(x as u32, y as u32);
                let blended = over(under, source);
                self.set(x as u32, y as u32, blended);
            }
        }
    }

    fn set(&mut self, x: u32, y: u32, pixel: [u8; 4]) {
        let slot = (x + self.width() * y) as usize * 4;
        self.pixels_mut()[slot..slot + 4].copy_from_slice(&pixel);
    }
}

struct Run<'a> {
    face: usize,
    text: &'a str,
}

fn runs<'a>(fonts: &mut Fonts, text: &'a str) -> Vec<Run<'a>> {
    let mut runs: Vec<Run<'a>> = Vec::new();
    let mut start = 0usize;
    let mut face = None;
    for (offset, character) in text.char_indices() {
        let chosen = fonts.face_for(character);
        match face {
            None => face = Some(chosen),
            Some(current) if current != chosen => {
                runs.push(Run {
                    face: current,
                    text: &text[start..offset],
                });
                start = offset;
                face = Some(chosen);
            }
            Some(_) => {}
        }
    }
    if let Some(face) = face {
        runs.push(Run {
            face,
            text: &text[start..],
        });
    }
    runs
}
