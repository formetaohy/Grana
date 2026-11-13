use std::path::PathBuf;

use swash::scale::{Render, ScaleContext, Scaler, Source};
use swash::shape::{ShapeContext, Shaper};
use swash::zeno::{Format, Vector};
use swash::{FontRef, Setting};

use crate::face::{self, Face};
use crate::glyph::GlyphRaster;
use crate::outline::Outline;

const PRIMARY_FLEX: usize = 0;
const PRIMARY_MONO: usize = 1;
const FALLBACK_START: usize = 2;

pub struct Fonts {
    faces: Vec<Face>,
    shape: ShapeContext,
    scale: ScaleContext,
    names: Vec<String>,
    pending: Vec<PathBuf>,
}

impl Fonts {
    pub fn system() -> Self {
        let mut faces = Vec::new();
        let mut names = Vec::new();
        for (face, name) in face::primary_faces() {
            faces.push(face);
            names.push(name);
        }
        Self {
            faces,
            shape: ShapeContext::new(),
            scale: ScaleContext::new(),
            names,
            pending: face::fallback_paths(),
        }
    }

    pub fn description(&self) -> &str {
        self.names.first().map_or("unknown font", String::as_str)
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    pub fn face_for(&mut self, character: char) -> usize {
        if self.faces[PRIMARY_FLEX].covers(character) {
            return PRIMARY_FLEX;
        }
        if let Some(offset) = self.faces[FALLBACK_START..]
            .iter()
            .position(|face| face.covers(character))
        {
            return FALLBACK_START + offset;
        }
        while !self.pending.is_empty() {
            let path = self.pending.remove(0);
            let Some(face) = Face::load(&path) else {
                continue;
            };
            let has_glyph = face.covers(character);
            self.faces.push(face);
            self.names.push(path.display().to_string());
            if has_glyph {
                return self.faces.len() - 1;
            }
        }
        PRIMARY_FLEX
    }

    pub fn face_for_mono(&mut self, character: char) -> usize {
        if self.faces[PRIMARY_MONO].covers(character) {
            return PRIMARY_MONO;
        }
        self.face_for(character)
    }

    fn font(&self, index: usize) -> FontRef<'static> {
        self.faces[index].font
    }

    fn shaper(
        &mut self,
        font: FontRef<'static>,
        style: GlyphStyle,
        pixels_per_point: f32,
    ) -> Shaper<'_> {
        self.shape
            .builder(font)
            .size(style.size * pixels_per_point)
            .variations(variations(style))
            .build()
    }

    pub fn shape(
        &mut self,
        index: usize,
        text: &str,
        style: GlyphStyle,
        pixels_per_point: f32,
    ) -> ShapedRun {
        let font = self.font(index);
        let mut shaper = self.shaper(font, style, pixels_per_point);
        let metrics = shaper.metrics();
        shaper.add_str(text);
        let mut glyphs = Vec::new();
        let mut width = 0.0f32;
        shaper.shape_with(|cluster| {
            for glyph in cluster.glyphs {
                let advance = glyph.advance;
                glyphs.push(ShapedGlyph {
                    id: glyph.id,
                    x: width + glyph.x,
                    y: glyph.y,
                    advance,
                });
                width += advance;
            }
        });
        ShapedRun {
            glyphs,
            width,
            ascent: metrics.ascent,
            descent: metrics.descent,
            leading: metrics.leading,
        }
    }

    pub fn rasterize(
        &mut self,
        index: usize,
        glyph: u16,
        style: GlyphStyle,
        pixels_per_point: f32,
        offset: [f32; 2],
    ) -> Option<GlyphRaster> {
        let font = self.font(index);
        let mut scaler: Scaler<'_> = self
            .scale
            .builder(font)
            .size(style.size * pixels_per_point)
            .hint(style.hint)
            .variations(variations(style))
            .build();
        let mut render = Render::new(&[Source::Outline]);
        render
            .format(Format::Alpha)
            .offset(Vector::new(offset[0], offset[1]));
        let image = render.render(&mut scaler, glyph)?;
        let placement = image.placement;
        Some(GlyphRaster {
            left: placement.left,
            top: placement.top,
            width: placement.width,
            height: placement.height,
            data: image.data.to_vec(),
        })
    }

    pub fn outline(&mut self, index: usize, glyph: u16, style: GlyphStyle) -> Option<Outline> {
        assert!(
            style.size.is_finite() && style.size > 0.0,
            "an outline is scaled to a positive size"
        );
        let font = self.font(index);
        let mut scaler: Scaler<'_> = self
            .scale
            .builder(font)
            .size(style.size)
            .hint(false)
            .variations(variations(style))
            .build();
        let scaled = scaler.scale_outline(glyph)?;
        let mut outline = Outline::new();
        for layer in 0..scaled.len() {
            if let Some(layer) = scaled.get(layer) {
                outline.absorb(crate::outline::collect(layer.path()));
            }
        }
        Some(outline)
    }

    pub fn has_glyph(&self, index: usize, character: char) -> bool {
        self.faces[index].covers(character)
    }
}

fn variations(style: GlyphStyle) -> [Setting<f32>; 4] {
    [
        Setting::from(("wght", style.weight)),
        Setting::from(("wdth", style.width)),
        Setting::from(("opsz", style.size.clamp(8.0, 144.0))),
        Setting::from(("GRAD", style.grade)),
    ]
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphStyle {
    pub size: f32,
    pub weight: f32,
    pub width: f32,
    pub grade: f32,
    pub hint: bool,
}

impl GlyphStyle {
    pub const fn new(size: f32, weight: f32) -> Self {
        Self {
            size,
            weight,
            width: 100.0,
            grade: 0.0,
            hint: false,
        }
    }

    pub const fn with_axes(mut self, width: f32, grade: f32) -> Self {
        self.width = width;
        self.grade = grade;
        self
    }

    pub const fn hinted(mut self, hint: bool) -> Self {
        self.hint = hint;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ShapedGlyph {
    pub id: u16,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub leading: f32,
}
