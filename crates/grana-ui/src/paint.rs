use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::style::{Border, Fill, Glass, Shadow};
use crate::text::{Text, TextLayout, TextStyle};

pub const KIND_SURFACE: u32 = 0;
pub const KIND_GLASS: u32 = 1;
pub const KIND_SHADOW: u32 = 2;
pub const KIND_GLYPH: u32 = 3;
pub const KIND_SCENE: u32 = 4;
pub const KIND_SEGMENT: u32 = 5;
pub const KIND_RING: u32 = 6;

pub const FLAG_GRADIENT: u32 = 1 << 0;
pub const FLAG_BORDER: u32 = 1 << 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Instance {
    pub rect: [f32; 4],
    pub corners: [f32; 4],
    pub color: [f32; 4],
    pub color2: [f32; 4],
    pub glass: [f32; 4],
    pub glass2: [f32; 4],
    pub uv: [f32; 4],
    pub clip: [f32; 4],
    pub clip_corners: [f32; 4],
    pub meta: [u32; 4],
    pub extra: [f32; 4],
}

impl Instance {
    fn blank(kind: u32) -> Self {
        Self {
            rect: [0.0; 4],
            corners: [0.0; 4],
            color: [1.0, 1.0, 1.0, 1.0],
            color2: [0.0; 4],
            glass: [0.0; 4],
            glass2: [0.0; 4],
            uv: [0.0, 0.0, 1.0, 1.0],
            clip: [0.0, 0.0, 0.0, 0.0],
            clip_corners: [0.0; 4],
            meta: [kind, 0, 0, 0],
            extra: [0.0; 4],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Clip {
    pub rect: Rect,
    pub corners: Corners,
}

impl Clip {
    pub fn new(rect: Rect, corners: Corners) -> Self {
        Self { rect, corners }
    }

    pub fn intersect(self, rect: Rect, corners: Corners) -> Self {
        let resolved = corners.resolved(rect.size());
        Self {
            rect: self.rect.intersect(rect),
            corners: Corners {
                top_left: self.corners.top_left.min(resolved.top_left),
                top_right: self.corners.top_right.min(resolved.top_right),
                bottom_right: self.corners.bottom_right.min(resolved.bottom_right),
                bottom_left: self.corners.bottom_left.min(resolved.bottom_left),
            },
        }
    }
}

pub struct Painter<'a> {
    instances: &'a mut Vec<Instance>,
    text: &'a mut Text,
    clip: Clip,
    gamma: f32,
}

impl<'a> Painter<'a> {
    pub fn new(
        instances: &'a mut Vec<Instance>,
        text: &'a mut Text,
        clip: Clip,
        dark: bool,
    ) -> Self {
        Self {
            instances,
            text,
            clip,
            gamma: if dark { 0.86 } else { 1.0 },
        }
    }

    pub fn clip(&self) -> Clip {
        self.clip
    }

    pub fn push_clip(&mut self, rect: Rect, corners: Corners) {
        self.clip = self.clip.intersect(rect, corners);
    }

    pub fn pop_clip(&mut self, previous: Clip) {
        self.clip = previous;
    }

    fn push(&mut self, mut instance: Instance) {
        let clip = self.clip;
        instance.clip = [
            clip.rect.min.x,
            clip.rect.min.y,
            clip.rect.width(),
            clip.rect.height(),
        ];
        instance.clip_corners = [
            clip.corners.top_left,
            clip.corners.top_right,
            clip.corners.bottom_right,
            clip.corners.bottom_left,
        ];
        self.instances.push(instance);
    }

    pub fn count(&self) -> usize {
        self.instances.len()
    }

    pub fn rect(&mut self, rect: Rect, corners: Corners, color: Color) {
        if rect.is_empty() || color.alpha <= 0.0 {
            return;
        }
        let mut instance = Instance::blank(KIND_SURFACE);
        instance.rect = [rect.min.x, rect.min.y, rect.width(), rect.height()];
        instance.corners = corners_array(corners.resolved(rect.size()));
        instance.color = color.to_array();
        self.push(instance);
    }

    pub fn gradient(&mut self, rect: Rect, corners: Corners, top: Color, bottom: Color) {
        if rect.is_empty() || (top.alpha <= 0.0 && bottom.alpha <= 0.0) {
            return;
        }
        let mut instance = Instance::blank(KIND_SURFACE);
        instance.rect = [rect.min.x, rect.min.y, rect.width(), rect.height()];
        instance.corners = corners_array(corners.resolved(rect.size()));
        instance.color = top.to_array();
        instance.color2 = bottom.to_array();
        instance.meta[1] |= FLAG_GRADIENT;
        self.push(instance);
    }

    pub fn outlined(
        &mut self,
        rect: Rect,
        corners: Corners,
        fill: Fill,
        border: Border,
        shadow: Shadow,
    ) {
        if shadow.color.alpha > 0.0 && shadow.radius > 0.0 {
            self.shadow(rect, corners, shadow);
        }
        match fill {
            Fill::None => {}
            Fill::Solid(color) => self.rect(rect, corners, color),
            Fill::Gradient(top, bottom) => self.gradient(rect, corners, top, bottom),
            Fill::Glass(glass) => self.glass(rect, corners, glass),
            Fill::Scene(uv) => self.scene(rect, corners, uv),
        }
        if border.width > 0.0 && border.color.alpha > 0.0 {
            let mut instance = Instance::blank(KIND_SURFACE);
            let inset = 0.5 * border.width;
            let outline = rect.shrink(inset);
            instance.rect = [
                outline.min.x,
                outline.min.y,
                outline.width(),
                outline.height(),
            ];
            instance.corners = corners_array(corners.resolved(rect.size()));
            instance.color = Color::TRANSPARENT.to_array();
            instance.color2 = border.color.to_array();
            instance.meta[1] |= FLAG_BORDER;
            instance.extra = [border.width, 0.0, 0.0, 0.0];
            self.push(instance);
        }
    }

    pub fn glass(&mut self, rect: Rect, corners: Corners, glass: Glass) {
        if rect.is_empty() || glass.tint.alpha <= 0.0 {
            return;
        }
        let mut instance = Instance::blank(KIND_GLASS);
        instance.rect = [rect.min.x, rect.min.y, rect.width(), rect.height()];
        instance.corners = corners_array(corners.resolved(rect.size()));
        instance.color = glass.tint.to_array();
        instance.glass = [
            glass.blur,
            glass.refraction,
            glass.dispersion,
            glass.thickness,
        ];
        instance.glass2 = [glass.saturation, glass.brightness, glass.rim, glass.sheen];
        instance.extra = [glass.specular, 0.0, 0.0, 0.0];
        self.push(instance);
    }

    pub fn shadow(&mut self, rect: Rect, corners: Corners, shadow: Shadow) {
        if rect.is_empty() || shadow.color.alpha <= 0.0 {
            return;
        }
        let mut instance = Instance::blank(KIND_SHADOW);
        let grow = 0.5 * shadow.radius;
        let expanded = rect.translate(shadow.offset).expand(grow);
        let resolved = corners.resolved(rect.size());
        let grown = Corners {
            top_left: resolved.top_left + grow,
            top_right: resolved.top_right + grow,
            bottom_right: resolved.bottom_right + grow,
            bottom_left: resolved.bottom_left + grow,
        };
        instance.rect = [
            expanded.min.x,
            expanded.min.y,
            expanded.width(),
            expanded.height(),
        ];
        instance.corners = corners_array(grown.resolved(expanded.size()));
        instance.color = shadow.color.to_array();
        instance.color2 = corners_array(resolved);
        instance.uv = [rect.min.x, rect.min.y, rect.width(), rect.height()];
        instance.extra = [shadow.radius * 0.5, 0.0, 0.0, 0.0];
        self.push(instance);
    }

    pub fn scene(&mut self, rect: Rect, corners: Corners, uv: [f32; 4]) {
        if rect.is_empty() {
            return;
        }
        let mut instance = Instance::blank(KIND_SCENE);
        instance.rect = [rect.min.x, rect.min.y, rect.width(), rect.height()];
        instance.corners = corners_array(corners.resolved(rect.size()));
        instance.uv = uv;
        self.push(instance);
    }

    pub fn segment(&mut self, from: Vec2, to: Vec2, width: f32, color: Color) {
        self.segment_gradient(from, to, width, color, color);
    }

    pub fn segment_gradient(&mut self, from: Vec2, to: Vec2, width: f32, start: Color, end: Color) {
        if width <= 0.0 || (start.alpha <= 0.0 && end.alpha <= 0.0) {
            return;
        }
        let mut instance = Instance::blank(KIND_SEGMENT);
        instance.rect = [from.x, from.y, to.x, to.y];
        instance.color = start.to_array();
        instance.color2 = end.to_array();
        instance.extra = [width, 0.0, 0.0, 0.0];
        self.push(instance);
    }

    pub fn ring(
        &mut self,
        center: Vec2,
        radius: f32,
        width: f32,
        start: f32,
        sweep: f32,
        color: Color,
    ) {
        if width <= 0.0 || color.alpha <= 0.0 {
            return;
        }
        let mut instance = Instance::blank(KIND_RING);
        instance.rect = [center.x, center.y, radius, width];
        instance.color = color.to_array();
        instance.extra = [start, sweep, 0.0, 0.0];
        self.push(instance);
    }

    pub fn glyphs(&mut self, layout: &TextLayout, origin: Vec2, color: Color) {
        let gamma = self.gamma;
        let mut plots = Vec::with_capacity(layout.glyphs.len());
        self.text
            .emit(layout, origin, color, |plot| plots.push(plot));
        for plot in plots {
            let mut instance = Instance::blank(KIND_GLYPH);
            instance.rect = [plot.left, plot.top, plot.width, plot.height];
            instance.color = plot.color.to_array();
            instance.uv = plot.slot.uv;
            instance.extra = [gamma, 0.0, 0.0, 0.0];
            self.push(instance);
        }
    }

    pub fn text(&mut self, layout: &TextLayout, position: Vec2, color: Color) {
        self.glyphs(layout, position, color);
    }

    pub fn text_at(&mut self, position: Vec2, text: &str, style: TextStyle) {
        let layout = self
            .text
            .layout(text, style.font, 0.0, false, style.truncate, style.align);
        self.glyphs(&layout, position, style.color);
    }

    pub fn text_in(
        &mut self,
        rect: Rect,
        text: &str,
        style: TextStyle,
        vertical: crate::text::Align,
    ) {
        let layout = self.text.layout(
            text,
            style.font,
            rect.width(),
            style.wrap,
            style.truncate,
            style.align,
        );
        let factor = match vertical {
            crate::text::Align::Start => 0.0,
            crate::text::Align::Center => 0.5,
            crate::text::Align::End => 1.0,
        };
        let origin = Vec2::new(
            rect.min.x,
            rect.min.y + (rect.height() - layout.size.y) * factor,
        );
        self.glyphs(&layout, origin, style.color);
    }
}

pub fn corners_array(corners: Corners) -> [f32; 4] {
    [
        corners.top_left,
        corners.top_right,
        corners.bottom_right,
        corners.bottom_left,
    ]
}
