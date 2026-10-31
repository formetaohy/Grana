use glam::Vec2;
use grana_ui::{Align, Axis, Block, Color, Length, Rect, Role, TextStyle, Ui};

use crate::action::Action;
use crate::state::State;
use crate::widget;

const WIDTHS: [u32; 4] = [1280, 1920, 2560, 3840];

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let animation = &mut state.animation;
    let busy = animation.movie.is_some();
    let duration = animation.timeline.duration();
    let position = animation.position.clamp(0.0, duration);
    let previewing = animation.previewing;
    let keys = animation.timeline.keys().len();
    let empty = animation.timeline.is_empty();
    let width = animation.width;
    let size = animation.size;
    let movie = animation.movie;
    let message = animation.message.map(str::to_string);
    let frames = animation.frames;
    let frame_interval = animation.frame_interval;
    let fps = animation.timeline.fps();
    let timeline_frames = animation.timeline.frames();
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        let label = if previewing {
            "Pause preview"
        } else {
            "Preview"
        };
        if !busy && widget::button(ui, "preview", label, Role::Prominent) {
            actions.push(Action::ToggleAnimation);
        }
        if !busy && widget::button(ui, "key", "Set keyframe", Role::Control) {
            actions.push(Action::SetKeyframe);
        }
        if !empty && widget::button(ui, "clear", "Clear", Role::Quiet) {
            actions.push(Action::ClearKeyframes);
        }
        widget::caption(ui, &format!("{keys} keyframes"));
    });
    if let Some(value) = widget::slider(
        ui,
        "playhead",
        Some("time"),
        position,
        0.0..=duration.max(0.01),
        false,
    ) {
        actions.push(Action::ScrubAnimation(value));
    }
    let frame = animation
        .timeline
        .sample(position)
        .map_or(0, |sample| sample.time);
    widget::caption(
        ui,
        &format!("{position:>5.2} / {duration:.2} s · frame {}", frame + 1),
    );
    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
        widget::field_label(ui, "fps");
        if let Some(value) = widget::drag_value(ui, "fps", fps as f32, 0.5, 0) {
            animation
                .timeline
                .set_fps((value.round() as u32).clamp(1, grana_animation::Timeline::MAX_FPS));
        }
        widget::field_label(ui, "length");
        if let Some(value) = widget::drag_value(ui, "length", duration, 0.1, 2) {
            animation.timeline.set_duration(value.clamp(0.5, 600.0));
        }
        widget::caption(ui, "s");
        widget::caption(
            ui,
            &format!(
                "{timeline_frames} frames · {:.1} s of data",
                frames as f32 * frame_interval
            ),
        );
    });
    ui.separator();
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        widget::section_title(ui, "keyframes");
        widget::weak(ui, "the playhead holds the pose the timeline samples");
    });
    let mut chosen = None;
    let mut removed = None;
    let selected = animation.selected;
    ui.scroll_axis(
        ui.id("keyframes"),
        Axis::Column,
        Length::Fixed(104.0),
        None,
        |ui| {
            if empty {
                widget::weak(
                    ui,
                    "set a keyframe to record the camera, the section plane and the time point",
                );
            }
            for (index, key) in animation.timeline.keys().iter().enumerate() {
                let label = format!(
                    "{:>6.2} s · frame {:>3} · yaw {:>5.2} · distance {:>7.2}",
                    key.at,
                    key.time + 1,
                    key.camera.yaw,
                    key.camera.distance
                );
                ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                    if crate::object_panel::row_button(
                        ui,
                        &format!("key.{index}"),
                        &label,
                        selected == Some(index),
                    ) {
                        chosen = Some(index);
                    }
                    if widget::icon_button(
                        ui,
                        &format!("delete.{index}"),
                        "\u{00D7}",
                        18.0,
                        Role::Quiet,
                    ) {
                        removed = Some(index);
                    }
                });
            }
        },
    );
    if let Some(index) = chosen {
        actions.push(Action::SelectKeyframe(index));
    }
    if let Some(index) = removed {
        actions.push(Action::DeleteKeyframe(index));
    }
    ui.separator();
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        widget::field_label(ui, "movie width");
        let options = WIDTHS
            .iter()
            .map(|width| (*width, width.to_string()))
            .collect::<Vec<_>>();
        if let Some(value) =
            grana_ui::popover::combo(ui, "movie.width", 96.0, &options, Some(width))
        {
            actions.push(Action::SetMovieWidth(value));
        }
        let style = TextStyle::new(
            ui.theme().font(grana_ui::Level::Monospace),
            ui.theme().palette.text_secondary,
        );
        ui.text_style(&format!("{} × {}", size[0], size[1]), style);
    });
    match movie {
        Some((frame, frames)) => {
            let progress = frame as f32 / frames.max(1) as f32;
            ui.row(Block::row().gaps(10.0).align(Align::Center), |ui| {
                widget::progress(ui, progress, 240.0);
                widget::caption(ui, &format!("{frame} / {frames}"));
                if widget::button(ui, "cancel", "Cancel", Role::Quiet) {
                    actions.push(Action::CancelMovie);
                }
            });
        }
        None => {
            ui.row(Block::row().gaps(10.0).align(Align::Center), |ui| {
                if !empty && !busy && widget::button(ui, "export", "Export movie", Role::Control) {
                    actions.push(Action::ExportMovie);
                }
                widget::weak(ui, "h264 mp4 of the 3d viewport");
            });
        }
    }
    if let Some(message) = message {
        widget::caption(ui, &message);
    }
    widget::weak(
        ui,
        "keyframes carry the camera, the section plane and the time point; motion eases in and out of every keyframe",
    );
    let _ = Vec2::ZERO;
    let _ = Color::TRANSPARENT;
    let _ = Rect::ZERO;
}
