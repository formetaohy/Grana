use glam::Vec2;
use grana_ui::{
    Ambient, Border, Color, Corners, Cursor, Fill, Icon, Level, Rect, Role, Shadow, TextStyle, Ui,
    icon,
};

use crate::action::Action;
use crate::overlay::{self, Overlay};
use crate::state::State;

use super::{Layout, SPLITTER, scene_uv};

const STAGE_RADIUS: f32 = 20.0;
const SECTION_RADIUS: f32 = 16.0;

pub(super) fn paint_backdrop(ui: &mut Ui, layout: &Layout, state: &State<'_>) {
    let full = Rect::from_min_size(Vec2::ZERO, ui.size());
    {
        let mut painter = ui.painter();
        painter.gradient(
            full,
            Corners::ZERO,
            Color::rgb(0.024, 0.032, 0.047),
            Color::rgb(0.004, 0.006, 0.010),
        );
        painter.ambient(
            full,
            Corners::ZERO,
            Ambient {
                uv: cover_uv(
                    Rect::from_center_size(
                        layout.volume.center() + Vec2::new(layout.volume.width() * 0.28, 0.0),
                        layout.volume.size(),
                    ),
                    full.size(),
                ),
                tint: Color::rgba(0.010, 0.016, 0.028, 0.15),
                blur: 0.0,
                saturation: 1.05,
                brightness: 0.85,
                opacity: 1.0,
            },
        );
        let channels = state.scene.dataset.channels();
        let glows = [
            (
                Vec2::new(full.width() * 0.20, full.height() * 0.22),
                full.width() * 0.82,
                0.10,
            ),
            (
                Vec2::new(full.width() * 0.82, full.height() * 0.72),
                full.width() * 0.90,
                0.11,
            ),
            (
                Vec2::new(full.width() * 0.62, full.height() * 0.38),
                full.width() * 0.56,
                0.07,
            ),
        ];
        for (index, (center, diameter, alpha)) in glows.iter().enumerate() {
            let channel = &channels[index % channels.len()];
            let tint = Color::from_rgb_f32(channel.tint()).with_alpha(*alpha);
            painter.glow(
                Rect::from_center_size(*center, Vec2::splat(*diameter)),
                tint,
                0.12,
            );
        }
        calibration_grid(&mut painter, full);
        painter.rect(full, Corners::ZERO, Color::black_alpha(0.24));
        painter.gradient(
            Rect::from_min_size(full.min, Vec2::new(full.width(), 150.0)),
            Corners::ZERO,
            Color::black_alpha(0.28),
            Color::TRANSPARENT,
        );
        painter.gradient(
            Rect::from_min_size(
                Vec2::new(full.min.x, full.max.y - 190.0),
                Vec2::new(full.width(), 190.0),
            ),
            Corners::ZERO,
            Color::TRANSPARENT,
            Color::black_alpha(0.26),
        );
    }
}

fn calibration_grid(painter: &mut grana_ui::Painter<'_>, full: Rect) {
    let step = 28.0;
    let minor = Color::white_alpha(0.22);
    let major = Color::white_alpha(0.32);
    let mut index = 0;
    let mut x = full.min.x;
    while x <= full.max.x {
        let color = if index % 5 == 0 { major } else { minor };
        painter.segment(
            Vec2::new(x, full.min.y),
            Vec2::new(x, full.max.y),
            0.5,
            color,
        );
        x += step;
        index += 1;
    }
    index = 0;
    let mut y = full.min.y;
    while y <= full.max.y {
        let color = if index % 5 == 0 { major } else { minor };
        painter.segment(
            Vec2::new(full.min.x, y),
            Vec2::new(full.max.x, y),
            0.5,
            color,
        );
        y += step;
        index += 1;
    }
}

pub(super) fn paint_stage(ui: &mut Ui, layout: &Layout) {
    let size = ui.size();
    {
        let mut painter = ui.painter();
        for (rect, corners, radius) in [
            (layout.volume, Corners::all(STAGE_RADIUS), 30.0),
            (layout.section, Corners::all(SECTION_RADIUS), 26.0),
        ] {
            painter.shadow(
                rect,
                corners,
                Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.32), radius, 12.0),
            );
            painter.scene(rect, corners, scene_uv(rect, size));
            painter.outlined(
                rect,
                corners,
                Fill::None,
                Border {
                    width: 1.0,
                    color: Color::white_alpha(0.13),
                },
                Shadow::NONE,
            );
        }
    }
}

pub(super) fn paint_overlay(ui: &mut Ui, layout: &Layout, state: &mut State<'_>) {
    let axis = state.viewport.section().axis();
    let (plane_u, plane_v) = axis.in_plane();
    let world = state.scene.dataset.world_size();
    let points_per_micron = layout.section.height() / state.viewport.section().span();
    let stage = Rect::from_center_size(
        layout.section.center(),
        Vec2::new(
            world[plane_u.index()] * points_per_micron,
            world[plane_v.index()] * points_per_micron,
        ),
    )
    .intersect(layout.section);
    let legends = overlay::legends(state.scene);
    let mut overlay = Overlay {
        camera: state.viewport.camera(),
        section: state.viewport.section(),
        world,
        volume_safe: layout.volume,
        section_stage: stage,
        levels: state.scene.dataset.level_count(),
        volume_level: state.frame.volume.level,
        section_level: state.frame.section.level,
        samples: state.frame.samples,
        volume_rect: layout.volume,
        section_rect: layout.section,
        region: &mut state.scene.region,
        legends: &legends,
        recording: state.animation.movie,
    };
    overlay::paint(ui, &mut overlay);
}

pub(super) fn split_handle(ui: &mut Ui, volume: Rect, section: Rect, actions: &mut Vec<Action>) {
    let x = volume.max.x + SPLITTER * 0.5;
    let rect = Rect::from_center_size(Vec2::new(x, volume.center().y), Vec2::new(6.0, 66.0));
    let response = ui.interact(ui.id("split.handle"), rect.expand(6.0));
    if response.hovered || response.dragging {
        ui.set_cursor(Cursor::ResizeHorizontal);
    }
    if response.dragging {
        let pointer = ui.input().position().x;
        let total = volume.width() + section.width() + SPLITTER;
        actions.push(Action::SetSplit(
            ((pointer - volume.min.x) / total).clamp(0.3, 0.75),
        ));
    }
    let look = ui.theme().look(Role::Toolbar, grana_ui::State::Rest);
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        Corners::capsule(),
        look.fill,
        look.border,
        look.shadow,
    );
    let dots = Color::white_alpha(if response.hovered { 0.60 } else { 0.32 });
    for offset in [-6.0f32, 0.0, 6.0] {
        painter.rect(
            Rect::from_center_size(
                Vec2::new(rect.center().x, rect.center().y + offset),
                Vec2::splat(1.6),
            ),
            Corners::capsule(),
            dots,
        );
    }
}

pub(super) fn toast(ui: &mut Ui, stage: Rect, state: &State<'_>) {
    let Some(message) = state.message else {
        return;
    };
    let width = (stage.width() - 48.0).min(580.0);
    let style = TextStyle::new(
        ui.theme().font(Level::Caption),
        ui.theme().palette.text_primary,
    )
    .wrapping();
    let layout = ui.layout_text(message.text(), style, width - 52.0);
    let height = layout.size.y + 20.0;
    let rect = Rect::from_min_size(
        Vec2::new(stage.center().x - width * 0.5, stage.min.y + 14.0),
        Vec2::new(width, height),
    );
    let look = ui.theme().look(Role::Hud, grana_ui::State::Rest);
    let warning = ui.theme().palette.warning;
    let text_color = ui.theme().palette.text_primary;
    let mut painter = ui.painter();
    painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    icon::paint(
        Icon::Warning,
        &mut painter,
        Vec2::new(rect.min.x + 21.0, rect.center().y),
        15.0,
        warning,
        1.5,
    );
    painter.text(
        &layout,
        Vec2::new(rect.min.x + 36.0, rect.min.y + 10.0),
        text_color,
    );
}

fn cover_uv(rect: Rect, size: Vec2) -> [f32; 4] {
    let scale = (size.x / rect.width().max(1.0)).max(size.y / rect.height().max(1.0)) * 1.06;
    let source = Rect::from_center_size(rect.center(), size / scale);
    scene_uv(source, size)
}
