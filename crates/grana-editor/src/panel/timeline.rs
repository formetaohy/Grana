use glam::Vec2;
use grana_session::Command;
use grana_ui::{
    Align, Border, BuildContext, Color, Corners, Element, Fill, Icon, Key, Length, Motion, Pointer,
    Rect, Role, Shadow, State, TextStyle,
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
    let center = rect.center().y;
    let left = rect.min.x + 8.0;
    let back = Rect::from_center_size(Vec2::new(left + 13.0, center), Vec2::splat(26.0));
    let play = Rect::from_center_size(Vec2::new(left + 44.0, center), Vec2::splat(28.0));
    let forward = Rect::from_center_size(Vec2::new(left + 75.0, center), Vec2::splat(26.0));
    let track_start = forward.max.x + 14.0;
    let readout_width = 142.0;
    let track_end = (rect.max.x - 12.0 - readout_width).max(track_start + 72.0);
    let track = Rect::from_min_max(
        Vec2::new(track_start, center - 12.0),
        Vec2::new(track_end, center + 12.0),
    );
    let frame = state.scene.time();
    let frames = state.scene.frame_count();
    let playing = state.workspace.playback.playing;
    let language = ctx.language();
    let back_element = crate::view::icon_button(
        ctx,
        "timeline.back",
        Icon::StepBack,
        26.0,
        Role::Quiet,
        grana_i18n::timeline::previous_frame(language),
        Message::Command(Command::SetTime(frame.saturating_sub(1))),
    )
    .at(back);
    let play_element = crate::view::icon_button(
        ctx,
        "timeline.play",
        if playing { Icon::Pause } else { Icon::Play },
        28.0,
        Role::Prominent,
        if playing {
            grana_i18n::timeline::pause(language)
        } else {
            grana_i18n::timeline::play(language)
        },
        Message::SetPlaying(!playing),
    )
    .at(play);
    let forward_element = crate::view::icon_button(
        ctx,
        "timeline.forward",
        Icon::StepForward,
        26.0,
        Role::Quiet,
        grana_i18n::timeline::next_frame(language),
        Message::Command(Command::SetTime((frame + 1) % frames.max(1))),
    )
    .at(forward);
    let scrub = scrub_track(ctx, track, state);
    let seconds = frame as f32 * state.scene.dataset.frame_interval();
    let readout = format!("{:02} / {:02}  ·  {seconds:.2} s", frame + 1, frames);
    let readout_rect = Rect::from_min_max(
        Vec2::new(track_end + 10.0, rect.min.y),
        Vec2::new(rect.max.x - 12.0, rect.max.y),
    );
    let font = theme.font(grana_ui::Level::Monospace);
    let readout_element = grana_ui::canvas(
        Length::Fixed(readout_rect.width()),
        readout_rect.height(),
        move |painter, info| {
            painter.text_in(
                info.rect,
                &readout,
                TextStyle::new(font, palette.text_secondary).aligned(Align::End),
                Align::Center,
            );
        },
    )
    .at(readout_rect);
    let surface = theme.surface(Role::Hud, State::Rest);
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
    let palette = ctx.theme().palette;
    let frame = state.scene.time();
    let frames = state.scene.frame_count();
    let bar = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 4.0));
    let position = if frames > 1 {
        ctx.animate(
            Key::new("timeline.spring").id(),
            (frame as f32 / (frames - 1) as f32).clamp(0.0, 1.0),
            Motion::snappy(),
        )
    } else {
        0.0
    };
    let knob_x = bar.min.x + bar.width() * position;
    let fill_end = knob_x.clamp(bar.min.x, bar.max.x);
    let ticks = frames > 1 && frames <= 40 && bar.width() / (frames - 1) as f32 >= 9.0;
    grana_ui::canvas(
        Length::Fixed(rect.width()),
        rect.height(),
        move |painter, info| {
            let engaged = info.hover.max(info.press);
            painter.rect(bar, Corners::all(2.0), palette.track);
            if fill_end > bar.min.x {
                painter.gradient(
                    Rect::from_min_max(bar.min, Vec2::new(fill_end, bar.max.y)),
                    Corners::all(2.0),
                    palette.accent_hover,
                    palette.accent,
                );
            }
            if ticks {
                let spacing = bar.width() / (frames - 1) as f32;
                for index in 0..frames {
                    let x = bar.min.x + spacing * index as f32;
                    painter.rect(
                        Rect::from_center_size(
                            Vec2::new(x, rect.center().y + 8.5),
                            Vec2::new(1.0, 2.5),
                        ),
                        Corners::all(0.5),
                        if index == frame {
                            palette.text_secondary
                        } else {
                            palette.text_quaternary
                        },
                    );
                }
            }
            let knob = Rect::from_center_size(
                Vec2::new(knob_x, bar.center().y),
                Vec2::new(11.0 + engaged * 1.5, 15.0 + engaged * 1.5),
            );
            painter.outlined(
                knob,
                Corners::all(3.5),
                Fill::Gradient(Color::bytes(245, 247, 250), Color::bytes(210, 214, 222)),
                Border {
                    width: 1.0,
                    color: if info.press > 0.5 {
                        palette.accent
                    } else {
                        Color::white_alpha(0.9)
                    },
                },
                Shadow::new(Color::black_alpha(0.45), 4.0, 1.0),
            );
        },
    )
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
