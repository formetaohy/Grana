use glam::{Mat4, Vec2, Vec3};
use grana_measure::Unit;
use grana_scene::Scene;
use grana_ui::{Border, Color, Corners, Fill, Painter, Rect, Shadow, TextStyle, TypeStyle};

const POINT_RADIUS: f32 = 3.2;
const PICK_RADIUS: f32 = 10.0;
const CHIP_GAP: f32 = 8.0;
const CHIP_RISE: f32 = 12.0;
const TEXT_POINTS: f32 = 10.5;
const MARGIN: f32 = 8.0;
const ACCENT: Color = Color::rgba(0.56, 0.86, 1.0, 0.96);
const SELECTED: Color = Color::rgba(1.0, 0.86, 0.45, 1.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Anchor,
    End,
    Body,
}

pub struct Item {
    pub index: usize,
    pub anchor: [f32; 3],
    pub end: Option<[f32; 3]>,
    pub text: String,
    pub selected: bool,
}

pub struct Placed {
    pub index: usize,
    pub at: Vec2,
    pub to: Option<Vec2>,
    pub text: String,
    pub selected: bool,
}

pub fn collect(scene: &Scene, time: u32) -> Vec<Item> {
    let selected = scene.annotations.selected();
    scene
        .annotations
        .iter()
        .enumerate()
        .filter(|(_, annotation)| annotation.shown_at(time))
        .map(|(index, annotation)| {
            let kind = annotation.kind();
            Item {
                index,
                anchor: kind.anchor(),
                end: kind.end(),
                text: label(annotation.name(), kind.length()),
                selected: selected == Some(index),
            }
        })
        .collect()
}

fn label(name: &str, length: Option<f32>) -> String {
    match length {
        Some(length) => format!("{name} · {}", crate::value(length, Unit::Micrometer, 2)),
        None => name.to_string(),
    }
}

pub fn volume(items: &[Item], view_projection: Mat4, rect: Rect) -> Vec<Placed> {
    items
        .iter()
        .filter_map(|item| {
            let at = project(view_projection, Vec3::from(item.anchor), rect)?;
            let to = match item.end {
                Some(end) => Some(project(view_projection, Vec3::from(end), rect)?),
                None => None,
            };
            Some(placed(item, at, to))
        })
        .collect()
}

fn placed(item: &Item, at: Vec2, to: Option<Vec2>) -> Placed {
    Placed {
        index: item.index,
        at,
        to,
        text: item.text.clone(),
        selected: item.selected,
    }
}

pub fn pick(placed: &[Placed], point: Vec2) -> Option<(usize, Part)> {
    let mut best: Option<(f32, usize, Part)> = None;
    for item in placed {
        let handles = [
            Some((Part::Anchor, item.at)),
            item.to.map(|to| (Part::End, to)),
        ];
        for (part, at) in handles.into_iter().flatten() {
            let distance = (point - at).length();
            if distance > PICK_RADIUS || best.is_some_and(|(nearest, ..)| nearest <= distance) {
                continue;
            }
            best = Some((distance, item.index, part));
        }
    }
    if best.is_some() {
        return best.map(|(_, index, part)| (index, part));
    }
    let mut best: Option<(f32, usize)> = None;
    for item in placed {
        let Some(to) = item.to else {
            continue;
        };
        let (distance, along) = segment_distance(point, item.at, to);
        if distance > PICK_RADIUS
            || !(0.2..=0.8).contains(&along)
            || best.is_some_and(|(nearest, _)| nearest <= distance)
        {
            continue;
        }
        best = Some((distance, item.index));
    }
    best.map(|(_, index)| (index, Part::Body))
}

fn segment_distance(point: Vec2, from: Vec2, to: Vec2) -> (f32, f32) {
    let axis = to - from;
    let length = axis.length_squared();
    if length <= 1.0e-9 {
        return ((point - from).length(), 0.0);
    }
    let along = ((point - from).dot(axis) / length).clamp(0.0, 1.0);
    ((point - (from + axis * along)).length(), along)
}

pub fn paint(painter: &mut Painter, pane: Rect, placed: &[Placed]) {
    if placed.is_empty() {
        return;
    }
    let previous = painter.clip();
    painter.push_clip(pane, Corners::ZERO);
    let mut occupied: Vec<Rect> = Vec::new();
    for item in placed {
        let color = if item.selected { SELECTED } else { ACCENT };
        match item.to {
            Some(to) => {
                painter.segment(item.at, to, 1.5, color);
                dot(painter, item.at, color);
                dot(painter, to, color);
                chip(
                    painter,
                    item.at.lerp(to, 0.5),
                    &item.text,
                    color,
                    &mut occupied,
                );
            }
            None => {
                painter.ring(
                    item.at,
                    POINT_RADIUS + 2.6,
                    1.1,
                    0.0,
                    std::f32::consts::TAU,
                    color,
                );
                dot(painter, item.at, color);
                chip(painter, item.at, &item.text, color, &mut occupied);
            }
        }
    }
    painter.pop_clip(previous);
}

fn dot(painter: &mut Painter, at: Vec2, color: Color) {
    painter.rect(
        Rect::from_center_size(at, Vec2::splat(POINT_RADIUS)),
        Corners::capsule(),
        color,
    );
}

fn chip(painter: &mut Painter, at: Vec2, text: &str, accent: Color, occupied: &mut Vec<Rect>) {
    let style = TextStyle::new(
        TypeStyle::new(TEXT_POINTS, 600.0, 14.0),
        Color::rgba(1.0, 1.0, 1.0, 0.94),
    );
    let size = painter.measure(text, style);
    let mut rect = Rect::from_min_size(
        Vec2::new(at.x + CHIP_GAP, at.y - CHIP_RISE - size.y - 5.0),
        Vec2::new(size.x + 14.0, size.y + 5.0),
    );
    for _ in 0..6 {
        let Some(other) = occupied.iter().find(|other| other.intersects(rect)) else {
            break;
        };
        rect = Rect::from_min_size(Vec2::new(rect.min.x, other.max.y + 2.0), rect.size());
    }
    occupied.push(rect);
    painter.outlined(
        rect,
        Corners::all(3.5),
        Fill::Solid(Color::black_alpha(0.66)),
        Border {
            width: 0.75,
            color: Color::white_alpha(0.18),
        },
        Shadow::NONE,
    );
    painter.rect(
        Rect::from_center_size(
            Vec2::new(rect.min.x + 5.0, rect.center().y),
            Vec2::splat(2.4),
        ),
        Corners::capsule(),
        accent,
    );
    painter.text_at(Vec2::new(rect.min.x + 10.0, rect.min.y + 2.0), text, style);
    painter.segment(
        at,
        Vec2::new(rect.min.x, rect.center().y),
        1.0,
        Color::white_alpha(0.30),
    );
}

pub fn pending(painter: &mut Painter, pane: Rect, view_projection: Mat4, rect: Rect, at: [f32; 3]) {
    let Some(center) = project(view_projection, Vec3::from(at), rect) else {
        return;
    };
    let previous = painter.clip();
    painter.push_clip(pane, Corners::ZERO);
    painter.ring(center, 7.0, 1.4, 0.0, std::f32::consts::TAU, SELECTED);
    dot(painter, center, SELECTED);
    painter.pop_clip(previous);
}

fn project(view_projection: Mat4, point: Vec3, rect: Rect) -> Option<Vec2> {
    let clip = view_projection * point.extend(1.0);
    if clip.w <= 1.0e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if !(-1.2..=1.2).contains(&ndc.x) || !(-1.2..=1.2).contains(&ndc.y) {
        return None;
    }
    let at = Vec2::new(
        rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width(),
        rect.min.y + (0.5 - ndc.y * 0.5) * rect.height(),
    );
    (at.x >= rect.min.x - MARGIN
        && at.x <= rect.max.x + MARGIN
        && at.y >= rect.min.y - MARGIN
        && at.y <= rect.max.y + MARGIN)
        .then_some(at)
}
