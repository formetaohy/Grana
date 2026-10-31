use egui::{RichText, Ui};

use crate::action::Action;
use crate::state::State;

const WIDTHS: [u32; 4] = [1280, 1920, 2560, 3840];

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let animation = &mut state.animation;
    let busy = animation.movie.is_some();
    let duration = animation.timeline.duration();
    let position = animation.position.clamp(0.0, duration);
    ui.horizontal(|ui| {
        let label = if animation.previewing {
            "Pause preview"
        } else {
            "Preview"
        };
        if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
            actions.push(Action::ToggleAnimation);
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Set keyframe"))
            .clicked()
        {
            actions.push(Action::SetKeyframe);
        }
        if ui
            .add_enabled(!animation.timeline.is_empty(), egui::Button::new("Clear"))
            .clicked()
        {
            actions.push(Action::ClearKeyframes);
        }
        ui.label(
            RichText::new(format!("{} keyframes", animation.timeline.keys().len()))
                .small()
                .weak(),
        );
    });
    ui.horizontal(|ui| {
        let mut playhead = position;
        let width = (ui.available_width() - 226.0).max(120.0);
        let slider = egui::Slider::new(&mut playhead, 0.0..=duration).show_value(false);
        if ui.add_sized([width, 18.0], slider).changed() {
            actions.push(Action::ScrubAnimation(playhead));
        }
        let frame = animation
            .timeline
            .sample(position)
            .map_or(0, |sample| sample.time);
        ui.monospace(format!(
            "{position:>5.2} / {duration:.2} s · frame {}",
            frame + 1
        ));
    });
    ui.horizontal(|ui| {
        let mut fps = animation.timeline.fps();
        if ui
            .add(
                egui::DragValue::new(&mut fps)
                    .range(1..=grana_animation::Timeline::MAX_FPS)
                    .prefix("fps "),
            )
            .changed()
        {
            animation.timeline.set_fps(fps);
        }
        let mut length = duration;
        if ui
            .add(
                egui::DragValue::new(&mut length)
                    .range(0.5..=600.0)
                    .speed(0.1)
                    .max_decimals(2)
                    .prefix("length ")
                    .suffix(" s"),
            )
            .changed()
        {
            animation.timeline.set_duration(length);
        }
        ui.label(
            RichText::new(format!(
                "{} frames · {:.1} s of data",
                animation.timeline.frames(),
                animation.frames as f32 * animation.frame_interval
            ))
            .small()
            .weak(),
        );
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.strong("Keyframes");
        ui.label(
            RichText::new("the playhead holds the pose the timeline samples")
                .small()
                .weak(),
        );
    });
    egui::ScrollArea::vertical()
        .id_salt("grana.keyframes")
        .max_height(104.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if animation.timeline.is_empty() {
                ui.label(
                    RichText::new(
                        "set a keyframe to record the camera, the section plane and the time point",
                    )
                    .small()
                    .weak(),
                );
            }
            for (index, key) in animation.timeline.keys().iter().enumerate() {
                ui.horizontal(|ui| {
                    let selected = animation.selected == Some(index);
                    let label = format!(
                        "{:>6.2} s · frame {:>3} · yaw {:>5.2} · distance {:>7.2}",
                        key.at,
                        key.time + 1,
                        key.camera.yaw,
                        key.camera.distance
                    );
                    if ui.selectable_label(selected, label).clicked() {
                        actions.push(Action::SelectKeyframe(index));
                    }
                    if ui.small_button("Delete").clicked() {
                        actions.push(Action::DeleteKeyframe(index));
                    }
                });
            }
        });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Movie width");
        for width in WIDTHS {
            if ui
                .selectable_label(animation.width == width, width.to_string())
                .clicked()
            {
                actions.push(Action::SetMovieWidth(width));
            }
        }
        ui.monospace(format!("{} × {}", animation.size[0], animation.size[1]));
    });
    ui.horizontal(|ui| match animation.movie {
        Some((frame, frames)) => {
            let progress = frame as f32 / frames.max(1) as f32;
            ui.add(
                egui::ProgressBar::new(progress)
                    .desired_width(240.0)
                    .text(format!("{frame} / {frames}")),
            );
            if ui.button("Cancel").clicked() {
                actions.push(Action::CancelMovie);
            }
        }
        None => {
            if ui
                .add_enabled(
                    !animation.timeline.is_empty(),
                    egui::Button::new("Export movie"),
                )
                .clicked()
            {
                actions.push(Action::ExportMovie);
            }
            ui.label(RichText::new("h264 mp4 of the 3d viewport").small().weak());
        }
    });
    if let Some(message) = animation.message {
        ui.label(RichText::new(message).small());
    }
    ui.label(
        RichText::new(
            "keyframes carry the camera, the section plane and the time point; motion eases in and out of every keyframe",
        )
        .small()
        .weak(),
    );
}
