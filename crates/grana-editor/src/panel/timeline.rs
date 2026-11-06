use glam::Vec2;
use grana_session::Command;
use grana_ui::glass::GlassStyle;
use grana_ui::{
    Align, BuildContext, Corners, Element, Icon, Key, Length, Motion, Pointer, Rect, Role,
    TextStyle,
};

use crate::message::Message;
use crate::state::View;

const KNOB: f32 = 20.0;

pub(super) fn transport<'a>(
    ctx: &mut BuildContext<'_, Message>,
    rect: Rect,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let center = rect.center().y;
    let left = rect.min.x + 12.0;
    let back = Rect::from_center_size(Vec2::new(left + 14.0, center), Vec2::splat(28.0));
    let play = Rect::from_center_size(Vec2::new(left + 48.0, center), Vec2::splat(30.0));
    let forward = Rect::from_center_size(Vec2::new(left + 82.0, center), Vec2::splat(28.0));
    let track_start = forward.max.x + 18.0;
    let readout_width = 148.0;
    let track_end = (rect.max.x - 16.0 - readout_width).max(track_start + 80.0);
    let track = Rect::from_min_max(
        Vec2::new(track_start, center - 14.0),
        Vec2::new(track_end, center + 14.0),
    );
    let frame = state.scene.time();
    let frames = state.scene.frame_count();
    let playing = state.workspace.playback.playing;
    let back_element = crate::view::icon_button(
        ctx,
        "timeline.back",
        Icon::StepBack,
        28.0,
        Role::Quiet,
        "Previous frame",
        Message::Command(Command::SetTime(frame.saturating_sub(1))),
    )
    .at(back);
    let play_element = crate::view::icon_button(
        ctx,
        "timeline.play",
        if playing { Icon::Pause } else { Icon::Play },
        30.0,
        Role::Prominent,
        if playing { "Pause" } else { "Play" },
        Message::SetPlaying(!playing),
    )
    .at(play);
    let forward_element = crate::view::icon_button(
        ctx,
        "timeline.forward",
        Icon::StepForward,
        28.0,
        Role::Quiet,
        "Next frame",
        Message::Command(Command::SetTime((frame + 1) % frames.max(1))),
    )
    .at(forward);
    let scrub = scrub_track(ctx, track, state);
    let seconds = frame as f32 * state.scene.dataset.frame_interval();
    let readout = format!("{:02} / {:02}  ·  {seconds:.2} s", frame + 1, frames);
    let readout_rect = Rect::from_min_max(
        Vec2::new(track_end + 12.0, rect.min.y),
        Vec2::new(rect.max.x - 16.0, rect.max.y),
    );
    let font = ctx.theme().font(grana_ui::Level::Monospace);
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
    Element::new(grana_ui::Layout::column())
        .named("transport")
        .at(rect)
        .glass(GlassStyle::transport())
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
            Motion::response(0.3, 0.18),
        )
    } else {
        0.0
    };
    let travel = (bar.width() - KNOB).max(0.0);
    let knob_x = bar.min.x + KNOB * 0.5 + travel * position;
    let knob = Rect::from_center_size(
        Vec2::new(knob_x, bar.center().y),
        Vec2::new(KNOB * 0.72, KNOB * 0.82),
    );
    let fill_end =
        (knob.min.x + position.clamp(0.0, 1.0) * knob.width()).clamp(bar.min.x, bar.max.x);
    let ticks = frames > 1 && frames <= 40 && bar.width() / (frames - 1) as f32 >= 9.0;
    let track = grana_ui::canvas(
        Length::Fixed(rect.width()),
        rect.height(),
        move |painter, _info| {
            painter.rect(bar, Corners::capsule(), palette.material_fill.amplify(0.9));
            if fill_end > bar.min.x {
                painter.gradient(
                    Rect::from_min_max(bar.min, Vec2::new(fill_end, bar.max.y)),
                    Corners::capsule(),
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
                            Vec2::new(x, rect.center().y + 10.0),
                            Vec2::new(1.0, 2.5),
                        ),
                        Corners::capsule(),
                        if index == frame {
                            palette.text_secondary
                        } else {
                            palette.text_quaternary
                        },
                    );
                }
            }
        },
    )
    .at(rect)
    .on_pointer(seek(bar, frames, frame));
    let knob_element = grana_ui::canvas(Length::Fixed(knob.width()), knob.height(), |_, _| {})
        .named("timeline.knob")
        .at(knob)
        .glass(GlassStyle::knob())
        .on_pointer(seek(bar, frames, frame));
    grana_ui::place(rect, [track, knob_element]).named("timeline.scrub")
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
