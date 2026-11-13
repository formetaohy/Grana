use glam::Vec2;
use grana_session::Command;
use grana_ui::{
    Align, BuildContext, Color, Corners, Element, Icon, Key, Length, Level, Motion, Pointer, Rect,
    Role, State, TextStyle,
};

use crate::message::Message;
use crate::state::View;

pub(super) fn transport<'a>(
    ctx: &mut BuildContext<'_, Message>,
    rect: Rect,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let inner = theme.metrics.corner_xs;
    let center = rect.center().y;
    let left = rect.min.x + 8.0;
    let back = Rect::from_center_size(Vec2::new(left + 15.0, center), Vec2::splat(30.0));
    let play = Rect::from_center_size(Vec2::new(left + 53.0, center), Vec2::new(42.0, 30.0));
    let forward = Rect::from_center_size(Vec2::new(left + 91.0, center), Vec2::splat(30.0));
    let track_start = forward.max.x + 14.0;
    let readout_width = 148.0;
    let track_end = (rect.max.x - 10.0 - readout_width).max(track_start + 72.0);
    let track = Rect::from_min_max(
        Vec2::new(track_start, center - 14.0),
        Vec2::new(track_end, center + 14.0),
    );
    let frame = state.scene.time();
    let frames = state.scene.frame_count();
    let playing = state.workspace.playback.playing;
    let language = ctx.language();
    let back_element = crate::view::icon_button(
        ctx,
        "timeline.back",
        Icon::StepBack,
        30.0,
        Role::SurfaceHigh,
        grana_i18n::timeline::previous_frame(language),
        Message::Command(Command::SetTime(frame.saturating_sub(1))),
    )
    .shaped(Corners::split_leading(inner))
    .at(back);
    let play_element = crate::view::icon_button(
        ctx,
        "timeline.play",
        if playing { Icon::Pause } else { Icon::Play },
        30.0,
        Role::Filled,
        if playing {
            grana_i18n::timeline::pause(language)
        } else {
            grana_i18n::timeline::play(language)
        },
        Message::SetPlaying(!playing),
    )
    .shaped(Corners::all(10.0))
    .at(play);
    let forward_element = crate::view::icon_button(
        ctx,
        "timeline.forward",
        Icon::StepForward,
        30.0,
        Role::SurfaceHigh,
        grana_i18n::timeline::next_frame(language),
        Message::Command(Command::SetTime((frame + 1) % frames.max(1))),
    )
    .shaped(Corners::split_trailing(inner))
    .at(forward);
    let scrub = scrub_track(ctx, track, state);
    let seconds = frame as f32 * state.scene.dataset.frame_interval();
    let readout = format!("{:02} / {:02}  ·  {seconds:.2} s", frame + 1, frames);
    let readout_rect = Rect::from_min_max(
        Vec2::new(track_end + 10.0, center - 14.0),
        Vec2::new(rect.max.x - 8.0, center + 14.0),
    );
    let font = theme.font(Level::Monospace);
    let readout_element =
        grana_ui::canvas(Length::Fixed(readout_rect.width()), readout_rect.height())
            .view(readout.clone(), move |_, painter, info| {
                painter.rect(
                    info.rect,
                    Corners::capsule(),
                    palette.surface_container_high,
                );
                painter.text_in(
                    info.rect.shrink2(Vec2::new(10.0, 0.0)),
                    &readout,
                    TextStyle::new(font, palette.on_surface).aligned(Align::Center),
                    Align::Center,
                );
            })
            .at(readout_rect);
    let surface = theme.surface(Role::FloatingToolbar, State::Rest);
    Element::new(grana_ui::Layout::column())
        .named("transport")
        .at(rect)
        .background(surface)
        .children([
            back_element,
            play_element,
            forward_element,
            scrub,
            readout_element,
        ])
}

fn scrub_track<'a>(
    ctx: &mut BuildContext<'_, Message>,
    rect: Rect,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let metrics = theme.metrics;
    let frame = state.scene.time();
    let frames = state.scene.frame_count();
    let track_h = metrics.slider_track;
    let bar = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), track_h));
    let position = if frames > 1 {
        ctx.animate(
            Key::new("timeline.spring").id(),
            (frame as f32 / (frames - 1) as f32).clamp(0.0, 1.0),
            Motion::spatial_fast(),
        )
    } else {
        0.0
    };
    let handle_x = bar.min.x + bar.width() * position;
    let gap = metrics.slider_gap;
    let outer_r = track_h * 0.5;
    let inner_r = 2.0;
    let ticks = frames > 1 && frames <= 48 && bar.width() / (frames - 1) as f32 >= 9.0;
    grana_ui::canvas(Length::Fixed(rect.width()), rect.height())
        .view((frames, position.to_bits()), move |_, painter, info| {
            let engaged = info.hover.max(info.press);
            let active_end = (handle_x - gap).clamp(bar.min.x, bar.max.x);
            if active_end > bar.min.x + 1.0 {
                painter.rect(
                    Rect::from_min_max(bar.min, Vec2::new(active_end, bar.max.y)),
                    Corners::left(outer_r, inner_r),
                    palette.primary,
                );
            }
            let inactive_start = (handle_x + gap).clamp(bar.min.x, bar.max.x);
            if bar.max.x > inactive_start + 1.0 {
                painter.rect(
                    Rect::from_min_max(Vec2::new(inactive_start, bar.min.y), bar.max),
                    Corners::right(outer_r, inner_r),
                    palette.surface_container_highest,
                );
            }
            if ticks {
                let spacing = bar.width() / (frames - 1) as f32;
                for index in 0..frames {
                    let x = bar.min.x + spacing * index as f32;
                    if (x - handle_x).abs() <= gap + 3.0 {
                        continue;
                    }
                    let color = if x < handle_x {
                        palette.on_primary.with_alpha(0.45)
                    } else {
                        palette.primary.with_alpha(0.42)
                    };
                    painter.rect(
                        Rect::from_center_size(Vec2::new(x, bar.center().y), Vec2::splat(3.0)),
                        Corners::capsule(),
                        color,
                    );
                }
            } else if bar.max.x > inactive_start + 8.0 {
                painter.rect(
                    Rect::from_center_size(
                        Vec2::new(bar.max.x - 5.0, bar.center().y),
                        Vec2::splat(4.0),
                    ),
                    Corners::capsule(),
                    palette.primary,
                );
            }
            let handle = Rect::from_center_size(
                Vec2::new(handle_x, bar.center().y),
                Vec2::new(4.5 + engaged * 1.2, 24.0 + engaged * 2.5),
            );
            painter.rect(
                handle,
                Corners::capsule(),
                if info.press > 0.5 {
                    palette.primary.lerp(Color::WHITE, 0.25)
                } else {
                    palette.primary
                },
            );
        })
        .named("timeline.scrub")
        .at(rect)
        .on_pointer(seek(bar, frames, frame))
}

fn seek(bar: Rect, frames: u32, frame: u32) -> impl Fn(Pointer) -> Option<Message> + 'static {
    move |pointer| {
        if frames <= 1 {
            return None;
        }
        let value = ((pointer.position.x - bar.min.x) / bar.width().max(1.0)).clamp(0.0, 1.0);
        let chosen = (value * (frames - 1) as f32).round() as u32;
        (chosen != frame).then_some(Message::Command(Command::SetTime(chosen)))
    }
}
