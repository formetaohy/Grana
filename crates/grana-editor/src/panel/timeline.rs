use glam::Vec2;
use grana_ui::{
    Align, Block, Color, Corners, Edges, Icon, Length, Motion, Rect, Role, Shadow, TextStyle,
    TypeStyle, Ui, icon,
};

use crate::state::State;
use crate::widget;
use grana_session::Command;

const TIMELINE_WIDTH: f32 = 520.0;
const TIMELINE_HEIGHT: f32 = 50.0;

pub(super) fn timeline_rect(volume: Rect) -> Rect {
    let width = TIMELINE_WIDTH.min(volume.width() - 56.0).max(320.0);
    Rect::from_min_size(
        Vec2::new(
            volume.center().x - width * 0.5,
            volume.max.y - 16.0 - TIMELINE_HEIGHT,
        ),
        Vec2::new(width, TIMELINE_HEIGHT),
    )
}

pub(super) fn timeline_bar(
    ui: &mut Ui,
    rect: Rect,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
) {
    let look = ui.theme().look(Role::Transport, grana_ui::State::Rest);
    ui.stacked(rect, false, |ui| {
        {
            let mut painter = ui.painter();
            painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        }
        ui.row(
            Block::row()
                .padding(Edges::symmetric(10.0, 0.0))
                .gaps(6.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                if widget::icon_button(
                    ui,
                    "previous",
                    Icon::StepBack,
                    26.0,
                    Role::Quiet,
                    "Previous frame",
                ) {
                    commands.push(Command::SetTime(state.scene.time().saturating_sub(1)));
                }
                if play_button(ui, state.workspace.playback.playing) {
                    state.workspace.playback.playing = !state.workspace.playback.playing;
                }
                if widget::icon_button(
                    ui,
                    "next",
                    Icon::StepForward,
                    26.0,
                    Role::Quiet,
                    "Next frame",
                ) {
                    commands.push(Command::SetTime(
                        (state.scene.time() + 1) % state.scene.frame_count(),
                    ));
                }
                ui.space(4.0);
                if let Some(frame) = time_bar(ui, state.scene.time(), state.scene.frame_count()) {
                    commands.push(Command::SetTime(frame));
                }
                time_readout(ui, state);
            },
        );
    });
}

fn play_button(ui: &mut Ui, playing: bool) -> bool {
    let key = ui.id("play");
    let rect = ui.allocate(Vec2::splat(32.0));
    let response = ui.interact(key, rect);
    let hover = ui.animate(
        key,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let press = ui.animate(
        key,
        if response.active { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let circle = Rect::from_center_size(rect.center(), Vec2::splat(30.0 - press * 1.2));
    let mut painter = ui.painter();
    painter.shadow(
        circle,
        Corners::capsule(),
        Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.30), 14.0, 3.0),
    );
    painter.rect(
        circle,
        Corners::capsule(),
        Color::WHITE.lerp(Color::rgba(0.94, 0.96, 1.0, 1.0), hover),
    );
    let symbol = if playing { Icon::Pause } else { Icon::Play };
    icon::paint(
        symbol,
        &mut painter,
        rect.center(),
        14.0,
        Color::rgb(0.03, 0.04, 0.06),
        1.6,
    );
    response.clicked
}

fn time_bar(ui: &mut Ui, frame: u32, frames: u32) -> Option<u32> {
    if frames <= 1 {
        return None;
    }
    let width = (ui.available().x - 104.0).max(80.0);
    let rect = ui.allocate(Vec2::new(width, 28.0));
    let response = ui.interact(ui.id("time"), rect);
    let bar = Rect::from_min_max(
        Vec2::new(rect.min.x + 8.0, rect.center().y - 2.0),
        Vec2::new(rect.max.x - 8.0, rect.center().y + 2.0),
    );
    let normalize = |frame: f32| (frame / (frames - 1) as f32).clamp(0.0, 1.0);
    let mut chosen = None;
    if response.active {
        let pointer = ui.input().position().x;
        let value = ((pointer - bar.min.x) / bar.width().max(1.0)).clamp(0.0, 1.0);
        chosen = Some((value * (frames - 1) as f32).round() as u32);
    }
    let position = ui.spring_to(
        ui.id("time.spring"),
        normalize(frame as f32),
        Motion::quick(),
    );
    let knob = Vec2::new(bar.min.x + bar.width() * position, bar.center().y);
    let grow = ui.animate(
        ui.id("time.grow"),
        if response.hovered || response.active {
            1.0
        } else {
            0.0
        },
        Motion::quick(),
    );
    let accent = ui.theme().palette.accent;
    let mut painter = ui.painter();
    painter.rect(bar, Corners::capsule(), Color::white_alpha(0.16));
    painter.rect(
        Rect::from_min_max(bar.min, Vec2::new(knob.x.max(bar.min.x + 4.0), bar.max.y)),
        Corners::capsule(),
        accent,
    );
    if frames <= 48 {
        for index in 0..frames {
            let x = bar.min.x + bar.width() * index as f32 / (frames - 1) as f32;
            painter.rect(
                Rect::from_center_size(Vec2::new(x, bar.max.y + 5.0), Vec2::new(1.0, 2.0)),
                Corners::capsule(),
                Color::white_alpha(if index == frame { 0.55 } else { 0.18 }),
            );
        }
    }
    painter.shadow(
        Rect::from_center_size(knob, Vec2::splat(12.0 + grow)),
        Corners::capsule(),
        Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.30), 10.0, 2.0),
    );
    painter.rect(
        Rect::from_center_size(knob, Vec2::splat(11.0 + grow)),
        Corners::capsule(),
        Color::WHITE,
    );
    chosen
}

fn time_readout(ui: &mut Ui, state: &State<'_>) {
    let frames = state.scene.frame_count();
    let time = state.scene.time();
    let seconds = time as f32 * state.scene.dataset.frame_interval();
    let text = format!("{:02} / {:02} · {seconds:.1}s", time + 1, frames);
    let style = TextStyle::new(
        TypeStyle::new(10.5, 470.0, 13.0),
        ui.theme().palette.text_secondary,
    )
    .aligned(Align::End);
    let layout = ui.layout_text(&text, style, 0.0);
    let rect = ui.allocate(Vec2::new(layout.size.x.max(96.0), 28.0));
    let mut painter = ui.painter();
    painter.text_in(rect, &text, style, Align::Center);
}
