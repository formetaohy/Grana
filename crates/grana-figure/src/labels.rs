use std::collections::HashMap;

use glam::{Mat4, Vec2, Vec3};
use grana_scene::Scene;
use grana_ui::{Color, Corners, Fill, Painter, Rect};

const CHIP_HEIGHT: f32 = 15.0;
const CHIP_PAD: f32 = 5.0;
const CHIP_GAP: f32 = 3.0;
const CHIP_RISE: f32 = 10.0;
const DOT: f32 = 2.6;
const TEXT_POINTS: f32 = 10.0;
const TEXT_WIDTH: f32 = 0.60;
const CELL_HEIGHT: f32 = 20.0;
const CELL_WIDTH: f32 = 28.0;
const LIMIT: usize = 2048;
const MARGIN: f32 = 24.0;

pub struct Label {
    pub position: [f32; 3],
    pub low: [f32; 3],
    pub high: [f32; 3],
    pub text: String,
    pub color: [f32; 3],
    pub selected: bool,
}

pub struct Placed {
    at: Vec2,
    text: String,
    color: Color,
    selected: bool,
    width: f32,
}

#[derive(Clone, Copy)]
pub struct Depth {
    eye: Vec3,
    forward: Vec3,
    orthographic: bool,
}

impl Depth {
    pub fn new(camera: &grana_display::Camera) -> Self {
        Self {
            eye: camera.eye(),
            forward: camera.forward(),
            orthographic: camera.projection().orthographic(),
        }
    }

    fn of(self, point: Vec3) -> f32 {
        if self.orthographic {
            (point - self.eye).dot(self.forward)
        } else {
            point.distance(self.eye)
        }
    }
}

pub fn labels(scene: &Scene, time: u32) -> Vec<Label> {
    let clip = scene.clip;
    let mut labels = Vec::new();
    for set in scene.objects.iter() {
        if !scene.shown(set) || !set.labels() {
            continue;
        }
        let Some(frame) = set.frame_shared(time) else {
            continue;
        };
        let metric = set.label_metric();
        let values = scene.label_values(set);
        let anchors = frame.anchors();
        for (index, anchor) in anchors.into_iter().enumerate() {
            if !set.visible_object(time, index) {
                continue;
            }
            let position = anchor.position();
            if clip.enabled() && clip.normal().dot(Vec3::from(position)) > clip.offset() {
                continue;
            }
            let bounds = anchor.bounds();
            labels.push(Label {
                position,
                low: bounds.min(),
                high: bounds.max(),
                text: text(
                    metric,
                    values
                        .as_ref()
                        .and_then(|values| values.get(index).copied()),
                    index,
                ),
                color: set.color(),
                selected: scene.object_selected(set.id(), index),
            });
        }
    }
    labels
}

fn text(metric: Option<grana_scene::ObjectColor>, value: Option<f32>, index: usize) -> String {
    let Some(metric) = metric else {
        return (index + 1).to_string();
    };
    match value {
        Some(value) if value.is_finite() => crate::value(value, metric.unit(), metric.decimals()),
        _ => "\u{2026}".to_string(),
    }
}

pub fn volume_labels(
    labels: &[Label],
    view_projection: Mat4,
    depth: Depth,
    rect: Rect,
) -> Vec<Placed> {
    let mut near = labels
        .iter()
        .filter_map(|label| {
            let point = Vec3::from(label.position);
            let at = project(view_projection, point, rect)?;
            if !inside(rect, at) {
                return None;
            }
            Some((!label.selected, depth.of(point), at, label))
        })
        .collect::<Vec<_>>();
    near.sort_unstable_by(
        |(left_selected, left, _, _), (right_selected, right, _, _)| {
            left_selected
                .cmp(right_selected)
                .then(left.total_cmp(right))
        },
    );
    place(near.into_iter().map(|(_, _, at, label)| (at, label)))
}

fn place<'a>(labels: impl Iterator<Item = (Vec2, &'a Label)>) -> Vec<Placed> {
    let mut placed = Vec::new();
    let mut cells = HashMap::<(i32, i32), ()>::new();
    for (at, label) in labels {
        if placed.len() >= LIMIT {
            break;
        }
        let width = chip_width(&label.text, label.selected);
        let rect = chip_rect(at, width, label.selected);
        let span = cell_span(rect);
        if span.iter().any(|cell| cells.contains_key(cell)) {
            continue;
        }
        for cell in span {
            cells.insert(cell, ());
        }
        placed.push(Placed {
            at,
            text: label.text.clone(),
            color: Color::rgb(label.color[0], label.color[1], label.color[2]),
            selected: label.selected,
            width,
        });
    }
    placed.reverse();
    placed
}

fn project(view_projection: Mat4, point: Vec3, rect: Rect) -> Option<Vec2> {
    let clip = view_projection * point.extend(1.0);
    if clip.w <= 1.0e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.x < -1.0 || ndc.x > 1.0 || ndc.y < -1.0 || ndc.y > 1.0 {
        return None;
    }
    Some(Vec2::new(
        rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width(),
        rect.min.y + (0.5 - ndc.y * 0.5) * rect.height(),
    ))
}

fn inside(rect: Rect, at: Vec2) -> bool {
    at.x >= rect.min.x + MARGIN
        && at.x <= rect.max.x - MARGIN
        && at.y >= rect.min.y + MARGIN
        && at.y <= rect.max.y - MARGIN
}

fn chip_width(text: &str, selected: bool) -> f32 {
    let glyphs = text.chars().count() as f32 * TEXT_POINTS * TEXT_WIDTH;
    glyphs + 2.0 * CHIP_PAD + DOT + CHIP_GAP + if selected { 2.0 } else { 0.0 }
}

fn chip_rect(at: Vec2, width: f32, selected: bool) -> Rect {
    let height = CHIP_HEIGHT + if selected { 2.0 } else { 0.0 };
    let bottom = at.y - CHIP_RISE;
    Rect::from_min_size(
        Vec2::new(at.x - 0.5 * width, bottom - height),
        Vec2::new(width, height),
    )
}

fn cell_span(rect: Rect) -> Vec<(i32, i32)> {
    let first = (
        (rect.min.x / CELL_WIDTH).floor() as i32,
        (rect.min.y / CELL_HEIGHT).floor() as i32,
    );
    let last = (
        (rect.max.x / CELL_WIDTH).floor() as i32,
        (rect.max.y / CELL_HEIGHT).floor() as i32,
    );
    let mut cells = Vec::new();
    for y in first.1..=last.1 {
        for x in first.0..=last.0 {
            cells.push((x, y));
        }
    }
    cells
}

pub fn paint(painter: &mut Painter, pane: Rect, placed: &[Placed]) {
    if placed.is_empty() {
        return;
    }
    let previous = painter.clip();
    painter.push_clip(pane, Corners::ZERO);
    for label in placed {
        let rect = chip_rect(label.at, label.width, label.selected);
        draw(painter, label, rect);
    }
    painter.pop_clip(previous);
}

fn draw(painter: &mut Painter, label: &Placed, rect: Rect) {
    let background = if label.selected {
        Color::black_alpha(0.74)
    } else {
        Color::black_alpha(0.56)
    };
    let border = if label.selected {
        Color::rgba(1.0, 0.86, 0.45, 0.55)
    } else {
        Color::white_alpha(0.14)
    };
    painter.outlined(
        rect,
        Corners::all(3.0),
        Fill::Solid(background),
        grana_ui::Border {
            width: 0.75,
            color: border,
        },
        grana_ui::Shadow::NONE,
    );
    let accent = if label.selected {
        Color::rgba(1.0, 0.86, 0.45, 1.0)
    } else {
        label.color
    };
    painter.rect(
        Rect::from_center_size(
            Vec2::new(rect.min.x + CHIP_PAD + 0.5 * DOT, rect.center().y),
            Vec2::splat(DOT),
        ),
        Corners::all(0.5 * DOT),
        accent,
    );
    let top = rect.min.y + 0.5 * (rect.height() - TEXT_POINTS * 1.32);
    painter.text_at(
        Vec2::new(rect.min.x + CHIP_PAD + DOT + CHIP_GAP, top),
        &label.text,
        grana_ui::TextStyle::new(
            grana_ui::TypeStyle::new(TEXT_POINTS, 600.0, TEXT_POINTS * 1.32),
            Color::rgba(1.0, 1.0, 1.0, 0.94),
        ),
    );
    painter.segment(
        Vec2::new(label.at.x, rect.max.y),
        label.at,
        1.0,
        Color::white_alpha(0.28),
    );
    painter.rect(
        Rect::from_center_size(label.at, Vec2::splat(DOT)),
        Corners::all(0.5 * DOT),
        accent,
    );
}
