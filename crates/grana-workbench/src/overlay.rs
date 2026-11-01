use glam::{Mat4, Vec2, Vec3};
use grana_measure::Unit;
use grana_render::{Camera, Palette, SectionCamera, plane_basis};
use grana_scene::Scene;
use grana_ui::{
    Align, Color, Corners, Fill, Icon, Painter, Rect, Role, TextStyle, TypeStyle, Ui, icon,
};
use grana_volume::{Axis, Region};

use crate::coloring;

const LABEL: Color = Color::rgba(1.0, 1.0, 1.0, 0.78);
const BAR: Color = Color::rgba(1.0, 1.0, 1.0, 0.88);
const CROSS: Color = Color::rgba(0.86, 0.90, 0.94, 0.55);
const PLANE: Color = Color::rgba(0.43, 0.75, 1.0, 0.45);
const REGION: Color = Color::rgba(1.0, 0.77, 0.36, 0.55);
const REGION_ACTIVE: Color = Color::rgba(1.0, 0.88, 0.58, 0.92);
const HANDLE_SIDE: f32 = 10.0;
const HANDLE_MINIMUM: f32 = 1.0e-3;
const SCALE_TARGET: f32 = 116.0;
const LEGEND_WIDTH: f32 = 188.0;
const LEGEND_BAR_WIDTH: f32 = 10.0;
const LEGEND_BAR_HEIGHT: f32 = 78.0;
const LEGEND_MARGIN: f32 = 16.0;
const LEGEND_SAMPLES: usize = 48;

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

pub struct Overlay<'a> {
    pub camera: &'a Camera,
    pub section: &'a SectionCamera,
    pub world: Vec3,
    pub volume_safe: Rect,
    pub section_stage: Rect,
    pub levels: usize,
    pub volume_level: usize,
    pub section_level: usize,
    pub samples: u32,
    pub volume_rect: Rect,
    pub section_rect: Rect,
    pub region: &'a mut Region,
    pub legends: &'a [Legend],
    pub recording: Option<(u32, u32)>,
}

pub fn paint(ui: &mut Ui, overlay: &mut Overlay<'_>) {
    paint_volume(ui, overlay);
    paint_section(ui, overlay);
    if let Some((frame, frames)) = overlay.recording {
        chip(
            ui,
            overlay.volume_rect,
            Vec2::new(
                overlay.volume_rect.min.x + 14.0,
                overlay.volume_rect.min.y + 58.0,
            ),
            Icon::Record,
            "REC",
            &format!("frame {frame} of {frames}"),
            Color::rgba(1.0, 0.55, 0.45, 1.0),
        );
    }
}

fn paint_volume(ui: &mut Ui, overlay: &mut Overlay<'_>) {
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
    let clip = ui.clip().intersect(rect, Corners::all(16.0));
    {
        let mut painter = ui.painter_in(clip);
        if let [Some(a), Some(b), Some(c), Some(d)] = projected {
            for (from, to) in [(a, b), (b, c), (c, d), (d, a)] {
                painter.segment(from, to, 1.0, PLANE);
            }
        }
        if let Some(focus) = project(view_projection, overlay.camera.focus(), rect) {
            cross(&mut painter, focus, 5.0, 1.0, CROSS);
        }
        region_box(&mut painter, view_projection, *overlay.region, rect);
    }
    region_handles(ui, view_projection, overlay.region, rect, overlay.world);
    let world_per_point =
        2.0 * overlay.camera.distance() * (overlay.camera.fov_y() * 0.5).tan() / rect.height();
    {
        let mut painter = ui.painter_in(clip);
        figure(
            &mut painter,
            overlay.volume_safe,
            world_per_point,
            overlay.legends,
        );
    }
    chip(
        ui,
        overlay.volume_safe,
        overlay.volume_safe.min + Vec2::new(14.0, 14.0),
        Icon::Cube,
        "3D",
        &format!(
            "Level {} of {} · {} samples",
            overlay.volume_level + 1,
            overlay.levels,
            overlay.samples
        ),
        LABEL,
    );
}

fn paint_section(ui: &mut Ui, overlay: &Overlay<'_>) {
    let rect = overlay.section_rect;
    let stage = overlay.section_stage;
    let axis = overlay.section.axis();
    let focus = overlay.camera.focus();
    let plane = focus[axis.index()] + 0.5 * overlay.world[axis.index()];
    let world_per_point = overlay.section.span() / rect.height();
    let world_per_pixel = world_per_point / ui.scale();
    let clip = ui.clip().intersect(rect, Corners::all(16.0));
    {
        let mut painter = ui.painter_in(clip);
        scale_bar(&mut painter, stage, world_per_point);
        cross(&mut painter, stage.center(), 6.0, 1.0, CROSS);
        painter.outlined(
            stage,
            Corners::all(7.0),
            Fill::None,
            grana_ui::Border {
                width: 0.75,
                color: Color::white_alpha(0.14),
            },
            grana_ui::Shadow::NONE,
        );
    }
    chip(
        ui,
        rect,
        rect.min + Vec2::new(14.0, 14.0),
        Icon::Section,
        &format!("{} · {} = {plane:.2} µm", plane_name(axis), axis.name()),
        &format!(
            "{world_per_pixel:.3} µm/px · level {} of {}",
            overlay.section_level + 1,
            overlay.levels
        ),
        LABEL,
    );
}
pub fn figure(painter: &mut Painter, rect: Rect, world_per_point: f32, legends: &[Legend]) {
    scale_bar(painter, rect, world_per_point);
    paint_legends(painter, rect, legends);
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
        if !set.visible() || !scene.channels[set.channel()].enabled {
            continue;
        }
        if set.detected() > 0
            && let Some(measurement) = set.object_coloring().metric()
            && let Some(extent) = set.object_extent()
        {
            legends.push(Legend {
                title: format!("{} · {}", set.name(), measurement.label(&names)),
                palette: set.object_coloring().palette(),
                reversed: set.object_coloring().reversed(),
                extent,
                unit: measurement.unit(),
                decimals: measurement.decimals(),
            });
        }
        if let Some(measurement) = set.track_coloring().metric()
            && set.tracks().is_some_and(|layer| layer.visible())
            && let Some(extent) = set.track_extent()
        {
            legends.push(Legend {
                title: format!("{} tracks · {}", set.name(), measurement.label()),
                palette: set.track_coloring().palette(),
                reversed: set.track_coloring().reversed(),
                extent,
                unit: measurement.unit(),
                decimals: measurement.decimals(),
            });
        }
    }
    legends
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

fn region_handles(
    ui: &mut Ui,
    view_projection: Mat4,
    region: &mut Region,
    rect: Rect,
    world: Vec3,
) {
    if !region.enabled() {
        return;
    }
    let half = 0.5 * world;
    let minimum = (HANDLE_MINIMUM * world.min_element()).max(1.0e-5);
    let mut low = region.min();
    let mut high = region.max();
    for axis in 0..3 {
        let step = (0.05 * world[axis]).max(minimum);
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
            let world_per_point = step / length;
            let handle = Rect::from_center_size(anchor, Vec2::splat(HANDLE_SIDE));
            let response = ui.interact(ui.id(&format!("region.handle.{axis}.{side}")), handle);
            if response.dragging {
                let delta = ui.input().delta().dot(screen / length) * world_per_point;
                if side == 0 {
                    low[axis] = (low[axis] + delta).clamp(-half[axis], high[axis] - minimum);
                } else {
                    high[axis] = (high[axis] + delta).clamp(low[axis] + minimum, half[axis]);
                }
            }
            let active = response.hovered || response.dragging;
            let color = if active { REGION_ACTIVE } else { REGION };
            let clip = ui.clip().intersect(rect, Corners::all(16.0));
            let mut painter = ui.painter_in(clip);
            painter.rect(handle, Corners::all(2.5), color);
        }
    }
    if low != region.min() || high != region.max() {
        region.set_bounds(low, high);
    }
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

fn cross(painter: &mut Painter, center: Vec2, radius: f32, width: f32, color: Color) {
    painter.segment(
        Vec2::new(center.x - radius, center.y),
        Vec2::new(center.x + radius, center.y),
        width,
        color,
    );
    painter.segment(
        Vec2::new(center.x, center.y - radius),
        Vec2::new(center.x, center.y + radius),
        width,
        color,
    );
}

fn caption_style() -> TextStyle {
    TextStyle::new(TypeStyle::new(11.0, 400.0, 14.0), LABEL)
}

fn chip(
    ui: &mut Ui,
    pane: Rect,
    origin: Vec2,
    symbol: Icon,
    title: &str,
    detail: &str,
    tint: Color,
) {
    let clip = ui.clip().intersect(pane, Corners::all(20.0));
    let title_style = TextStyle::new(
        TypeStyle::new(11.5, 600.0, 15.0),
        Color::rgba(1.0, 1.0, 1.0, 0.94),
    );
    let detail_style = TextStyle::new(TypeStyle::new(10.5, 400.0, 13.0), Color::white_alpha(0.56));
    let maximum = (pane.width() - 80.0).max(120.0);
    let title_layout = ui.layout_text(title, title_style, maximum);
    let detail_layout = ui.layout_text(detail, detail_style, maximum);
    let width = title_layout.size.x.max(detail_layout.size.x).min(maximum) + 52.0;
    let height = 44.0;
    let rect = Rect::from_min_size(origin, Vec2::new(width, height));
    let look = ui.theme().look(Role::Chip, grana_ui::State::Rest);
    let mut painter = ui.painter_in(clip);
    painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    icon::paint(
        symbol,
        &mut painter,
        Vec2::new(rect.min.x + 18.0, rect.center().y),
        15.0,
        tint,
        1.5,
    );
    painter.text(
        &title_layout,
        Vec2::new(rect.min.x + 33.0, rect.min.y + 8.0),
        Color::rgba(1.0, 1.0, 1.0, 0.94),
    );
    painter.text(
        &detail_layout,
        Vec2::new(rect.min.x + 33.0, rect.min.y + 25.0),
        Color::white_alpha(0.56),
    );
}

fn scale_bar(painter: &mut Painter, rect: Rect, world_per_point: f32) {
    let length = nice_length(SCALE_TARGET * world_per_point);
    let points = length / world_per_point;
    let left = rect.min.x + 18.0;
    let bottom = rect.max.y - 24.0;
    let color = BAR;
    let pill = Rect::from_min_size(
        Vec2::new(left - 9.0, bottom - 10.0),
        Vec2::new(points + 64.0, 23.0),
    );
    painter.outlined(
        pill,
        Corners::capsule(),
        Fill::Solid(Color::black_alpha(0.38)),
        grana_ui::Border {
            width: 0.5,
            color: Color::white_alpha(0.10),
        },
        grana_ui::Shadow::NONE,
    );
    painter.segment(
        Vec2::new(left, bottom),
        Vec2::new(left + points, bottom),
        2.0,
        color,
    );
    painter.segment(
        Vec2::new(left, bottom - 4.0),
        Vec2::new(left, bottom + 4.0),
        2.0,
        color,
    );
    painter.segment(
        Vec2::new(left + points, bottom - 4.0),
        Vec2::new(left + points, bottom + 4.0),
        2.0,
        color,
    );
    painter.text_at(
        Vec2::new(left + points + 8.0, bottom - 7.0),
        &format_length(length),
        caption_style(),
    );
}

fn paint_legends(painter: &mut Painter, rect: Rect, legends: &[Legend]) {
    if legends.is_empty() || rect.width() < 340.0 || rect.height() < 260.0 {
        return;
    }
    let left = rect.max.x - LEGEND_MARGIN - LEGEND_WIDTH;
    let mut title_top = rect.min.y + 52.0;
    for legend in legends {
        let bar_top = title_top + 16.0;
        let bar_bottom = bar_top + LEGEND_BAR_HEIGHT;
        if bar_bottom + 12.0 > rect.max.y {
            break;
        }
        let card = Rect::from_min_max(
            Vec2::new(left - 10.0, title_top - 9.0),
            Vec2::new(left + LEGEND_WIDTH, bar_bottom + 10.0),
        );
        painter.rect(card, Corners::all(10.0), Color::black_alpha(0.42));
        painter.text_at(
            Vec2::new(left, title_top),
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
                    Vec2::new(left, bottom - step),
                    Vec2::new(left + LEGEND_BAR_WIDTH, bottom + 0.5),
                ),
                Corners::ZERO,
                Color::rgb(red, green, blue),
            );
        }
        let (minimum, maximum) = legend.extent;
        let text_left = left + LEGEND_BAR_WIDTH + 8.0;
        for (align, offset, value) in [
            (Align::Start, 0.0, maximum),
            (Align::Center, 0.5, 0.5 * (minimum + maximum)),
            (Align::End, 1.0, minimum),
        ] {
            let height = 14.0;
            let top = bar_top + (bar_bottom - bar_top - height) * offset;
            painter.text_at(
                Vec2::new(text_left, top + 1.0),
                &coloring::value(value, legend.unit, legend.decimals),
                TextStyle::new(TypeStyle::new(11.0, 400.0, 14.0), LABEL).aligned(align),
            );
        }
        title_top = bar_bottom + 36.0;
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

fn plane_name(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "YZ",
        Axis::Y => "XZ",
        Axis::Z => "XY",
    }
}
