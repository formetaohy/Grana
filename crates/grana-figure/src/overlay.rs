use std::sync::Arc;

use glam::{Mat4, Vec2, Vec3};
use grana_display::{Camera, Palette, SectionCamera, plane_basis};
use grana_measure::Unit;
use grana_scene::{ObjectColor, Scene, TrackColor};
use grana_ui::{
    Align, Color, Corners, Fill, Painter, Rect, Text, TextLayout, TextStyle, TypeStyle,
};
use grana_volume::{Axis, Clip, Region};

use crate::annotation::{self, Item as AnnotationItem};
use crate::labels::{self, Label};
use crate::value;

const BAR: Color = Color::rgba(1.0, 1.0, 1.0, 0.90);
const PLANE: Color = Color::rgba(1.0, 1.0, 1.0, 0.22);
const LABEL: Color = Color::rgba(1.0, 1.0, 1.0, 0.82);
const REGION: Color = Color::rgba(0.98, 0.75, 0.29, 0.40);
const CUT: Color = Color::rgba(1.00, 0.48, 0.44, 0.60);
const HANDLE_SIDE: f32 = 9.0;
const HANDLE_MINIMUM: f32 = 1.0e-3;
const SCALE_TARGET: f32 = 104.0;
const LEGEND_WIDTH: f32 = 176.0;
const LEGEND_BAR_WIDTH: f32 = 7.0;
const LEGEND_BAR_HEIGHT: f32 = 72.0;
const LEGEND_MARGIN: f32 = 14.0;
const LEGEND_SAMPLES: usize = 48;
const CAPTION_WIDTH: f32 = 340.0;
const CAPTION_MARGIN: f32 = 16.0;
const CAPTION_TITLE: Color = Color::rgba(1.0, 1.0, 1.0, 0.95);
const CAPTION_DETAIL: Color = Color::rgba(1.0, 1.0, 1.0, 0.60);
const CAPTION_LINE: Color = Color::rgba(1.0, 1.0, 1.0, 0.82);

const REGION_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (1, 3),
    (3, 2),
    (2, 0),
    (4, 5),
    (5, 7),
    (7, 6),
    (6, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

pub struct Legend {
    pub title: String,
    pub palette: Palette,
    pub reversed: bool,
    pub extent: (f32, f32),
    pub unit: Unit,
    pub decimals: u8,
}

pub struct Caption {
    pub title: String,
    pub detail: String,
    pub lines: Vec<String>,
}

pub(crate) struct Placed {
    title: Arc<TextLayout>,
    detail: Arc<TextLayout>,
    lines: Vec<Arc<TextLayout>>,
    width: f32,
    height: f32,
}

impl Placed {
    pub(crate) fn new(text: &mut Text, caption: &Caption, rect: Rect) -> Self {
        let title = text.layout(
            &caption.title,
            title_style().font,
            CAPTION_WIDTH,
            true,
            false,
            Align::Start,
        );
        let detail = text.layout(
            &caption.detail,
            detail_style().font,
            CAPTION_WIDTH,
            true,
            false,
            Align::Start,
        );
        let mut lines = caption
            .lines
            .iter()
            .map(|line| {
                text.layout(
                    line,
                    line_style().font,
                    CAPTION_WIDTH,
                    true,
                    false,
                    Align::Start,
                )
            })
            .collect::<Vec<_>>();
        let room = (rect.height() - CAPTION_MARGIN - 34.0).max(0.0);
        let mut height = height_of(&title, &detail, &lines);
        while !lines.is_empty() && height > room {
            lines.pop();
            height = height_of(&title, &detail, &lines);
        }
        let width = lines
            .iter()
            .fold(title.size.x.max(detail.size.x), |width, line| {
                width.max(line.size.x)
            });
        Self {
            title,
            detail,
            lines,
            width,
            height,
        }
    }
}

fn height_of(title: &TextLayout, detail: &TextLayout, lines: &[Arc<TextLayout>]) -> f32 {
    title.size.y
        + detail.size.y
        + lines.iter().map(|line| line.size.y).sum::<f32>()
        + 4.0
        + 9.0
        + lines.len() as f32 * 3.0
}

pub struct Overlay<'a> {
    pub camera: &'a Camera,
    pub section: &'a SectionCamera,
    pub world: Vec3,
    pub volume_safe: Rect,
    pub section_safe: Rect,
    pub section_stage: Rect,
    pub volume_rect: Rect,
    pub section_rect: Rect,
    pub region: &'a Region,
    pub clip: Clip,
    pub legends: &'a [Legend],
    pub labels: &'a [Label],
    pub annotations: &'a [AnnotationItem],
    pub pending: Option<[f32; 3]>,
    pub voxel: Vec3,
    pub recording: Option<(u32, u32)>,
}

pub struct Handle {
    pub axis: usize,
    pub side: usize,
    pub rect: Rect,
    pub screen: Vec2,
    pub world_per_point: f32,
}

pub fn paint(painter: &mut Painter, overlay: &Overlay<'_>) {
    paint_volume(painter, overlay);
    paint_section(painter, overlay);
}

fn paint_volume(painter: &mut Painter, overlay: &Overlay<'_>) {
    let rect = overlay.volume_rect;
    let aspect = rect.width() / rect.height();
    let view_projection = overlay
        .camera
        .view_projection(aspect, 0.5 * overlay.world.length());
    let corners = plane_corners(
        overlay.camera.focus(),
        overlay.section.axis(),
        overlay.world,
    );
    let projected = corners.map(|corner| project(view_projection, corner, rect));
    let clip = painter.clip();
    painter.push_clip(painter.clip().rect.intersect(rect), Corners::ZERO);
    if let [Some(a), Some(b), Some(c), Some(d)] = projected {
        for (from, to) in [(a, b), (b, c), (c, d), (d, a)] {
            painter.segment(from, to, 1.0, PLANE);
        }
    }
    region_box(painter, view_projection, *overlay.region, rect);
    clip_plane(painter, view_projection, overlay.clip, overlay.world, rect);
    let world_per_point =
        2.0 * overlay.camera.distance() * (overlay.camera.fov_y() * 0.5).tan() / rect.height();
    figure(
        painter,
        overlay.volume_safe,
        world_per_point,
        overlay.legends,
        None,
        true,
    );
    if !overlay.labels.is_empty() {
        let placed =
            labels::volume_labels(overlay.labels, view_projection, overlay.camera.eye(), rect);
        labels::paint(painter, rect, &placed);
    }
    let placed = annotation::volume(overlay.annotations, view_projection, world_per_point, rect);
    annotation::paint(painter, rect, &placed);
    if let Some(pending) = overlay.pending {
        annotation::pending(painter, rect, view_projection, rect, pending);
    }
    painter.pop_clip(clip);
}

pub fn handles(overlay: &Overlay<'_>) -> Vec<Handle> {
    let rect = overlay.volume_rect;
    let aspect = rect.width() / rect.height();
    let view_projection = overlay
        .camera
        .view_projection(aspect, 0.5 * overlay.world.length());
    let minimum = (HANDLE_MINIMUM * overlay.world.min_element()).max(1.0e-5);
    let low = overlay.region.min();
    let high = overlay.region.max();
    let mut handles = Vec::new();
    if !overlay.region.enabled() {
        return handles;
    }
    for axis in 0..3 {
        let step = (0.05 * overlay.world[axis]).max(minimum);
        for side in 0..2 {
            let mut point = 0.5 * (low + high);
            point[axis] = if side == 0 { low[axis] } else { high[axis] };
            let Some(anchor) = project(view_projection, point, rect) else {
                continue;
            };
            let mut direction = Vec3::ZERO;
            direction[axis] = step;
            let Some(ahead) = project(view_projection, point + direction, rect) else {
                continue;
            };
            let screen = ahead - anchor;
            let length = screen.length();
            if length < 1.0e-3 {
                continue;
            }
            handles.push(Handle {
                axis,
                side,
                rect: Rect::from_center_size(anchor, Vec2::splat(HANDLE_SIDE)),
                screen,
                world_per_point: step / length,
            });
        }
    }
    handles
}

fn paint_section(painter: &mut Painter, overlay: &Overlay<'_>) {
    let rect = overlay.section_rect;
    let stage = overlay.section_stage;
    let focus = overlay.camera.focus();
    let world_per_point = overlay.section.span() / rect.height();
    let clip = painter.clip();
    painter.push_clip(painter.clip().rect.intersect(rect), Corners::ZERO);
    painter.outlined(
        stage,
        Corners::all(2.0),
        Fill::None,
        grana_ui::Border {
            width: 1.0,
            color: Color::white_alpha(0.12),
        },
        grana_ui::Shadow::NONE,
    );
    scale_bar(painter, overlay.section_safe, world_per_point, true);
    if !overlay.labels.is_empty() {
        let placed = labels::section_labels(
            overlay.labels,
            focus,
            overlay.section.axis(),
            overlay.section.span(),
            rect,
        );
        labels::paint(painter, rect, &placed);
    }
    let tolerance = 1.5 * overlay.voxel[overlay.section.axis().index()];
    let placed = annotation::section(
        overlay.annotations,
        focus,
        overlay.section.axis(),
        overlay.section.span(),
        rect,
        tolerance,
    );
    annotation::paint(painter, rect, &placed);
    painter.pop_clip(clip);
}
pub(crate) fn figure(
    painter: &mut Painter,
    rect: Rect,
    world_per_point: f32,
    legends: &[Legend],
    caption: Option<&Placed>,
    backdrop: bool,
) {
    scale_bar(painter, rect, world_per_point, backdrop);
    paint_caption(painter, rect, caption, backdrop);
    paint_legends(painter, rect, legends, backdrop);
}

pub fn caption(scene: &Scene, time: u32) -> Caption {
    let frames = scene.frame_count();
    let detail = if frames > 1 {
        let seconds = time as f32 * scene.dataset.frame_interval();
        format!("frame {} of {frames} · {seconds:.2} s", time + 1)
    } else {
        "single frame".to_string()
    };
    let lines = scene
        .objects
        .iter()
        .filter(|set| scene.shown(set) && set.detected() > 0)
        .map(|set| {
            let count = set.frame(time).map_or(0, |frame| frame.object_count());
            format!("{} · {count} {}", set.name(), set.kind().count_noun())
        })
        .collect();
    Caption {
        title: scene.dataset.name().to_string(),
        detail,
        lines,
    }
}

fn paint_caption(painter: &mut Painter, rect: Rect, caption: Option<&Placed>, backdrop: bool) {
    let Some(placed) = caption else {
        return;
    };
    if rect.width() < 160.0 || rect.height() < 72.0 {
        return;
    }
    let origin = rect.min + Vec2::splat(CAPTION_MARGIN);
    if backdrop {
        let card = Rect::from_min_size(
            origin - Vec2::new(10.0, 8.0),
            Vec2::new(placed.width + 20.0, placed.height + 14.0),
        );
        painter.outlined(
            card,
            Corners::all(6.0),
            Fill::Solid(Color::black_alpha(0.45)),
            grana_ui::Border {
                width: 0.5,
                color: Color::white_alpha(0.10),
            },
            grana_ui::Shadow::NONE,
        );
    }
    let mut top = origin.y;
    painter.text(
        placed.title.as_ref(),
        Vec2::new(origin.x, top),
        CAPTION_TITLE,
    );
    top += placed.title.size.y + 2.0;
    painter.text(
        placed.detail.as_ref(),
        Vec2::new(origin.x, top),
        CAPTION_DETAIL,
    );
    top += placed.detail.size.y + 9.0;
    for line in &placed.lines {
        painter.text(line.as_ref(), Vec2::new(origin.x, top), CAPTION_LINE);
        top += line.size.y + 3.0;
    }
}

pub fn legends(scene: &Scene) -> Vec<Legend> {
    let names = scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let mut legends = Vec::new();
    for set in scene.objects.iter() {
        if !scene.shown(set) {
            continue;
        }
        if set.detected() > 0
            && let Some(metric) = set.object_coloring().metric()
            && let Some(extent) = set.object_extent()
        {
            legends.push(Legend {
                title: format!("{} · {}", set.name(), metric_label(scene, metric, &names)),
                palette: set.object_coloring().palette(),
                reversed: set.object_coloring().reversed(),
                extent,
                unit: metric.unit(),
                decimals: metric.decimals(),
            });
        }
        if let Some(metric) = set.track_coloring().metric()
            && set.tracks().is_some_and(|layer| layer.visible())
            && let Some(extent) = set.track_extent()
        {
            legends.push(Legend {
                title: format!("{} tracks · {}", set.name(), track_label(scene, metric)),
                palette: set.track_coloring().palette(),
                reversed: set.track_coloring().reversed(),
                extent,
                unit: metric.unit(),
                decimals: metric.decimals(),
            });
        }
    }
    legends
}

fn metric_label(scene: &Scene, metric: ObjectColor, channels: &[String]) -> String {
    match metric {
        ObjectColor::Measurement(measurement) => measurement.label(channels),
        ObjectColor::Relation {
            target,
            field,
            shift,
        } => {
            let target = scene
                .objects
                .get(target)
                .map_or_else(|| "target".to_string(), |set| set.name().to_string());
            let offset = if shift == 0 {
                String::new()
            } else {
                format!(" · t{shift:+}")
            };
            format!("{target}{offset} · {}", field.label())
        }
        ObjectColor::Child { target, field } => {
            let target = scene
                .objects
                .get(target)
                .map_or_else(|| "children".to_string(), |set| set.name().to_string());
            format!("{target} · {}", field.label(channels))
        }
        ObjectColor::Score => grana_measure::SCORE_LABEL.to_string(),
    }
}

fn track_label(scene: &Scene, metric: TrackColor) -> String {
    match metric {
        TrackColor::Measurement(measurement) => measurement.label().to_string(),
        TrackColor::Relation {
            target,
            field,
            aggregate,
            shift,
        } => {
            let target = scene
                .objects
                .get(target)
                .map_or_else(|| "target".to_string(), |set| set.name().to_string());
            let offset = if shift == 0 {
                String::new()
            } else {
                format!(" · t{shift:+}")
            };
            format!("{target}{offset} {} · {}", field.label(), aggregate.name())
        }
    }
}

fn region_box(painter: &mut Painter, view_projection: Mat4, region: Region, rect: Rect) {
    if !region.enabled() {
        return;
    }
    let min = region.min();
    let max = region.max();
    let corners: [Vec3; 8] = std::array::from_fn(|index: usize| {
        Vec3::new(
            if index & 1 == 0 { min.x } else { max.x },
            if index & 2 == 0 { min.y } else { max.y },
            if index & 4 == 0 { min.z } else { max.z },
        )
    });
    let projected = corners.map(|corner| project(view_projection, corner, rect));
    for (from, to) in REGION_EDGES {
        if let (Some(from), Some(to)) = (projected[from], projected[to]) {
            painter.segment(from, to, 1.0, REGION);
        }
    }
}

fn clip_plane(painter: &mut Painter, view_projection: Mat4, clip: Clip, world: Vec3, rect: Rect) {
    if !clip.enabled() {
        return;
    }
    let normal = clip.normal();
    let guide = if normal.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let across = normal.cross(guide).normalize();
    let down = normal.cross(across).normalize();
    let centre = normal * clip.offset();
    let reach = world.length();
    let corners = vec![
        centre - across * reach - down * reach,
        centre + across * reach - down * reach,
        centre + across * reach + down * reach,
        centre - across * reach + down * reach,
    ];
    let half = 0.5 * world;
    let mut polygon = corners;
    for axis in 0..3 {
        let mut direction = Vec3::ZERO;
        direction[axis] = 1.0;
        polygon = clipped(polygon, direction, half[axis]);
        polygon = clipped(polygon, -direction, half[axis]);
    }
    if polygon.len() < 3 {
        return;
    }
    let projected = polygon
        .iter()
        .map(|point| project(view_projection, *point, rect))
        .collect::<Vec<_>>();
    for (index, from) in projected.iter().enumerate() {
        let to = projected[(index + 1) % projected.len()];
        if let (Some(from), Some(to)) = (from, to) {
            painter.segment(*from, to, 1.0, CUT);
        }
    }
}

fn clipped(points: Vec<Vec3>, normal: Vec3, offset: f32) -> Vec<Vec3> {
    let mut kept = Vec::with_capacity(points.len() + 1);
    for (index, point) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        let here = normal.dot(*point) - offset;
        let there = normal.dot(next) - offset;
        if here <= 0.0 {
            kept.push(*point);
        }
        if (here < 0.0) != (there < 0.0) {
            kept.push(*point + (next - *point) * (here / (here - there)));
        }
    }
    kept
}

fn plane_corners(focus: Vec3, axis: Axis, world: Vec3) -> [Vec3; 4] {
    let (u, v) = axis.in_plane();
    let (right, up) = plane_basis(axis);
    let half_u = 0.5 * world[u.index()];
    let half_v = 0.5 * world[v.index()];
    let mut center = Vec3::ZERO;
    center[axis.index()] = focus[axis.index()];
    [
        center - right * half_u - up * half_v,
        center + right * half_u - up * half_v,
        center + right * half_u + up * half_v,
        center - right * half_u + up * half_v,
    ]
}

fn project(view_projection: Mat4, point: Vec3, rect: Rect) -> Option<Vec2> {
    let clip = view_projection * point.extend(1.0);
    if clip.w <= 1.0e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(Vec2::new(
        rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width(),
        rect.min.y + (0.5 - ndc.y * 0.5) * rect.height(),
    ))
}

fn caption_style() -> TextStyle {
    TextStyle::new(TypeStyle::new(11.0, 400.0, 14.0), LABEL)
}

fn title_style() -> TextStyle {
    TextStyle::new(TypeStyle::new(13.0, 600.0, 17.0), CAPTION_TITLE)
}

fn detail_style() -> TextStyle {
    TextStyle::new(TypeStyle::new(10.5, 400.0, 13.0), CAPTION_DETAIL)
}

fn line_style() -> TextStyle {
    TextStyle::new(TypeStyle::new(10.5, 400.0, 14.0), CAPTION_LINE)
}

fn scale_bar(painter: &mut Painter, rect: Rect, world_per_point: f32, backdrop: bool) {
    let length = nice_length(SCALE_TARGET * world_per_point);
    let points = length / world_per_point;
    let left = rect.min.x + 14.0;
    let bottom = rect.max.y - 18.0;
    let color = BAR;
    let pill = Rect::from_min_size(
        Vec2::new(left - 10.0, bottom - 11.0),
        Vec2::new(points + 60.0, 22.0),
    );
    if backdrop {
        painter.outlined(
            pill,
            Corners::capsule(),
            Fill::Solid(Color::black_alpha(0.42)),
            grana_ui::Border {
                width: 1.0,
                color: Color::white_alpha(0.12),
            },
            grana_ui::Shadow::NONE,
        );
    }
    painter.segment(
        Vec2::new(left, bottom),
        Vec2::new(left + points, bottom),
        1.5,
        color,
    );
    painter.segment(
        Vec2::new(left, bottom - 3.0),
        Vec2::new(left, bottom + 3.0),
        1.5,
        color,
    );
    painter.segment(
        Vec2::new(left + points, bottom - 3.0),
        Vec2::new(left + points, bottom + 3.0),
        1.5,
        color,
    );
    painter.text_at(
        Vec2::new(left + points + 7.0, bottom - 7.0),
        &format_length(length),
        caption_style(),
    );
}

fn paint_legends(painter: &mut Painter, rect: Rect, legends: &[Legend], backdrop: bool) {
    if legends.is_empty() || rect.width() < 320.0 || rect.height() < 240.0 {
        return;
    }
    let left = rect.max.x - LEGEND_MARGIN - LEGEND_WIDTH;
    let mut title_top = rect.min.y + 48.0;
    for legend in legends {
        let bar_top = title_top + 15.0;
        let bar_bottom = bar_top + LEGEND_BAR_HEIGHT;
        if bar_bottom + 10.0 > rect.max.y {
            break;
        }
        let card = Rect::from_min_max(
            Vec2::new(left - 10.0, title_top - 8.0),
            Vec2::new(left + LEGEND_WIDTH, bar_bottom + 8.0),
        );
        if backdrop {
            painter.outlined(
                card,
                Corners::all(10.0),
                Fill::Solid(Color::black_alpha(0.42)),
                grana_ui::Border {
                    width: 1.0,
                    color: Color::white_alpha(0.12),
                },
                grana_ui::Shadow::NONE,
            );
        }
        painter.text_at(
            Vec2::new(left + 2.0, title_top),
            &truncated(&legend.title),
            caption_style(),
        );
        let step = LEGEND_BAR_HEIGHT / LEGEND_SAMPLES as f32;
        for index in 0..LEGEND_SAMPLES {
            let position = (index as f32 + 0.5) / LEGEND_SAMPLES as f32;
            let position = if legend.reversed {
                1.0 - position
            } else {
                position
            };
            let [red, green, blue] = legend.palette.sample(position);
            let bottom = bar_bottom - index as f32 * step;
            painter.rect(
                Rect::from_min_max(
                    Vec2::new(left + 2.0, bottom - step),
                    Vec2::new(left + 2.0 + LEGEND_BAR_WIDTH, bottom + 0.5),
                ),
                Corners::ZERO,
                Color::rgb(red, green, blue),
            );
        }
        let (minimum, maximum) = legend.extent;
        let text_left = left + LEGEND_BAR_WIDTH + 10.0;
        for (align, offset, amount) in [
            (Align::Start, 0.0, maximum),
            (Align::Center, 0.5, 0.5 * (minimum + maximum)),
            (Align::End, 1.0, minimum),
        ] {
            let height = 14.0;
            let top = bar_top + (bar_bottom - bar_top - height) * offset;
            painter.text_at(
                Vec2::new(text_left, top + 1.0),
                &value(amount, legend.unit, legend.decimals),
                TextStyle::new(TypeStyle::new(10.5, 400.0, 14.0), LABEL).aligned(align),
            );
        }
        title_top = bar_bottom + 30.0;
    }
}

fn truncated(title: &str) -> String {
    let mut characters = title.chars();
    let prefix = characters.by_ref().take(26).collect::<String>();
    if characters.next().is_some() {
        format!("{prefix}\u{2026}")
    } else {
        prefix
    }
}

fn nice_length(target: f32) -> f32 {
    let exponent = target.max(1.0e-9).log10().floor();
    let scale = 10.0f32.powf(exponent);
    let fraction = target / scale;
    let factor = if fraction >= 5.0 {
        5.0
    } else if fraction >= 2.0 {
        2.0
    } else {
        1.0
    };
    factor * scale
}

fn format_length(length: f32) -> String {
    if length >= 10.0 {
        format!("{length:.0} µm")
    } else if length >= 1.0 {
        format!("{length:.1} µm")
    } else {
        format!("{length:.3} µm")
    }
}
