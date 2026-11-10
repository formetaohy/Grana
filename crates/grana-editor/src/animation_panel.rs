use glam::Vec2;
use grana_session::{Command, Export};
use grana_ui::{
    Align, Axis, BuildContext, Color, Corners, Cursor, Element, Icon, Key, Length, Level, Rect,
    Role, State, TextStyle, TypeStyle, icon,
};

use crate::message::Message;
use crate::state::View;
use crate::view;

const WIDTHS: [u32; 4] = [1280, 1920, 2560, 3840];
const IMAGE_WIDTHS: [u32; 5] = [0, 1280, 1920, 2560, 3840];
const TRACK_HEIGHT: f32 = 58.0;

pub fn view<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let timeline_card = grana_ui::column()
        .width(Length::Fixed(470.0))
        .gap(8.0)
        .children([timeline(ctx, state)]);
    let movie_card = grana_ui::column()
        .width(Length::Fixed(270.0))
        .gap(8.0)
        .children([movie(ctx, state)]);
    let still_card = grana_ui::column()
        .width(Length::Fill)
        .gap(8.0)
        .children([still(ctx, state)]);
    grana_ui::row()
        .gap(12.0)
        .align(Align::Start)
        .children([timeline_card, movie_card, still_card])
}

fn timeline<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let duration = state.timeline.duration();
    let position = state.workspace.playback.position.clamp(0.0, duration);
    let previewing = state.workspace.playback.previewing;
    let empty = state.timeline.is_empty();
    let keys = state.timeline.keys().to_vec();
    let selected = state.workspace.playback.selected;
    let current_frame = state
        .timeline
        .sample(position)
        .map_or(1, |sample| sample.time + 1);
    let key_subtitle = format!(
        "{} keyframe{}",
        keys.len(),
        if keys.len() == 1 { "" } else { "s" }
    );
    let theme = *ctx.theme();
    let (symbol, label) = if previewing {
        (Icon::Pause, "Pause")
    } else {
        (Icon::Play, "Preview")
    };
    let mut controls = vec![
        grana_ui::widget::action_button(&theme, symbol, label, Role::Prominent)
            .named("animation.preview")
            .press(Message::SetPreviewing(!previewing)),
        grana_ui::widget::action_button(&theme, Icon::Plus, "Keyframe", Role::Control)
            .named("animation.keyframe")
            .press(Message::Command(Command::SetKeyframe)),
    ];
    if !empty {
        controls.push(
            view::button(ctx, "Clear", Role::Quiet)
                .press(Message::Command(Command::ClearKeyframes)),
        );
    }
    controls.push(grana_ui::text(
        format!("{position:.2} / {duration:.2} s · F{current_frame}"),
        TextStyle::new(theme.font(Level::Monospace), theme.palette.text_tertiary),
    ));
    let mut children = vec![
        view::card_title(ctx, Icon::Film, "Timeline", Some(&key_subtitle)),
        grana_ui::row()
            .gap(6.0)
            .align(Align::Center)
            .children(controls),
        keyframe_track(ctx, duration, position, &keys, selected),
    ];
    if !keys.is_empty() {
        children.push(keyframe_list(ctx, &keys, selected));
    }
    view::card(ctx, 8.0, children)
}

fn keyframe_track<'a>(
    ctx: &mut BuildContext<'_, Message>,
    duration: f32,
    position: f32,
    keys: &[grana_animation::Keyframe],
    selected: Option<usize>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let accent = theme.palette.accent;
    let label_style = TextStyle::new(
        TypeStyle::new(9.5, 400.0, 12.0),
        theme.palette.text_quaternary,
    );
    let empty = TextStyle::new(
        TypeStyle::new(10.5, 400.0, 13.0),
        theme.palette.text_tertiary,
    )
    .aligned(Align::Center);
    let tick_color = Color::white_alpha(0.14);
    let track_key = Key::new("animation.track");
    let track_rect = ctx.rect_of(track_key).unwrap_or(Rect::ZERO);
    let bar = Rect::from_min_max(
        Vec2::new(track_rect.min.x + 8.0, track_rect.center().y - 1.0),
        Vec2::new(track_rect.max.x - 8.0, track_rect.center().y + 1.0),
    );
    let keys = keys.to_vec();
    let markers = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (index, key.at / duration.max(1.0e-6)))
        .collect::<Vec<_>>();
    let markers_paint = markers.clone();
    let handler_markers = markers
        .iter()
        .map(|(index, fraction)| (*index, bar.min.x + bar.width() * fraction))
        .collect::<Vec<_>>();
    let key_selected = selected;
    let hover = ctx.pointer();
    grana_ui::canvas(Length::Fill, TRACK_HEIGHT, move |painter, info| {
        let rect = info.rect;
        let bar = Rect::from_min_max(
            Vec2::new(rect.min.x + 8.0, rect.center().y - 1.0),
            Vec2::new(rect.max.x - 8.0, rect.center().y + 1.0),
        );
        let playhead_x = bar.min.x + bar.width() * (position / duration.max(1.0e-6));
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
        painter.outlined(
            Rect::from_center_size(Vec2::new(playhead_x, bar.center().y), Vec2::new(14.0, 20.0)),
            Corners::capsule(),
            grana_ui::Fill::Solid(Color::white_alpha(0.16)),
            grana_ui::Border {
                width: 1.0,
                color: Color::white_alpha(0.30),
            },
            grana_ui::Shadow::NONE,
        );
        for (index, fraction) in &markers_paint {
            let x = bar.min.x + bar.width() * fraction;
            let center = Vec2::new(x, bar.center().y);
            let active =
                key_selected == Some(*index) || (hover.x - x).abs() <= 10.0 && bar.contains(hover);
            let size = if active { 6.2 } else { 5.2 };
            let color = if key_selected == Some(*index) {
                accent
            } else {
                Color::WHITE
            };
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
    })
    .named("animation.track")
    .cursor(Cursor::Crosshair)
    .on_pointer(move |pointer| {
        let t = ((pointer.position.x - bar.min.x) / bar.width().max(1.0)).clamp(0.0, 1.0);
        let time = t * duration;
        if pointer.phase == grana_ui::Phase::Up
            && let Some((index, _)) = handler_markers
                .iter()
                .find(|(_, x)| (pointer.position.x - *x).abs() <= 10.0)
        {
            return Some(Message::SetPlaybackSelected(Some(*index)));
        }
        Some(Message::SetPlaybackPosition(time))
    })
}

fn keyframe_list<'a>(
    ctx: &mut BuildContext<'_, Message>,
    keys: &[grana_animation::Keyframe],
    selected: Option<usize>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let accent = theme.palette.accent;
    let secondary = theme.palette.text_secondary;
    let font = theme.font(Level::Monospace);
    let mut rows = Vec::new();
    for (index, key) in keys.iter().enumerate() {
        let active = selected == Some(index);
        let text = format!(
            "{:>6.2}s · F{:>2} · Yaw {:>5.2} · Dist {:>6.1}",
            key.at,
            key.time + 1,
            key.views[0].camera.yaw,
            key.views[0].camera.distance
        );
        let body = grana_ui::canvas(Length::Fixed(424.0), 24.0, move |painter, info| {
            if active {
                painter.rect(info.rect, Corners::all(8.0), accent.with_alpha(0.16));
            } else if info.hover > 0.01 {
                painter.rect(
                    info.rect,
                    Corners::all(8.0),
                    Color::white_alpha(0.05 * info.hover),
                );
            }
            painter.text_in(
                info.rect.shrink2(Vec2::new(8.0, 0.0)),
                &text,
                TextStyle::new(font, secondary),
                Align::Center,
            );
        })
        .named(&format!("animation.key.{index}"))
        .on_pointer(move |pointer| {
            (pointer.phase == grana_ui::Phase::Up)
                .then_some(Message::SetPlaybackSelected(Some(index)))
        });
        let delete = view::icon_button(
            ctx,
            &format!("animation.key.delete.{index}"),
            Icon::Trash,
            18.0,
            Role::Quiet,
            "Delete keyframe",
            Message::Command(Command::DeleteKeyframe(index)),
        );
        rows.push(
            grana_ui::row()
                .gap(4.0)
                .align(Align::Center)
                .children([body, delete]),
        );
    }
    Element::new(grana_ui::Layout::column())
        .scroll(Axis::Column, Length::Fixed(88.0), None)
        .clipped()
        .children(rows)
}

fn movie<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let export = state.export;
    let width = export.movie;
    let fps = state.timeline.fps();
    let duration = state.timeline.duration();
    let frames = state.io.frames;
    let interval = state.io.frame_interval;
    let running = state.io.exporting;
    let theme = *ctx.theme();
    let rects = state.stage.rects();
    let stage_width = (rects[1].max.x - rects[0].min.x).max(1.0);
    let stage_height = rects[0].height().max(1.0);
    let movie_size = [
        width,
        even((width as f32 * stage_height / stage_width).round() as u32),
    ];
    let labels = WIDTHS
        .iter()
        .map(|width| format!("{width} px"))
        .collect::<Vec<_>>();
    let selected = WIDTHS.iter().position(|value| *value == width).unwrap_or(1);
    let width_combo = combo(
        ctx,
        "animation.movie.width",
        150.0,
        selected,
        &labels,
        move |index| match WIDTHS.get(index) {
            Some(value) => Message::SetExport(Export {
                movie: *value,
                ..export
            }),
            None => Message::Noop,
        },
    );
    let fps_value = grana_ui::widget::drag_value(&theme, fps as f32, 0)
        .named("animation.fps")
        .on_drag_value(0.5, move |value| {
            Message::Command(Command::SetTimelineFps(
                (value.round() as u32).clamp(1, grana_animation::Timeline::MAX_FPS),
            ))
        });
    let fps_row = grana_ui::row().gap(6.0).align(Align::Center).children([
        fps_value,
        view::caption(ctx, &format!("fps · {} frames", state.timeline.frames())),
    ]);
    let duration_value = grana_ui::widget::drag_value(&theme, duration, 2)
        .named("animation.duration")
        .on_drag_value(0.1, |value| {
            Message::Command(Command::SetTimelineDuration(value.clamp(0.5, 600.0)))
        });
    let duration_row = grana_ui::row().gap(6.0).align(Align::Center).children([
        duration_value,
        view::caption(ctx, &format!("s ({:.1} s data)", frames as f32 * interval)),
    ]);
    let mut children = vec![
        view::card_title(
            ctx,
            Icon::Film,
            "Movie",
            Some(&format!("{} × {}", movie_size[0], movie_size[1])),
        ),
        view::form_row(ctx, "Width", width_combo),
        view::form_row(ctx, "Frame rate", fps_row),
        view::form_row(ctx, "Duration", duration_row),
        view::divider(ctx),
    ];
    match running {
        Some((frame, total)) => {
            let fraction = frame as f32 / total.max(1) as f32;
            children.push(grana_ui::widget::progress(fraction, 240.0));
            children.push(
                grana_ui::row().gap(8.0).align(Align::Center).children([
                    view::caption(ctx, &format!("Exporting {frame}/{total}")),
                    view::button(ctx, "Cancel", Role::Quiet)
                        .press(Message::Command(Command::CancelExport)),
                ]),
            );
        }
        None => {
            let empty = state.timeline.is_empty();
            let mp4 = grana_ui::widget::button_enabled(
                &theme,
                "Export Movie (MP4)",
                Role::Prominent,
                Length::Fill,
                !empty,
            )
            .named("animation.export.mp4");
            let mov = grana_ui::widget::button_enabled(
                &theme,
                "Transparent Movie (MOV)",
                Role::Control,
                Length::Fill,
                !empty,
            )
            .named("animation.export.mov");
            children.push(if empty {
                mp4
            } else {
                mp4.press(Message::Command(Command::ExportMovie {
                    transparent: false,
                }))
            });
            children.push(if empty {
                mov
            } else {
                mov.press(Message::Command(Command::ExportMovie { transparent: true }))
            });
        }
    }
    view::card(ctx, 6.0, children)
}

fn still<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let export = state.export;
    let image_width = export.image;
    let transparent = export.transparent;
    let running = state.io.exporting;
    let frames = state.io.frames;
    let message = state.io.export_message.clone();
    let rects = state.stage.rects();
    let stage_width = (rects[1].max.x - rects[0].min.x).max(1.0);
    let resolved_width = if image_width == 0 {
        (stage_width * ctx.scale()).round() as u32
    } else {
        image_width
    };
    let labels = IMAGE_WIDTHS
        .iter()
        .map(|width| width_label(*width))
        .collect::<Vec<_>>();
    let selected = IMAGE_WIDTHS
        .iter()
        .position(|value| *value == image_width)
        .unwrap_or(0);
    let width_combo = combo(
        ctx,
        "animation.image.width",
        170.0,
        selected,
        &labels,
        move |index| match IMAGE_WIDTHS.get(index) {
            Some(value) => Message::SetExport(Export {
                image: *value,
                ..export
            }),
            None => Message::Noop,
        },
    );
    let transparent_row =
        grana_ui::widget::toggle_row(&theme, "Transparent Background", transparent)
            .named("animation.transparent")
            .on_toggle(move |value| {
                Message::SetExport(Export {
                    transparent: value,
                    ..export
                })
            });
    let mut children = vec![
        view::card_title(
            ctx,
            Icon::Camera,
            "Still & Sequence",
            Some(&format!("{resolved_width} px")),
        ),
        view::form_row(ctx, "Width", width_combo),
        transparent_row,
        view::labelled(ctx, "Sequence", &format!("{frames} PNG frames")),
        view::divider(ctx),
    ];
    let idle = running.is_none();
    let save =
        grana_ui::widget::button_enabled(&theme, "Save Image", Role::Prominent, Length::Fill, idle)
            .named("animation.save.image");
    children.push(if idle {
        save.press(Message::Command(Command::SaveSnapshot))
    } else {
        save
    });
    children.push(view::caption(
        ctx,
        "Eight bit PNG or sixteen bit OME-TIFF, chosen by the saved extension.",
    ));
    let sequence = grana_ui::widget::button_enabled(
        &theme,
        "Export Frame Sequence",
        Role::Control,
        Length::Fill,
        idle && frames > 0,
    )
    .named("animation.export.sequence");
    children.push(if idle && frames > 0 {
        sequence.press(Message::Command(Command::ExportSequence))
    } else {
        sequence
    });
    if let Some(message) = message {
        children.push(view::weak(ctx, &message));
    }
    view::card(ctx, 6.0, children)
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

fn combo<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    width: f32,
    selected: usize,
    labels: &[String],
    message: impl Fn(usize) -> Message + 'static,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let label = labels.get(selected).cloned().unwrap_or_default();
    let style =
        TextStyle::new(theme.font(Level::Callout).medium(), palette.text_primary).truncating();
    let rest = theme.surface(Role::Control, State::Rest);
    let hover = theme.surface(Role::Control, State::Hover);
    let anchor = grana_ui::canvas(Length::Fixed(width), 28.0, move |painter, info| {
        let surface = if info.hover > 0.5 { hover } else { rest };
        painter.outlined(
            info.rect,
            surface.corners,
            surface.fill,
            surface.border,
            surface.shadow,
        );
        painter.text_in(
            Rect::from_min_max(
                info.rect.min + Vec2::new(10.0, 0.0),
                Vec2::new(info.rect.max.x - 22.0, info.rect.max.y),
            ),
            &label,
            style,
            Align::Center,
        );
        icon::chevron(
            painter,
            Vec2::new(info.rect.max.x - 11.0, info.rect.center().y),
            5.0,
            std::f32::consts::FRAC_PI_2,
            palette.text_tertiary,
            1.5,
        );
    })
    .named(key)
    .press(Message::Noop)
    .cursor(Cursor::Pointing);
    let mut rows = Vec::with_capacity(labels.len());
    for (index, label) in labels.iter().enumerate() {
        rows.push(
            grana_ui::widget::menu_row(&theme, label, index == selected)
                .key(Key::index(key, index))
                .press(message(index)),
        );
    }
    let popup = grana_ui::popover::popup(Key::new(key), width.max(120.0), rows);
    grana_ui::column()
        .width(Length::Auto)
        .children([anchor, popup])
}
