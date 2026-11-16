use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use grana_raster::Bitmap;
use grana_text::{Contour, Curve};

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::image::ImageAtlas;
use crate::record::Mark;
use crate::style::{Border, Fill, Shadow, Surface};
use crate::text::{Align, Text, TextLayout, TextStyle};

pub const KIND_SURFACE: u32 = 0;
pub const KIND_SHADOW: u32 = 1;
pub const KIND_GLYPH: u32 = 2;
pub const KIND_SCENE: u32 = 3;
pub const KIND_SEGMENT: u32 = 4;
pub const KIND_RING: u32 = 5;
pub const KIND_TRIANGLE: u32 = 6;
pub const KIND_IMAGE: u32 = 7;

pub const FLAG_GRADIENT: u32 = 1 << 0;
pub const FLAG_BORDER: u32 = 1 << 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Instance {
    pub rect: [f32; 4],
    pub corners: [f32; 4],
    pub color: [f32; 4],
    pub color2: [f32; 4],
    pub uv: [f32; 4],
    pub clip: [f32; 4],
    pub clip_corners: [f32; 4],
    pub meta: [u32; 4],
    pub extra: [f32; 4],
}

impl Instance {
    pub fn translate(&mut self, delta: Vec2) {
        self.rect[0] += delta.x;
        self.rect[1] += delta.y;
        self.clip[0] += delta.x;
        self.clip[1] += delta.y;
        match self.meta[0] {
            KIND_SHADOW => {
                self.uv[0] += delta.x;
                self.uv[1] += delta.y;
            }
            KIND_SEGMENT => {
                self.rect[2] += delta.x;
                self.rect[3] += delta.y;
            }
            KIND_TRIANGLE => {
                self.uv[0] += delta.x;
                self.uv[1] += delta.y;
                self.uv[2] += delta.x;
                self.uv[3] += delta.y;
                self.extra[0] += delta.x;
                self.extra[1] += delta.y;
            }
            _ => {}
        }
    }

    fn blank(kind: u32) -> Self {
        Self {
            rect: [0.0; 4],
            corners: [0.0; 4],
            color: [1.0, 1.0, 1.0, 1.0],
            color2: [0.0; 4],
            uv: [0.0, 0.0, 1.0, 1.0],
            clip: [0.0, 0.0, 0.0, 0.0],
            clip_corners: [0.0; 4],
            meta: [kind, 0, 0, 0],
            extra: [0.0; 4],
        }
    }

    fn touches(&self, clip: crate::geometry::Rect) -> bool {
        let bounds = bounds(self);
        !bounds.is_empty() && bounds.intersects(clip)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Default)]
pub(crate) struct Instances {
    pub material: Vec<Instance>,
}

impl Instances {
    pub fn clear(&mut self) {
        self.material.clear();
    }

    pub fn len(&self) -> usize {
        self.material.len()
    }

    pub fn is_empty(&self) -> bool {
        self.material.is_empty()
    }
}

#[derive(Clone, Default)]
pub(crate) struct LayerBuffer {
    pub material: Vec<Instance>,
    pub rebuild: bool,
    pub dirty: Option<(usize, usize)>,
    pub revision: u64,
}

impl LayerBuffer {
    fn replace(&mut self, start: usize, instances: &[Instance]) {
        if instances.is_empty() {
            return;
        }
        self.material[start..start + instances.len()].copy_from_slice(instances);
    }

    pub(crate) fn patch(&mut self, start: usize, instances: &[Instance]) {
        if instances.is_empty() {
            return;
        }
        self.replace(start, instances);
        self.mark(start, instances.len());
        self.touch();
    }

    fn mark(&mut self, start: usize, len: usize) {
        if len == 0 {
            return;
        }
        let end = start + len;
        self.dirty = Some(match self.dirty {
            Some((low, high)) => (low.min(start), high.max(end)),
            None => (start, end),
        });
    }

    pub(crate) fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn assign(&mut self, instances: &[Instance]) {
        self.material.clear();
        self.material.extend_from_slice(instances);
        self.dirty = Some((0, self.material.len()));
        self.touch();
    }
}

pub struct Painter<'a> {
    instances: &'a mut Instances,
    text: &'a mut Text,
    images: &'a mut ImageAtlas,
    clip: Clip,
    gamma: f32,
    marks: Option<&'a mut Vec<Mark>>,
}

impl<'a> Painter<'a> {
    pub(crate) fn new(
        instances: &'a mut Instances,
        text: &'a mut Text,
        images: &'a mut ImageAtlas,
        clip: Clip,
        marks: Option<&'a mut Vec<Mark>>,
    ) -> Self {
        Self {
            instances,
            text,
            images,
            clip,
            gamma: 0.86,
            marks,
        }
    }

    fn mark(&mut self, mark: Mark) {
        if let Some(marks) = self.marks.as_deref_mut() {
            marks.push(mark);
        }
    }

    pub fn clip(&self) -> Clip {
        self.clip
    }

    pub fn recording(&self) -> bool {
        self.marks.is_some()
    }

    pub fn push_clip(&mut self, rect: Rect, corners: Corners) {
        self.clip = self.clip.intersect(rect, corners);
    }

    pub fn pop_clip(&mut self, previous: Clip) {
        self.clip = previous;
    }

    fn push(&mut self, instance: Instance) {
        let clip = self.clip;
        if !instance.touches(clip.rect) {
            return;
        }
        self.instances.material.push(clipped(instance, clip));
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
        let clip = self.clip.rect;
        self.mark(Mark::Rect {
            rect,
            corners,
            color,
            clip,
        });
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
        let clip = self.clip.rect;
        self.mark(Mark::Gradient {
            rect,
            corners,
            top,
            bottom,
            clip,
        });
    }

    pub fn surface(&mut self, rect: Rect, surface: Surface) {
        self.outlined(
            rect,
            surface.corners,
            surface.fill,
            surface.border,
            surface.shadow,
        );
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
            let clip = self.clip.rect;
            self.mark(Mark::Border {
                rect,
                corners,
                width: border.width,
                color: border.color,
                clip,
            });
        }
    }

    pub fn shadow(&mut self, rect: Rect, corners: Corners, shadow: Shadow) {
        if rect.is_empty() || shadow.color.alpha <= 0.0 || shadow.radius <= 0.0 {
            return;
        }
        let mut instance = Instance::blank(KIND_SHADOW);
        let sigma = (shadow.radius * 0.5).max(0.5);
        let expanded = rect.translate(shadow.offset).expand(2.6 * sigma);
        instance.rect = [
            expanded.min.x,
            expanded.min.y,
            expanded.width(),
            expanded.height(),
        ];
        instance.corners = corners_array(corners.resolved(rect.size()));
        instance.color = shadow.color.to_array();
        instance.color2 = corners_array(corners.resolved(rect.size()));
        instance.uv = [rect.min.x, rect.min.y, rect.width(), rect.height()];
        instance.extra = [sigma, 0.0, 0.0, 0.0];
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

    pub fn bitmap(&mut self, rect: Rect, corners: Corners, bitmap: &Bitmap) {
        if rect.is_empty() || !self.clip.rect.intersects(rect) {
            return;
        }
        let clip = self.clip.rect;
        self.mark(Mark::Bitmap {
            rect,
            bitmap: bitmap.clone(),
            clip,
        });
        let Some(uv) = self.images.slot(bitmap) else {
            return;
        };
        let mut instance = Instance::blank(KIND_IMAGE);
        instance.rect = [rect.min.x, rect.min.y, rect.width(), rect.height()];
        instance.corners = corners_array(corners.resolved(rect.size()));
        instance.uv = uv;
        self.push(instance);
    }

    pub fn contour(&mut self, points: &[Vec2], color: Color) {
        assert!(self.marks.is_some(), "a contour fill records a vector page");
        if points.len() < 3 || color.alpha <= 0.0 {
            return;
        }
        let mut contour = Contour::new(points[0].to_array());
        for point in &points[1..] {
            contour.push(Curve::Line(point.to_array()));
        }
        let clip = self.clip.rect;
        self.mark(Mark::Outline {
            contours: vec![contour],
            color,
            clip,
        });
    }

    pub fn triangle(&mut self, a: Vec2, b: Vec2, c: Vec2, color: Color) {
        if color.alpha <= 0.0 {
            return;
        }
        let min = a.min(b).min(c);
        let max = a.max(b).max(c);
        let rect = Rect::from_min_max(min, max);
        if rect.is_empty() {
            return;
        }
        let mut instance = Instance::blank(KIND_TRIANGLE);
        instance.rect = [rect.min.x, rect.min.y, rect.width(), rect.height()];
        instance.uv = [a.x, a.y, b.x, b.y];
        instance.extra = [c.x, c.y, 0.0, 0.0];
        instance.color = color.to_array();
        self.push(instance);
        let clip = self.clip.rect;
        self.mark(Mark::Triangle {
            corners: [a.to_array(), b.to_array(), c.to_array()],
            color,
            clip,
        });
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
        let clip = self.clip.rect;
        self.mark(Mark::Segment {
            from: from.to_array(),
            to: to.to_array(),
            width,
            start,
            end,
            clip,
        });
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
        let clip = self.clip.rect;
        self.mark(Mark::Arc {
            center: center.to_array(),
            radius,
            width,
            start,
            sweep,
            color,
            clip,
        });
    }

    pub fn glyphs(&mut self, layout: &TextLayout, origin: Vec2, color: Color) -> Option<Rect> {
        let Self {
            instances,
            text,
            clip,
            gamma,
            ..
        } = self;
        let clip = *clip;
        let gamma = *gamma;
        let mut ink: Option<Rect> = None;
        text.emit(layout, origin, color, |plot| {
            let rect = Rect::from_min_size(
                Vec2::new(plot.left, plot.top),
                Vec2::new(plot.width, plot.height),
            );
            ink = Some(ink.map_or(rect, |held| held.union(rect)));
            let mut instance = Instance::blank(KIND_GLYPH);
            instance.rect = [plot.left, plot.top, plot.width, plot.height];
            instance.color = plot.color.to_array();
            instance.uv = plot.slot.uv;
            instance.extra = [gamma, 0.0, 0.0, 0.0];
            if instance.touches(clip.rect) {
                instances.material.push(clipped(instance, clip));
            }
        });
        ink
    }

    pub fn text(&mut self, layout: &TextLayout, position: Vec2, color: Color) {
        self.glyphs(layout, position, color);
        self.record_text(layout, position, color);
    }

    pub fn layout(
        &mut self,
        text: &str,
        style: TextStyle,
        max_width: f32,
    ) -> std::sync::Arc<TextLayout> {
        self.text.layout(
            text,
            style.font,
            max_width,
            style.wrap,
            style.truncate,
            style.align,
        )
    }

    pub fn measure(&mut self, text: &str, style: TextStyle) -> Vec2 {
        self.text
            .layout(text, style.font, 0.0, false, style.truncate, style.align)
            .size
    }

    pub fn text_at(&mut self, position: Vec2, text: &str, style: TextStyle) {
        let layout = self
            .text
            .layout(text, style.font, 0.0, false, style.truncate, style.align);
        self.glyphs(&layout, position, style.color);
        self.record_text(&layout, position, style.color);
    }

    pub fn text_in(&mut self, rect: Rect, text: &str, style: TextStyle, vertical: Align) {
        let layout = self.text.layout(
            text,
            style.font,
            rect.width(),
            style.wrap,
            style.truncate,
            style.align,
        );
        let factor = match vertical {
            Align::Start | Align::Baseline => 0.0,
            Align::Center => 0.5,
            Align::End => 1.0,
        };
        let origin = Vec2::new(
            rect.min.x,
            rect.min.y + (rect.height() - layout.size.y) * factor,
        );
        let start = self.instances.material.len();
        let Some(ink) = self.glyphs(&layout, origin, style.color) else {
            self.record_text(&layout, origin, style.color);
            return;
        };
        let mut delta = Vec2::ZERO;
        if matches!(style.align, Align::Center | Align::End) {
            let target = if style.align == Align::Center {
                rect.center().x
            } else {
                rect.max.x
            };
            let current = if style.align == Align::Center {
                ink.center().x
            } else {
                ink.max.x
            };
            delta.x = target - current;
        }
        if factor > 0.0 {
            let target = rect.min.y + rect.height() * factor;
            let current = ink.min.y + ink.height() * factor;
            delta.y = target - current;
        }
        let pixels_per_point = self.text.pixels_per_point();
        delta = (delta * pixels_per_point).round() / pixels_per_point;
        if delta != Vec2::ZERO {
            for instance in &mut self.instances.material[start..] {
                instance.rect[0] += delta.x;
                instance.rect[1] += delta.y;
            }
        }
        self.record_text(&layout, origin + delta, style.color);
    }

    fn record_text(&mut self, layout: &TextLayout, origin: Vec2, color: Color) {
        if self.marks.is_none() || color.alpha <= 0.0 {
            return;
        }
        let contours = self.text.contours(layout, origin);
        if contours.is_empty() {
            return;
        }
        let clip = self.clip.rect;
        self.mark(Mark::Outline {
            contours,
            color,
            clip,
        });
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

pub(crate) const DAMAGE_REGIONS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Regions {
    rects: [Rect; DAMAGE_REGIONS],
    len: usize,
}

impl Regions {
    const EMPTY: Self = Self {
        rects: [Rect::ZERO; DAMAGE_REGIONS],
        len: 0,
    };

    pub fn iter(&self) -> impl Iterator<Item = Rect> + '_ {
        self.rects[..self.len].iter().copied()
    }

    fn insert(&mut self, rect: Rect) {
        let mut pending = rect;
        let mut index = 0;
        while index < self.len {
            let held = self.rects[index];
            if held.contains_rect(pending) {
                return;
            }
            if pending.contains_rect(held) || held.intersects(pending) {
                pending = pending.union(held);
                self.len -= 1;
                self.rects[index] = self.rects[self.len];
                index = 0;
                continue;
            }
            index += 1;
        }
        if self.len == DAMAGE_REGIONS {
            let bounds = self
                .rects
                .iter()
                .copied()
                .reduce(Rect::union)
                .expect("a filled damage region holds a rectangle");
            self.len = 1;
            self.rects[0] = bounds.union(pending);
        } else {
            self.rects[self.len] = pending;
            self.len += 1;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Damage {
    #[default]
    None,
    Regions(Regions),
    Full,
}

impl Damage {
    pub fn add(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        match self {
            Self::Full => {}
            Self::None => {
                let mut regions = Regions::EMPTY;
                regions.insert(rect);
                *self = Self::Regions(regions);
            }
            Self::Regions(regions) => regions.insert(rect),
        }
    }

    pub fn full(&mut self) {
        *self = Self::Full;
    }
}

pub(crate) fn extent(instances: &[Instance]) -> Option<Rect> {
    instances
        .iter()
        .map(bounds)
        .reduce(|held, rect| held.union(rect))
        .map(|rect| rect.expand(1.0))
}

fn bounds(instance: &Instance) -> Rect {
    match instance.meta[0] {
        KIND_SEGMENT => {
            let width = 0.5 * instance.extra[0].max(0.0);
            Rect::from_min_max(
                Vec2::new(instance.rect[0], instance.rect[1])
                    .min(Vec2::new(instance.rect[2], instance.rect[3]))
                    - Vec2::splat(width),
                Vec2::new(instance.rect[0], instance.rect[1])
                    .max(Vec2::new(instance.rect[2], instance.rect[3]))
                    + Vec2::splat(width),
            )
        }
        KIND_RING => {
            let center = Vec2::new(instance.rect[0], instance.rect[1]);
            let radius = instance.rect[2].max(0.0) + instance.rect[3].max(0.0);
            Rect::from_min_max(center - Vec2::splat(radius), center + Vec2::splat(radius))
        }
        KIND_TRIANGLE => {
            let a = Vec2::new(instance.uv[0], instance.uv[1]);
            let b = Vec2::new(instance.uv[2], instance.uv[3]);
            let c = Vec2::new(instance.extra[0], instance.extra[1]);
            Rect::from_min_max(a.min(b).min(c), a.max(b).max(c))
        }
        _ => {
            let border = if (instance.meta[1] & FLAG_BORDER) != 0 {
                instance.extra[0].max(0.0)
            } else {
                0.0
            };
            Rect::from_min_size(
                Vec2::new(instance.rect[0], instance.rect[1]),
                Vec2::new(instance.rect[2], instance.rect[3]),
            )
            .expand(border)
        }
    }
}

fn clipped(mut instance: Instance, clip: Clip) -> Instance {
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
    instance
}
