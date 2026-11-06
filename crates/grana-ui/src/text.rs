use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec2;
use grana_text::Fonts;

use crate::atlas::{Atlas, GlyphSlot};
use crate::color::Color;
use crate::theme::TypeStyle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Baseline,
}

impl Align {
    fn factor(self) -> f32 {
        match self {
            Self::Start | Self::Baseline => 0.0,
            Self::Center => 0.5,
            Self::End => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub font: TypeStyle,
    pub color: Color,
    pub align: Align,
    pub wrap: bool,
    pub truncate: bool,
}

impl TextStyle {
    pub fn new(font: TypeStyle, color: Color) -> Self {
        Self {
            font,
            color,
            align: Align::Start,
            wrap: false,
            truncate: false,
        }
    }

    pub fn aligned(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn wrapping(mut self) -> Self {
        self.wrap = true;
        self
    }

    pub fn truncating(mut self) -> Self {
        self.truncate = true;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlacedGlyph {
    pub face: u16,
    pub id: u16,
    pub x: f32,
    pub baseline: f32,
}

#[derive(Clone, Debug)]
pub struct TextLayout {
    pub text: String,
    pub style: TypeStyle,
    pub glyphs: Vec<PlacedGlyph>,
    pub size: Vec2,
    pub baseline: f32,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Spec {
    size: u32,
    weight: u32,
    leading: u32,
    width: u32,
    pixels_per_point: u32,
    align: u8,
    wrap: bool,
    truncate: bool,
}

struct Entry {
    spec: Spec,
    used: u64,
    layout: Arc<TextLayout>,
}

pub struct Text {
    fonts: Fonts,
    atlas: Atlas,
    entries: HashMap<String, Vec<Entry>>,
    frame: u64,
    pixels_per_point: f32,
}

impl Text {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            fonts: Fonts::system(),
            atlas: Atlas::new(device),
            entries: HashMap::new(),
            frame: 0,
            pixels_per_point: 1.0,
        }
    }

    pub fn description(&self) -> String {
        format!(
            "{} · {} faces · atlas {}²",
            self.fonts.description(),
            self.fonts.face_count(),
            self.atlas.size()
        )
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    pub fn begin(&mut self, pixels_per_point: f32) {
        self.pixels_per_point = pixels_per_point;
        self.frame += 1;
        if self.frame.is_multiple_of(120) {
            self.evict();
        }
    }

    pub fn flush(&mut self, queue: &wgpu::Queue) {
        self.atlas.flush(queue);
    }

    fn evict(&mut self) {
        let floor = self.frame.saturating_sub(240);
        self.entries.retain(|_, entries| {
            entries.retain(|entry| entry.used >= floor);
            !entries.is_empty()
        });
    }

    pub fn layout(
        &mut self,
        text: &str,
        style: TypeStyle,
        max_width: f32,
        wrap: bool,
        truncate: bool,
        align: Align,
    ) -> Arc<TextLayout> {
        let spec = Spec {
            size: (style.size * 4.0).round() as u32,
            weight: (style.weight * 4.0).round() as u32,
            leading: (style.leading * 4.0).round() as u32,
            width: (max_width.max(0.0) * 4.0).round() as u32,
            pixels_per_point: (self.pixels_per_point * 100.0).round() as u32,
            align: align.factor() as u8,
            wrap,
            truncate,
        };
        if let Some(entries) = self.entries.get_mut(text)
            && let Some(entry) = entries.iter_mut().find(|entry| entry.spec == spec)
        {
            entry.used = self.frame;
            return Arc::clone(&entry.layout);
        }
        let layout = Arc::new(self.build(text, style, max_width, wrap, truncate, align));
        self.entries
            .entry(text.to_string())
            .or_default()
            .push(Entry {
                spec,
                used: self.frame,
                layout: Arc::clone(&layout),
            });
        layout
    }

    fn build(
        &mut self,
        text: &str,
        style: TypeStyle,
        max_width: f32,
        wrap: bool,
        truncate: bool,
        align: Align,
    ) -> TextLayout {
        let cjk = text.chars().any(is_cjk);
        let leading = if cjk {
            style.leading.max(style.size * 1.28)
        } else {
            style.leading.max(style.size * 1.05)
        };
        let ascent = style.size * if cjk { 0.88 } else { 0.78 };
        let descent = style.size * if cjk { 0.18 } else { 0.22 };
        let baseline_offset = 0.5 * (leading - (ascent + descent)) + ascent;
        let bounded = max_width > 0.0;
        let mut lines: Vec<(Vec<PlacedGlyph>, f32)> = Vec::new();
        let mut glyphs: Vec<PlacedGlyph> = Vec::new();
        let mut pen = 0.0f32;
        let mut truncated = false;

        let flush = |lines: &mut Vec<(Vec<PlacedGlyph>, f32)>,
                     glyphs: &mut Vec<PlacedGlyph>,
                     pen: &mut f32| {
            if !glyphs.is_empty() || !lines.is_empty() {
                lines.push((std::mem::take(glyphs), *pen));
                *pen = 0.0;
            }
        };

        'paragraphs: for paragraph in text.split('\n') {
            if !glyphs.is_empty() {
                flush(&mut lines, &mut glyphs, &mut pen);
            }
            for token in tokenize(paragraph) {
                let character = token.chars().next().unwrap_or(' ');
                let face = self.fonts.face_for(character);
                let run = self.fonts.shape(face, token, style.size, style.weight, 1.0);
                let whitespace = token.starts_with(' ');
                if wrap && bounded && !whitespace && pen > 0.0 && pen + run.width > max_width + 0.01
                {
                    flush(&mut lines, &mut glyphs, &mut pen);
                }
                if truncate && !wrap && bounded && pen + run.width > max_width {
                    let ellipsis =
                        self.fonts
                            .shape(face, "\u{2026}", style.size, style.weight, 1.0);
                    if pen + ellipsis.width <= max_width {
                        for glyph in &ellipsis.glyphs {
                            glyphs.push(PlacedGlyph {
                                face: face as u16,
                                id: glyph.id,
                                x: pen + glyph.x,
                                baseline: 0.0,
                            });
                        }
                        pen += ellipsis.width;
                    }
                    truncated = true;
                    flush(&mut lines, &mut glyphs, &mut pen);
                    break 'paragraphs;
                }
                for glyph in &run.glyphs {
                    glyphs.push(PlacedGlyph {
                        face: face as u16,
                        id: glyph.id,
                        x: pen + glyph.x,
                        baseline: 0.0,
                    });
                }
                pen += run.width;
            }
            if !glyphs.is_empty() {
                flush(&mut lines, &mut glyphs, &mut pen);
            } else {
                lines.push((Vec::new(), 0.0));
            }
        }
        if lines.is_empty() {
            lines.push((Vec::new(), 0.0));
        }

        let mut widest = 0.0f32;
        for (line, width) in &lines {
            widest = widest.max(*width);
            let _ = line;
        }
        let reference = if bounded { max_width } else { widest };
        let mut placed = Vec::new();
        for (index, (line, width)) in lines.iter().enumerate() {
            let shift = (reference - width) * align.factor();
            let baseline = baseline_offset + index as f32 * leading;
            for glyph in line {
                placed.push(PlacedGlyph {
                    x: glyph.x + shift,
                    baseline,
                    ..*glyph
                });
            }
        }
        let size = Vec2::new(widest, leading * lines.len() as f32);
        TextLayout {
            text: text.to_string(),
            style,
            glyphs: placed,
            size,
            baseline: baseline_offset,
            truncated,
        }
    }

    pub fn pixels_per_point(&self) -> f32 {
        self.pixels_per_point
    }

    pub fn emit(
        &mut self,
        layout: &TextLayout,
        origin: Vec2,
        color: Color,
        mut plot: impl FnMut(GlyphPlot),
    ) {
        if layout.glyphs.is_empty() {
            return;
        }
        let pixels_per_point = self.pixels_per_point;
        let hint = false;
        for glyph in &layout.glyphs {
            let pen_x = (origin.x + glyph.x) * pixels_per_point;
            let baseline = (origin.y + glyph.baseline) * pixels_per_point;
            let Some(slot) = self.atlas.slot(
                &mut self.fonts,
                crate::atlas::GlyphRequest {
                    face: glyph.face as usize,
                    id: glyph.id,
                    style: layout.style,
                    pixels_per_point,
                    hint,
                    position: (pen_x, baseline),
                },
            ) else {
                continue;
            };
            plot(GlyphPlot {
                slot,
                left: (pen_x.floor() + slot.left as f32) / pixels_per_point,
                top: (baseline.floor() - slot.top as f32) / pixels_per_point,
                width: slot.width as f32 / pixels_per_point,
                height: slot.height as f32 / pixels_per_point,
                color,
            });
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphPlot {
    pub slot: GlyphSlot,
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub color: Color,
}

fn tokenize(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0usize;
    let mut previous = None;
    let mut whitespace = false;
    for (index, character) in text.char_indices() {
        let space = character == ' ';
        if let Some(before) = previous {
            let boundary =
                space != whitespace || (!space && !whitespace && allows_break(before, character));
            if boundary {
                tokens.push(&text[start..index]);
                start = index;
            }
        }
        previous = Some(character);
        whitespace = space;
    }
    if start < text.len() {
        tokens.push(&text[start..]);
    }
    tokens
}

fn allows_break(before: char, after: char) -> bool {
    if is_opening(before) || is_closing(after) {
        return false;
    }
    is_cjk(before) || is_cjk(after) || before == '-'
}

fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x2E80..=0x2EFF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
            | 0x20000..=0x2FA1F
    )
}

fn is_opening(character: char) -> bool {
    matches!(
        character,
        '（' | '《' | '「' | '『' | '【' | '〔' | '〈' | '(' | '[' | '{'
    )
}

fn is_closing(character: char) -> bool {
    matches!(
        character,
        '，' | '。'
            | '！'
            | '？'
            | '；'
            | '：'
            | '、'
            | '）'
            | '》'
            | '」'
            | '』'
            | '】'
            | '〕'
            | '〉'
            | '…'
            | '—'
            | '·'
            | ','
            | '.'
            | ';'
            | ':'
            | '!'
            | '?'
            | ')'
            | ']'
            | '}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_text_breaks_between_characters() {
        assert_eq!(tokenize("中文测试"), ["中", "文", "测", "试"]);
        assert_eq!(tokenize("中文，测试"), ["中", "文，", "测", "试"]);
        assert_eq!(tokenize("abc def"), ["abc", " ", "def"]);
    }

    #[test]
    fn cjk_text_uses_a_fallback_face() {
        let (device, _) = crate::test_gpu::gpu();
        let mut text = Text::new(&device);
        text.begin(1.0);
        let style = TypeStyle::new(14.0, 400.0, 18.0);
        let layout = text.layout("中文测试", style, 0.0, false, false, Align::Start);
        assert_eq!(layout.glyphs.len(), 4);
        assert!(layout.size.x > 0.0);
        assert!(layout.size.y > 0.0);
    }
}
