use glam::Vec2;
use grana_ui::{Align, Color, Corners, Icon, Motion, Rect, Role, TextStyle, TypeStyle, Ui};

use crate::state::State;
use crate::widget;
use grana_session::Command;

pub(super) fn transport(
    ui: &mut Ui,
    rect: Rect,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
) {
    let palette = ui.theme().palette;
    let center = rect.center().y;
    let left = rect.min.x + 14.0;
    let readout_width = 136.0;
    let back = Rect::from_min_size(Vec2::new(left, center - 14.0), Vec2::splat(28.0));
    let play = Rect::from_min_size(Vec2::new(left + 32.0, center - 14.0), Vec2::splat(28.0));
    let forward = Rect::from_min_size(Vec2::new(left + 64.0, center - 14.0), Vec2::splat(28.0));
    let track_start = forward.max.x + 20.0;
    let track_end = (rect.max.x - 14.0 - readout_width).max(track_start + 80.0);
    let track = Rect::from_min_max(
        Vec2::new(track_start, center - 12.0),
        Vec2::new(track_end, center + 12.0),
    );
    ui.place(back, |ui| {
        if widget::icon_button(
            ui,
            "timeline.back",
            Icon::StepBack,
            28.0,
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
        if widget::icon_button(ui, "timeline.play", symbol, 28.0, Role::Prominent, tooltip) {
            state.workspace.playback.playing = !state.workspace.playback.playing;
            state.workspace.playback.previewing = false;
        }
    });
    ui.place(forward, |ui| {
        if widget::icon_button(
            ui,
            "timeline.forward",
            Icon::StepForward,
            28.0,
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
    let readout = format!("{:02} / {:02}   ·   {seconds:.2} s", time + 1, frames);
    let readout_rect = Rect::from_min_max(
        Vec2::new(track_end + 16.0, rect.min.y),
        Vec2::new(rect.max.x - 14.0, rect.max.y),
    );
    let mut painter = ui.painter();
    painter.text_in(
        readout_rect,
        &readout,
        TextStyle::new(TypeStyle::new(11.0, 400.0, 14.0), palette.text_tertiary)
            .aligned(Align::End),
        Align::Center,
    );
}

fn scrub(ui: &mut Ui, rect: Rect, frame: u32, frames: u32) -> Option<u32> {
    if frames <= 1 {
        return None;
    }
    let palette = ui.theme().palette;
    let response = ui.interact(ui.id("timeline.track"), rect);
    let bar = Rect::from_min_max(
        Vec2::new(rect.min.x, rect.center().y - 2.0),
        Vec2::new(rect.max.x, rect.center().y + 2.0),
    );
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
        Motion::quick(),
    );
    let knob = Vec2::new(bar.min.x + bar.width() * position, bar.center().y);
    let grow = ui.animate(
        ui.id("timeline.grow"),
        if response.hovered || response.active {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    let mut painter = ui.painter();
    painter.rect(bar, Corners::capsule(), palette.border);
    painter.rect(
        Rect::from_min_max(bar.min, Vec2::new(knob.x.max(bar.min.x + 4.0), bar.max.y)),
        Corners::capsule(),
        palette.accent,
    );
    let spacing = bar.width() / (frames - 1) as f32;
    if frames <= 40 && spacing >= 8.0 {
        for index in 0..frames {
            let x = bar.min.x + spacing * index as f32;
            painter.rect(
                Rect::from_center_size(Vec2::new(x, rect.center().y + 8.0), Vec2::new(1.0, 2.0)),
                Corners::capsule(),
                Color::white_alpha(if index == frame { 0.35 } else { 0.12 }),
            );
        }
    }
    let size = 10.0 + grow;
    let knob_rect = Rect::from_center_size(knob, Vec2::splat(size));
    painter.rect(knob_rect, Corners::capsule(), palette.scene_backdrop);
    painter.outlined(
        knob_rect,
        Corners::capsule(),
        grana_ui::Fill::None,
        grana_ui::Border {
            width: 1.5,
            color: palette.accent,
        },
        grana_ui::Shadow::NONE,
    );
    chosen
}
