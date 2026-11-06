use glam::Vec2;
use grana_ui::{
    Align, Axis, Block, Color, Corners, Icon, Length, Motion, Rect, Role, TextStyle, TypeStyle, Ui,
};

use crate::panel::{card, card_title, divider, form_combo, form_row};
use crate::state::State;
use crate::widget;
use grana_session::{Command, Export};

const WIDTHS: [u32; 4] = [1280, 1920, 2560, 3840];
const IMAGE_WIDTHS: [u32; 5] = [0, 1280, 1920, 2560, 3840];
const TRACK_HEIGHT: f32 = 58.0;

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let duration = state.timeline.duration();
    let position = state.workspace.playback.position.clamp(0.0, duration);
    let previewing = state.workspace.playback.previewing;
    let empty = state.timeline.is_empty();
    let keys = state.timeline.keys().to_vec();
    let selected = state.workspace.playback.selected;
    let fps = state.timeline.fps();
    let width = state.workspace.playback.width;
    let export = state.io.export;
    let message = state.io.export_message.clone();
    let frames = state.io.frames;
    let interval = state.io.frame_interval;
    let timeline_frames = state.timeline.frames();
    let current_frame = state
        .timeline
        .sample(position)
        .map_or(1, |sample| sample.time + 1);
    let gap = 12.0;
    let sheet_width = ui.available().x;
    let export_card_w = ((sheet_width - gap * 2.0) * 0.27).clamp(240.0, 296.0);
    let timeline_card_w = (sheet_width - gap * 2.0 - export_card_w * 2.0).max(340.0);
    let width_key = ui.id("export.width");
    let transparent_key = ui.id("export.transparent");
    let image_width = ui
        .value_of(width_key)
        .map_or(0, |value| value.round() as u32);
    let transparent = ui
        .value_of(transparent_key)
        .is_some_and(|value| value > 0.5);
    let image_options = IMAGE_WIDTHS
        .iter()
        .map(|width| (*width, width_label(*width)))
        .collect::<Vec<_>>();
    let stage_width = (state.stage.section_rect().max.x - state.stage.volume_rect().min.x).max(1.0);
    let stage_height = (state.stage.volume_rect().max.y - state.stage.volume_rect().min.y).max(1.0);
    let resolved_width = if image_width == 0 {
        (stage_width * ui.scale()).round() as u32
    } else {
        image_width
    };
    let movie_size = [
        width,
        even((width as f32 * stage_height / stage_width).round() as u32),
    ];
    state.workspace.movie_size = movie_size;
    let key_subtitle = format!(
        "{} keyframe{}",
        keys.len(),
        if keys.len() == 1 { "" } else { "s" }
    );
    ui.row(Block::row().gaps(gap).align(Align::Start), |ui| {
        ui.column(
            Block::column()
                .width(Length::Fixed(timeline_card_w))
                .gaps(8.0),
            |ui| {
                card(ui, 8.0, |ui| {
                    card_title(ui, Icon::Film, "Timeline", Some(&key_subtitle));
                    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                        let (symbol, label) = if previewing {
                            (Icon::Pause, "Pause")
                        } else {
                            (Icon::Play, "Preview")
                        };
                        if widget::action_button(ui, "preview", symbol, label, Role::Prominent)
                            .clicked
                        {
                            state.workspace.playback.previewing =
                                !state.workspace.playback.previewing;
                        }
                        if widget::action_button(ui, "key", Icon::Plus, "Keyframe", Role::Control)
                            .clicked
                        {
                            commands.push(Command::SetKeyframe);
                        }
                        if !empty && widget::button(ui, "clear", "Clear", Role::Quiet) {
                            commands.push(Command::ClearKeyframes);
                        }
                        ui.space(4.0);
                        let style = TextStyle::new(
                            ui.theme().font(grana_ui::Level::Monospace),
                            ui.theme().palette.text_tertiary,
                        );
                        ui.text_style(
                            &format!("{position:.2} / {duration:.2} s · F{current_frame}"),
                            style,
                        );
                    });
                    let track = keyframe_track(ui, duration, position, &keys, selected);
                    if let Some(position) = track.scrub {
                        state.workspace.playback.position = position;
                    }
                    if let Some(index) = track.selected {
                        state.workspace.playback.selected = Some(index);
                    }
                    keyframe_list(ui, &keys, selected, commands, &mut state.workspace.playback);
                });
            },
        );
        ui.column(
            Block::column()
                .width(Length::Fixed(export_card_w))
                .gaps(8.0),
            |ui| {
                card(ui, 6.0, |ui| {
                    card_title(
                        ui,
                        Icon::Film,
                        "Movie",
                        Some(&format!("{} × {}", movie_size[0], movie_size[1])),
                    );
                    let options = WIDTHS
                        .iter()
                        .map(|width| (*width, format!("{width} px")))
                        .collect::<Vec<_>>();
                    if let Some(value) =
                        form_combo(ui, "movie.width", "Width", &options, Some(width))
                    {
                        state.workspace.playback.width = value;
                    }
                    form_row(ui, "Frame rate", |ui| {
                        ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                            if let Some(value) = widget::drag_value(ui, "fps", fps as f32, 0.5, 0) {
                                commands.push(Command::SetTimelineFps(
                                    (value.round() as u32)
                                        .clamp(1, grana_animation::Timeline::MAX_FPS),
                                ));
                            }
                            widget::caption(ui, &format!("fps · {timeline_frames} frames"));
                        });
                    });
                    form_row(ui, "Duration", |ui| {
                        ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                            if let Some(value) = widget::drag_value(ui, "length", duration, 0.1, 2)
                            {
                                commands
                                    .push(Command::SetTimelineDuration(value.clamp(0.5, 600.0)));
                            }
                            widget::caption(
                                ui,
                                &format!("s ({:.1} s data)", frames as f32 * interval),
                            );
                        });
                    });
                    divider(ui);
                    match export {
                        Some((frame, total)) => {
                            let fraction = frame as f32 / total.max(1) as f32;
                            widget::progress(ui, fraction, ui.available().x);
                            ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                                widget::caption(ui, &format!("Exporting {frame}/{total}"));
                                if widget::button(ui, "cancel", "Cancel", Role::Quiet) {
                                    commands.push(Command::CancelExport);
                                }
                            });
                        }
                        None => {
                            let enabled = !empty;
                            if widget::button_with_enabled(
                                ui,
                                "export",
                                "Export Movie (MP4)",
                                Role::Prominent,
                                Length::Fill,
                                enabled,
                            )
                            .clicked
                            {
                                commands.push(Command::ExportMovie(Export::new(width, false)));
                            }
                            if widget::button_with_enabled(
                                ui,
                                "export.alpha",
                                "Transparent Movie (MOV)",
                                Role::Control,
                                Length::Fill,
                                enabled,
                            )
                            .clicked
                            {
                                commands.push(Command::ExportMovie(Export::new(width, true)));
                            }
                        }
                    }
                });
            },
        );
        ui.column(Block::column().width(Length::Fill).gaps(8.0), |ui| {
            card(ui, 6.0, |ui| {
                card_title(
                    ui,
                    Icon::Camera,
                    "Still & Sequence",
                    Some(&format!("{resolved_width} px")),
                );
                if let Some(value) = form_combo(
                    ui,
                    "export.width",
                    "Width",
                    &image_options,
                    Some(image_width),
                ) {
                    ui.store_value(width_key, value as f32);
                    commands.push(Command::SetExport(Export::new(value, transparent)));
                }
                if let Some(value) = widget::toggle_row(
                    ui,
                    "export.transparent",
                    "Transparent Background",
                    transparent,
                ) {
                    ui.store_value(transparent_key, if value { 1.0 } else { 0.0 });
                    commands.push(Command::SetExport(Export::new(image_width, value)));
                }
                widget::labelled(ui, "Sequence", &format!("{frames} PNG frames"));
                divider(ui);
                let idle = export.is_none();
                if widget::button_with_enabled(
                    ui,
                    "save.image",
                    "Save Image (PNG)",
                    Role::Prominent,
                    Length::Fill,
                    idle,
                )
                .clicked
                {
                    commands.push(Command::SaveSnapshot);
                }
                if widget::button_with_enabled(
                    ui,
                    "export.sequence",
                    "Export Frame Sequence",
                    Role::Control,
                    Length::Fill,
                    idle && frames > 0,
                )
                .clicked
                {
                    commands.push(Command::ExportSequence(Export::new(
                        image_width,
                        transparent,
                    )));
                }
                if let Some(message) = message {
                    widget::weak(ui, &message);
                }
            });
        });
    });
}

fn width_label(width: u32) -> String {
    if width == 0 {
        "Native Screen".to_string()
    } else {
        format!("{width} px")
    }
}

fn even(value: u32) -> u32 {
    (value.max(2) / 2) * 2
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
            let tick_rect = if index == 0 {
                Rect::from_min_size(Vec2::new(x, bar.max.y + 6.0), Vec2::new(40.0, 14.0))
            } else if index == ticks {
                Rect::from_min_size(Vec2::new(x - 40.0, bar.max.y + 6.0), Vec2::new(40.0, 14.0))
            } else {
                Rect::from_center_size(Vec2::new(x, bar.max.y + 13.0), Vec2::new(40.0, 14.0))
            };
            let align = if index == 0 {
                Align::Start
            } else if index == ticks {
                Align::End
            } else {
                Align::Center
            };
            painter.text_in(tick_rect, &label, label_style.aligned(align), Align::Center);
        }
        painter.segment(
            Vec2::new(playhead_x, rect.min.y + 8.0),
            Vec2::new(playhead_x, rect.max.y - 16.0),
            1.2,
            accent,
        );
    }
    let playhead_knob =
        Rect::from_center_size(Vec2::new(playhead_x, bar.center().y), Vec2::new(14.0, 20.0));
    let playhead_key = key ^ 0x77;
    ui.glass(
        playhead_key,
        playhead_knob,
        grana_ui::glass::GlassStyle::knob(),
    );
    ui.engage_glass(playhead_key, response.active);
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
            "Add a keyframe to animate the camera and section",
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
    commands: &mut Vec<Command>,
    playback: &mut crate::state::Playback,
) {
    if keys.is_empty() {
        return;
    }
    ui.scroll_axis(
        ui.id("keyframes"),
        Axis::Column,
        Length::Fixed(88.0),
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
                        painter.rect(row, Corners::all(8.0), accent.with_alpha(0.16));
                    } else if hover > 0.01 {
                        painter.rect(row, Corners::all(8.0), Color::white_alpha(0.05 * hover));
                    }
                    painter.text_in(
                        Rect::from_min_max(
                            Vec2::new(row.min.x + 8.0, row.min.y),
                            Vec2::new(row.max.x - 28.0, row.max.y),
                        ),
                        &format!(
                            "{:>6.2}s · F{:>2} · Yaw {:>5.2} · Dist {:>6.1}",
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
                    playback.selected = Some(index);
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
                            commands.push(Command::DeleteKeyframe(index));
                        }
                    },
                );
            }
        },
    );
}
