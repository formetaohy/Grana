use glam::Vec2;
use grana_ui::glass::GlassStyle;
use grana_ui::{Align, Corners, Icon, Level, Motion, Rect, Role, TextStyle, Ui};

use crate::state::State;
use crate::widget;
use grana_session::Command;

const KNOB: f32 = 22.0;

pub(super) fn transport(
    ui: &mut Ui,
    rect: Rect,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
) {
    ui.glass(ui.id("transport.glass"), rect, GlassStyle::transport());
    let palette = ui.theme().palette;
    let center = rect.center().y;
    let left = rect.min.x + 12.0;
    let back = Rect::from_center_size(Vec2::new(left + 14.0, center), Vec2::splat(30.0));
    let play = Rect::from_center_size(Vec2::new(left + 50.0, center), Vec2::splat(30.0));
    let forward = Rect::from_center_size(Vec2::new(left + 84.0, center), Vec2::splat(30.0));
    let track_start = forward.max.x + 20.0;
    let readout_width = 160.0;
    let track_end = (rect.max.x - 18.0 - readout_width).max(track_start + 80.0);
    let track = Rect::from_min_max(
        Vec2::new(track_start, center - 14.0),
        Vec2::new(track_end, center + 14.0),
    );
    ui.place(back, |ui| {
        if widget::icon_button(
            ui,
            "timeline.back",
            Icon::StepBack,
            30.0,
            Role::Quiet,
            "Previous frame",
        ) {
            commands.push(Command::SetTime(state.scene.time().saturating_sub(1)));
        }
    });
    ui.place(play, |ui| {
        let symbol = if state.workspace.playback.playing {
            Icon::Pause
        } else {
            Icon::Play
        };
        let tooltip = if state.workspace.playback.playing {
            "Pause"
        } else {
            "Play"
        };
        if widget::icon_button(ui, "timeline.play", symbol, 32.0, Role::Prominent, tooltip) {
            state.workspace.playback.playing = !state.workspace.playback.playing;
            state.workspace.playback.previewing = false;
        }
    });
    ui.place(forward, |ui| {
        if widget::icon_button(
            ui,
            "timeline.forward",
            Icon::StepForward,
            30.0,
            Role::Quiet,
            "Next frame",
        ) {
            commands.push(Command::SetTime(
                (state.scene.time() + 1) % state.scene.frame_count(),
            ));
        }
    });
    if let Some(frame) = scrub(ui, track, state.scene.time(), state.scene.frame_count())
        && frame != state.scene.time()
    {
        commands.push(Command::SetTime(frame));
    }
    let frames = state.scene.frame_count();
    let time = state.scene.time();
    let seconds = time as f32 * state.scene.dataset.frame_interval();
    let readout = format!("{:02} / {:02}   {seconds:.2} s", time + 1, frames);
    let readout_rect = Rect::from_min_max(
        Vec2::new(track_end + 18.0, rect.min.y),
        Vec2::new(rect.max.x - 20.0, rect.max.y),
    );
    let font = ui.theme().font(Level::Monospace);
    let mut painter = ui.painter();
    painter.text_in(
        readout_rect,
        &readout,
        TextStyle::new(font, palette.text_secondary).aligned(Align::End),
        Align::Center,
    );
}

fn scrub(ui: &mut Ui, rect: Rect, frame: u32, frames: u32) -> Option<u32> {
    if frames <= 1 {
        return None;
    }
    let palette = ui.theme().palette;
    let response = ui.interact(ui.id("timeline.track"), rect);
    let bar = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 4.0));
    let normalize = |frame: f32| (frame / (frames - 1) as f32).clamp(0.0, 1.0);
    let mut chosen = None;
    if response.active {
        let pointer = ui.input().position().x;
        let value = ((pointer - bar.min.x) / bar.width().max(1.0)).clamp(0.0, 1.0);
        chosen = Some((value * (frames - 1) as f32).round() as u32);
    }
    let position = ui.spring_to(
        ui.id("timeline.spring"),
        normalize(frame as f32),
        Motion::response(0.3, 0.18),
    );
    let engaged = ui.animate(
        ui.id("timeline.grow"),
        if response.hovered || response.active {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    let travel = (bar.width() - KNOB).max(0.0);
    let knob_x = bar.min.x + KNOB * 0.5 + travel * position;
    let knob_size = Vec2::new(KNOB * 0.58 + engaged * 1.6, KNOB + engaged * 4.0);
    {
        let mut painter = ui.painter();
        painter.rect(bar, Corners::capsule(), palette.material_fill.amplify(0.9));
        painter.gradient(
            Rect::from_min_max(
                bar.min,
                Vec2::new((knob_x - 2.0).max(bar.min.x + 2.0), bar.max.y),
            ),
            Corners::capsule(),
            palette.accent_hover,
            palette.accent,
        );
    }
    let knob = Rect::from_center_size(Vec2::new(knob_x, bar.center().y), knob_size);
    ui.glass(ui.id("timeline.knob"), knob, GlassStyle::lens());
    if frames <= 40 {
        let spacing = bar.width() / (frames - 1) as f32;
        if spacing >= 9.0 {
            let mut painter = ui.painter();
            for index in 0..frames {
                let x = bar.min.x + spacing * index as f32;
                painter.rect(
                    Rect::from_center_size(
                        Vec2::new(x, rect.center().y + 10.0),
                        Vec2::new(1.0, 2.0),
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
    }
    chosen
}
