use std::path::{Path, PathBuf};

use swash::scale::{Render, ScaleContext, Scaler, Source};
use swash::shape::{ShapeContext, Shaper};
use swash::zeno::{Format, Vector};
use swash::{FontRef, Setting};

use crate::theme::TypeStyle;

const FACE_CANDIDATES: &[(&str, &[&str])] = &[
    (
        "windows",
        &[
            "C:/Windows/Fonts/SegUIVar.ttf",
            "C:/Windows/Fonts/segoeui.ttf",
        ],
    ),
    (
        "macos",
        &[
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/SFNSDisplay.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ],
    ),
    (
        "linux",
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        ],
    ),
];

const FALLBACK_CANDIDATES: &[&str] = &[
    "C:/Windows/Fonts/seguisym.ttf",
    "C:/Windows/Fonts/segoeuisl.ttf",
    "/System/Library/Fonts/Apple Symbols.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

pub struct Face {
    font: FontRef<'static>,
}

impl Face {
    fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        let leaked: &'static [u8] = Box::leak(data.into_boxed_slice());
        let font = FontRef::from_index(leaked, 0)?;
        Some(Self { font })
    }
}

pub struct Fonts {
    faces: Vec<Face>,
    shape: ShapeContext,
    scale: ScaleContext,
    names: Vec<String>,
}

impl Fonts {
    pub fn system() -> Self {
        let mut paths = Vec::new();
        if let Ok(path) = std::env::var("GRANA_UI_FONT") {
            paths.push(PathBuf::from(path));
        }
        for (platform, candidates) in FACE_CANDIDATES {
            if std::env::consts::OS == *platform {
                paths.extend(candidates.iter().map(PathBuf::from));
            }
        }
        let mut faces = Vec::new();
        let mut names = Vec::new();
        for path in &paths {
            if let Some(face) = Face::load(path) {
                faces.push(face);
                names.push(path.display().to_string());
            }
        }
        for candidate in FALLBACK_CANDIDATES {
            let path = Path::new(candidate);
            if let Some(face) = Face::load(path) {
                faces.push(face);
                names.push(candidate.to_string());
            }
        }
        assert!(
            !faces.is_empty(),
            "no system UI font is available; set GRANA_UI_FONT to a TrueType font file"
        );
        Self {
            faces,
            shape: ShapeContext::new(),
            scale: ScaleContext::new(),
            names,
        }
    }

    pub fn description(&self) -> &str {
        self.names.first().map_or("unknown font", String::as_str)
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    pub fn face_for(&self, character: char) -> usize {
        for (index, face) in self.faces.iter().enumerate() {
            if face.font.charmap().map(character) != 0 {
                return index;
            }
        }
        0
    }

    fn shaper(
        &mut self,
        font: FontRef<'static>,
        style: TypeStyle,
        pixels_per_point: f32,
    ) -> Shaper<'_> {
        self.shape
            .builder(font)
            .size(style.size * pixels_per_point)
            .variations([Setting::from(("wght", style.weight))])
            .build()
    }

    pub fn shape(
        &mut self,
        index: usize,
        text: &str,
        style: TypeStyle,
        pixels_per_point: f32,
    ) -> ShapedRun {
        let font = self.faces[index].font;
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
        style: TypeStyle,
        pixels_per_point: f32,
        hint: bool,
        offset: Vector,
    ) -> Option<GlyphRaster> {
        let font = self.faces[index].font;
        let scaler: Scaler<'_> = self
            .scale
            .builder(font)
            .size(style.size * pixels_per_point)
            .hint(hint)
            .variations([Setting::from(("wght", style.weight))])
            .build();
        let mut scaler = scaler;
        let render = Render::new(&[Source::Outline]);
        let mut render = render;
        render.format(Format::Alpha).offset(offset);
        let image = render.render(&mut scaler, glyph)?;
        let placement = image.placement;
        let data = image.data.to_vec();
        Some(GlyphRaster {
            left: placement.left,
            top: placement.top,
            width: placement.width,
            height: placement.height,
            data,
        })
    }

    pub fn has_glyph(&self, index: usize, character: char) -> bool {
        self.faces[index].font.charmap().map(character) != 0
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

#[derive(Clone, Debug)]
pub struct GlyphRaster {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}
