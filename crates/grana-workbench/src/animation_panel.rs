use glam::Vec2;
use grana_ui::{
    Align, Axis, Block, Color, Corners, Icon, Length, Motion, Rect, Role, TextStyle, TypeStyle, Ui,
};

use crate::action::Action;
use crate::panel::{card, card_title};
use crate::state::State;
use crate::widget;

const WIDTHS: [u32; 4] = [1280, 1920, 2560, 3840];
const TRACK_HEIGHT: f32 = 62.0;

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let duration = state.animation.timeline.duration();
    let position = state.animation.position.clamp(0.0, duration);
    let previewing = state.animation.previewing;
    let empty = state.animation.timeline.is_empty();
    let keys = state.animation.timeline.keys().to_vec();
    let selected = state.animation.selected;
    let fps = state.animation.timeline.fps();
    let width = state.animation.width;
    let size = state.animation.size;
    let movie = state.animation.movie;
    let message = state.animation.message.map(str::to_string);
    let frames = state.animation.frames;
    let interval = state.animation.frame_interval;
    let timeline_frames = state.animation.timeline.frames();
    let current_frame = state
        .animation
        .timeline
        .sample(position)
        .map_or(1, |sample| sample.time + 1);
    let gap = 14.0;
    let sheet_width = ui.available().x;
    let export_width = 306.0;
    ui.row(Block::row().gaps(gap).align(Align::Start), |ui| {
        ui.column(
            Block::column()
                .width(Length::Fixed((sheet_width - gap - export_width).max(420.0)))
                .gaps(10.0),
            |ui| {
                card(ui, 8.0, |ui| {
                    card_title(
                        ui,
                        Icon::Film,
                        "Timeline",
                        Some(&format!("{} keyframes", keys.len())),
                    );
                    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                        let (symbol, label) = if previewing {
                            (Icon::Pause, "Pause")
                        } else {
                            (Icon::Play, "Preview")
                        };
                        if widget::action_button(ui, "preview", symbol, label, Role::Prominent)
                            .clicked
                        {
                            actions.push(Action::ToggleAnimation);
                        }
                        if widget::action_button(ui, "key", Icon::Plus, "Keyframe", Role::Control)
                            .clicked
                        {
                            actions.push(Action::SetKeyframe);
                        }
                        if !empty
                            && widget::button(ui, "clear", "Clear", Role::Quiet)
                        {
                            actions.push(Action::ClearKeyframes);
                        }
                        ui.space(2.0);
                        let style = TextStyle::new(
                            ui.theme().font(grana_ui::Level::Monospace),
                            ui.theme().palette.text_tertiary,
                        );
                        ui.text_style(
                            &format!(
                                "{position:.2} / {duration:.2} s · frame {current_frame}"
                            ),
                            style,
                        );
                    });
                    let track = keyframe_track(ui, duration, position, &keys, selected);
                    if let Some(position) = track.scrub {
                        actions.push(Action::ScrubAnimation(position));
                    }
                    if let Some(index) = track.selected {
                        actions.push(Action::SelectKeyframe(index));
                    }
                    keyframe_list(ui, &keys, selected, actions);
                });
            },
        );
        ui.column(
            Block::column()
                .width(Length::Fixed(export_width))
                .gaps(10.0),
            |ui| {
                card(ui, 8.0, |ui| {
                    card_title(ui, Icon::Camera, "Movie", None);
                    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                        widget::field_label(ui, "FPS");
                        if let Some(value) =
                            widget::drag_value(ui, "fps", fps as f32, 0.5, 0)
                        {
                            state.animation.timeline.set_fps(
                                (value.round() as u32)
                                    .clamp(1, grana_animation::Timeline::MAX_FPS),
                            );
                        }
                        widget::field_label(ui, "LENGTH");
                        if let Some(value) =
                            widget::drag_value(ui, "length", duration, 0.1, 2)
                        {
                            state
                                .animation
                                .timeline
                                .set_duration(value.clamp(0.5, 600.0));
                        }
                        widget::caption(ui, "s");
                    });
                    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                        widget::field_label(ui, "WIDTH");
                        let options = WIDTHS
                            .iter()
                            .map(|width| (*width, width.to_string()))
                            .collect::<Vec<_>>();
                        if let Some(value) = grana_ui::popover::combo(
                            ui,
                            "movie.width",
                            96.0,
                            &options,
                            Some(width),
                        ) {
                            actions.push(Action::SetMovieWidth(value));
                        }
                        let style = TextStyle::new(
                            ui.theme().font(grana_ui::Level::Monospace),
                            ui.theme().palette.text_secondary,
                        );
                        ui.text_style(&format!("{} × {}", size[0], size[1]), style);
                    });
                    widget::caption(
                        ui,
                        &format!(
                            "{timeline_frames} frames · {:.1} s of data",
                            frames as f32 * interval
                        ),
                    );
                    ui.separator();
                    match movie {
                        Some((frame, total)) => {
                            let fraction = frame as f32 / total.max(1) as f32;
                            widget::progress(ui, fraction, ui.available().x);
                            ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                                widget::caption(ui, &format!("Rendering {frame} / {total}"));
                                if widget::button(ui, "cancel", "Cancel", Role::Quiet) {
                                    actions.push(Action::CancelMovie);
                                }
                            });
                        }
                        None => {
                            let enabled = !empty;
                            let role = if enabled {
                                Role::Prominent
                            } else {
                                Role::Quiet
                            };
                            if widget::button_with(
                                ui,
                                "export",
                                "Export movie",
                                role,
                                Length::Fill,
                            )
                            .clicked
                                && enabled
                            {
                                actions.push(Action::ExportMovie);
                            }
                        }
                    }
                    if let Some(message) = message {
                        widget::weak(ui, &message);
                    }
                    widget::weak(
                        ui,
                        "Keyframes record the camera, section plane and time point. Motion eases through every keyframe.",
                    );
                });
            },
        );
    });
}

struct TrackResult {
    scrub: Option<f32>,
    selected: Option<usize>,
}

fn keyframe_track(
    ui: &mut Ui,
    duration: f32,
    position: f32,
    keys: &[grana_animation::Keyframe],
    selected: Option<usize>,
) -> TrackResult {
    let rect = ui.allocate_full(TRACK_HEIGHT);
    let bar = Rect::from_min_max(
        Vec2::new(rect.min.x + 8.0, rect.center().y - 1.0),
        Vec2::new(rect.max.x - 8.0, rect.center().y + 1.0),
    );
    let key = ui.id("timeline.track");
    let response = ui.interact(key, rect);
    let mut scrub = None;
    if response.active && duration > 0.0 {
        let pointer = ui.input().position().x;
        let t = ((pointer - bar.min.x) / bar.width().max(1.0)).clamp(0.0, 1.0);
        scrub = Some(t * duration);
    }
    let accent = ui.theme().palette.accent;
    let tick_color = Color::white_alpha(0.14);
    let label_style = TextStyle::new(
        TypeStyle::new(9.5, 400.0, 12.0),
        ui.theme().palette.text_quaternary,
    );
    let playhead_x = bar.min.x + bar.width() * (position / duration.max(1.0e-6));
    {
        let mut painter = ui.painter();
        let rail = Rect::from_center_size(bar.center(), Vec2::new(bar.width(), 4.0));
        painter.rect(rail, Corners::capsule(), Color::white_alpha(0.14));
        painter.rect(
            Rect::from_min_max(
                rail.min,
                Vec2::new(playhead_x.max(rail.min.x + 2.0), rail.max.y),
            ),
            Corners::capsule(),
            accent.with_alpha(0.60),
        );
        let ticks = 6;
        for index in 0..=ticks {
            let x = bar.min.x + bar.width() * index as f32 / ticks as f32;
            painter.rect(
                Rect::from_center_size(Vec2::new(x, bar.center().y), Vec2::new(1.0, 8.0)),
                Corners::capsule(),
                tick_color,
            );
            let label = format!("{:.1}s", duration * index as f32 / ticks as f32);
            painter.text_at(Vec2::new(x + 2.0, bar.max.y + 8.0), &label, label_style);
        }
        painter.segment(
            Vec2::new(playhead_x, rect.min.y + 6.0),
            Vec2::new(playhead_x, rect.max.y - 16.0),
            1.2,
            accent,
        );
        painter.rect(
            Rect::from_center_size(Vec2::new(playhead_x, rect.min.y + 5.0), Vec2::splat(5.0)),
            Corners::capsule(),
            accent,
        );
    }
    let mut chose = None;
    for (index, key) in keys.iter().enumerate() {
        let x = bar.min.x + bar.width() * (key.at / duration.max(1.0e-6)).clamp(0.0, 1.0);
        let center = Vec2::new(x, bar.center().y);
        let hit = Rect::from_center_size(center, Vec2::splat(20.0));
        let response = ui.interact(ui.id(&format!("key.{}", index)), hit);
        if response.clicked {
            chose = Some(index);
        }
        let active = selected == Some(index) || response.hovered;
        let size = if active { 6.2 } else { 5.2 };
        let color = if selected == Some(index) {
            accent
        } else {
            Color::WHITE
        };
        let mut painter = ui.painter();
        painter.triangle(
            center + Vec2::new(0.0, -size),
            center + Vec2::new(size, 0.0),
            center + Vec2::new(0.0, size),
            color,
        );
        painter.triangle(
            center + Vec2::new(0.0, -size),
            center + Vec2::new(-size, 0.0),
            center + Vec2::new(0.0, size),
            color,
        );
    }
    if keys.is_empty() {
        let empty = TextStyle::new(
            TypeStyle::new(10.5, 400.0, 13.0),
            ui.theme().palette.text_tertiary,
        )
        .aligned(Align::Center);
        let mut painter = ui.painter();
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x, rect.min.y),
                Vec2::new(rect.max.x, rect.max.y - 12.0),
            ),
            "Set a keyframe to begin",
            empty,
            Align::Center,
        );
    }
    TrackResult {
        scrub,
        selected: chose,
    }
}

fn keyframe_list(
    ui: &mut Ui,
    keys: &[grana_animation::Keyframe],
    selected: Option<usize>,
    actions: &mut Vec<Action>,
) {
    if keys.is_empty() {
        return;
    }
    ui.scroll_axis(
        ui.id("keyframes"),
        Axis::Column,
        Length::Fixed(92.0),
        None,
        |ui| {
            for (index, key) in keys.iter().enumerate() {
                let row = ui.allocate_full(24.0);
                let response = ui.interact(ui.id(&format!("key.row.{index}")), row);
                let hover = ui.animate(
                    ui.id(&format!("key.row.{index}.hover")),
                    if response.hovered { 1.0 } else { 0.0 },
                    Motion::quick(),
                );
                let active = selected == Some(index);
                let accent = ui.theme().palette.accent;
                let secondary = ui.theme().palette.text_secondary;
                let font = ui.theme().font(grana_ui::Level::Monospace);
                {
                    let mut painter = ui.painter();
                    if active {
                        painter.rect(row, Corners::all(6.0), accent.with_alpha(0.16));
                    } else if hover > 0.01 {
                        painter.rect(row, Corners::all(6.0), Color::white_alpha(0.04 * hover));
                    }
                    painter.text_in(
                        Rect::from_min_max(
                            Vec2::new(row.min.x + 8.0, row.min.y),
                            Vec2::new(row.max.x - 28.0, row.max.y),
                        ),
                        &format!(
                            "{:>6.2}s · frame {:>3} · yaw {:>5.2} · dist {:>6.1}",
                            key.at,
                            key.time + 1,
                            key.camera.yaw,
                            key.camera.distance
                        ),
                        TextStyle::new(font, secondary),
                        Align::Center,
                    );
                }
                if response.clicked {
                    actions.push(Action::SelectKeyframe(index));
                }
                ui.place(
                    Rect::from_center_size(
                        Vec2::new(row.max.x - 12.0, row.center().y),
                        Vec2::splat(20.0),
                    ),
                    |ui| {
                        if widget::icon_button(
                            ui,
                            "delete",
                            Icon::Trash,
                            18.0,
                            Role::Quiet,
                            "Delete keyframe",
                        ) {
                            actions.push(Action::DeleteKeyframe(index));
                        }
                    },
                );
            }
        },
    );
}
