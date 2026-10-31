use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke};
use glam::{Mat4, Vec3};
use grana_render::{Camera, SectionCamera, plane_basis};
use grana_volume::Axis;

const LABEL_COLOR: Color32 = Color32::from_gray(196);
const BAR_COLOR: Color32 = Color32::from_gray(225);
const CROSS_COLOR: Color32 = Color32::from_rgba_premultiplied(220, 230, 240, 150);
const PLANE_COLOR: Color32 = Color32::from_rgba_premultiplied(110, 190, 255, 110);
const SEPARATOR_COLOR: Color32 = Color32::from_gray(44);
const SCALE_TARGET: f32 = 116.0;

pub struct Overlay<'a> {
    pub camera: &'a Camera,
    pub section: &'a SectionCamera,
    pub world: Vec3,
    pub levels: usize,
    pub volume_level: usize,
    pub section_level: usize,
    pub volume_rect: Rect,
    pub section_rect: Rect,
}

pub fn paint(ui: &egui::Ui, overlay: &Overlay<'_>) {
    let painter = ui.painter();
    painter.line_segment(
        [
            Pos2::new(overlay.volume_rect.right() + 1.0, overlay.volume_rect.top()),
            Pos2::new(
                overlay.volume_rect.right() + 1.0,
                overlay.volume_rect.bottom(),
            ),
        ],
        Stroke::new(1.0, SEPARATOR_COLOR),
    );
    paint_volume(ui, overlay);
    paint_section(ui, overlay);
}

fn paint_volume(ui: &egui::Ui, overlay: &Overlay<'_>) {
    let painter = ui.painter().with_clip_rect(overlay.volume_rect);
    let aspect = overlay.volume_rect.width() / overlay.volume_rect.height();
    let view_projection = overlay
        .camera
        .view_projection(aspect, 0.5 * overlay.world.length());
    let corners = plane_corners(
        overlay.camera.focus(),
        overlay.section.axis(),
        overlay.world,
    );
    let projected = corners.map(|corner| project(view_projection, corner, overlay.volume_rect));
    if let [Some(a), Some(b), Some(c), Some(d)] = projected {
        let stroke = Stroke::new(1.0, PLANE_COLOR);
        for (from, to) in [(a, b), (b, c), (c, d), (d, a)] {
            painter.line_segment([from, to], stroke);
        }
    }
    if let Some(focus) = project(view_projection, overlay.camera.focus(), overlay.volume_rect) {
        cross(&painter, focus, 5.0, 1.0, CROSS_COLOR);
    }
    let world_per_point = 2.0 * overlay.camera.distance() * (overlay.camera.fov_y() * 0.5).tan()
        / overlay.volume_rect.height();
    scale_bar(&painter, overlay.volume_rect, world_per_point);
    let level = overlay.volume_level + 1;
    label(
        &painter,
        overlay.volume_rect,
        format!("3D · level {level} / {}", overlay.levels),
    );
}

fn paint_section(ui: &egui::Ui, overlay: &Overlay<'_>) {
    let painter = ui.painter().with_clip_rect(overlay.section_rect);
    let axis = overlay.section.axis();
    let focus = overlay.camera.focus();
    let plane = focus[axis.index()] + 0.5 * overlay.world[axis.index()];
    let world_per_point = overlay.section.span() / overlay.section_rect.height();
    let world_per_pixel = world_per_point / ui.ctx().pixels_per_point().max(1.0);
    scale_bar(&painter, overlay.section_rect, world_per_point);
    let center = overlay.section_rect.center();
    cross(&painter, center, 6.0, 1.0, CROSS_COLOR);
    label(
        &painter,
        overlay.section_rect,
        format!(
            "{} · {} = {plane:.2} µm · {world_per_pixel:.3} µm/px · level {} / {}",
            plane_name(axis),
            axis.name(),
            overlay.section_level + 1,
            overlay.levels,
        ),
    );
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

fn project(view_projection: Mat4, point: Vec3, rect: Rect) -> Option<Pos2> {
    let clip = view_projection * point.extend(1.0);
    if clip.w <= 1.0e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(Pos2::new(
        rect.left() + (ndc.x * 0.5 + 0.5) * rect.width(),
        rect.top() + (0.5 - ndc.y * 0.5) * rect.height(),
    ))
}

fn cross(painter: &Painter, center: Pos2, radius: f32, width: f32, color: Color32) {
    let stroke = Stroke::new(width, color);
    painter.line_segment(
        [
            Pos2::new(center.x - radius, center.y),
            Pos2::new(center.x + radius, center.y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(center.x, center.y - radius),
            Pos2::new(center.x, center.y + radius),
        ],
        stroke,
    );
}

fn scale_bar(painter: &Painter, rect: Rect, world_per_point: f32) {
    let length = nice_length(SCALE_TARGET * world_per_point);
    let points = length / world_per_point;
    let left = rect.left() + 18.0;
    let bottom = rect.bottom() - 22.0;
    let stroke = Stroke::new(2.0, BAR_COLOR);
    painter.line_segment(
        [Pos2::new(left, bottom), Pos2::new(left + points, bottom)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(left, bottom - 4.0), Pos2::new(left, bottom + 4.0)],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(left + points, bottom - 4.0),
            Pos2::new(left + points, bottom + 4.0),
        ],
        stroke,
    );
    painter.text(
        Pos2::new(left + points + 8.0, bottom),
        Align2::LEFT_CENTER,
        format_length(length),
        FontId::proportional(12.0),
        LABEL_COLOR,
    );
}

fn label(painter: &Painter, rect: Rect, text: String) {
    painter.text(
        Pos2::new(rect.left() + 18.0, rect.top() + 12.0),
        Align2::LEFT_TOP,
        text,
        FontId::proportional(12.0),
        LABEL_COLOR,
    );
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
