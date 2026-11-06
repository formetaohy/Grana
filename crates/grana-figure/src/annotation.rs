use glam::{Mat4, Vec2, Vec3};
use grana_measure::Unit;
use grana_scene::Scene;
use grana_ui::{Border, Color, Corners, Fill, Painter, Rect, Shadow, TextStyle, TypeStyle};
use grana_volume::Axis;

const POINT_RADIUS: f32 = 3.2;
const CHIP_GAP: f32 = 8.0;
const CHIP_RISE: f32 = 12.0;
const TEXT_POINTS: f32 = 10.5;
const MARGIN: f32 = 8.0;
const ACCENT: Color = Color::rgba(0.56, 0.86, 1.0, 0.96);
const SELECTED: Color = Color::rgba(1.0, 0.86, 0.45, 1.0);

pub struct Item {
    pub anchor: [f32; 3],
    pub end: Option<[f32; 3]>,
    pub radius: Option<f32>,
    pub text: String,
    pub selected: bool,
}

pub struct Placed {
    pub at: Vec2,
    pub to: Option<Vec2>,
    pub reach: Option<f32>,
    pub text: String,
    pub selected: bool,
}

pub fn collect(scene: &Scene) -> Vec<Item> {
    let selected = scene.annotations.selected();
    scene
        .annotations
        .iter()
        .enumerate()
        .filter(|(_, annotation)| annotation.visible())
        .map(|(index, annotation)| {
            let kind = annotation.kind();
            Item {
                anchor: kind.anchor(),
                end: kind.end(),
                radius: kind.radius(),
                text: label(annotation.name(), kind.length(), kind.radius()),
                selected: selected == Some(index),
            }
        })
        .collect()
}

fn label(name: &str, length: Option<f32>, radius: Option<f32>) -> String {
    match (length, radius) {
        (Some(length), _) => format!("{name} · {}", crate::value(length, Unit::Micrometer, 2)),
        (None, Some(radius)) => format!("{name} · r {}", crate::value(radius, Unit::Micrometer, 2)),
        (None, None) => name.to_string(),
    }
}

pub fn volume(
    items: &[Item],
    view_projection: Mat4,
    world_per_point: f32,
    rect: Rect,
) -> Vec<Placed> {
    items
        .iter()
        .filter_map(|item| {
            let at = project(view_projection, Vec3::from(item.anchor), rect)?;
            let to = match item.end {
                Some(end) => Some(project(view_projection, Vec3::from(end), rect)?),
                None => None,
            };
            let reach = item.radius.map(|radius| radius / world_per_point);
            Some(placed(item, at, to, reach))
        })
        .collect()
}

pub fn section(
    items: &[Item],
    focus: Vec3,
    axis: Axis,
    span: f32,
    rect: Rect,
    tolerance: f32,
) -> Vec<Placed> {
    let (u, v) = axis.in_plane();
    let plane = focus[axis.index()];
    let points_per_micron = rect.height() / span;
    let screen = |point: [f32; 3]| {
        Vec2::new(
            rect.center().x + (point[u.index()] - focus[u.index()]) * points_per_micron,
            rect.center().y - (point[v.index()] - focus[v.index()]) * points_per_micron,
        )
    };
    items
        .iter()
        .filter_map(|item| {
            let depth = (item.anchor[axis.index()] - plane).abs();
            let reach = match item.radius {
                Some(radius) => {
                    if depth >= radius {
                        return None;
                    }
                    Some((radius * radius - depth * depth).sqrt() * points_per_micron)
                }
                None => {
                    if depth > tolerance {
                        return None;
                    }
                    None
                }
            };
            let to = match item.end {
                Some(end) => {
                    if (end[axis.index()] - plane).abs() > tolerance {
                        return None;
                    }
                    Some(screen(end))
                }
                None => None,
            };
            Some(placed(item, screen(item.anchor), to, reach))
        })
        .collect()
}

fn placed(item: &Item, at: Vec2, to: Option<Vec2>, reach: Option<f32>) -> Placed {
    Placed {
        at,
        to,
        reach,
        text: item.text.clone(),
        selected: item.selected,
    }
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
        match (item.to, item.reach) {
            (Some(to), _) => {
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
            (None, Some(reach)) if reach >= 1.0 => {
                painter.ring(item.at, reach, 1.2, 0.0, std::f32::consts::TAU, color);
                dot(painter, item.at, color);
                chip(painter, item.at, &item.text, color, &mut occupied);
            }
            _ => {
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
