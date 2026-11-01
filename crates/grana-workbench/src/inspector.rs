use glam::Vec2;
use grana_scene::Scene;
use grana_ui::{Align, Block, Color, Corners, Length, Rect, Role, TextStyle, Ui};
use grana_volume::Axis;

use crate::action::Action;
use crate::state::State;
use crate::widget;

const CHANNEL_ROW: f32 = 32.0;

fn heading(ui: &mut Ui, title: &str) {
    let rect = ui.allocate_full(22.0);
    let style = TextStyle::new(
        ui.theme().font(grana_ui::Level::Headline),
        ui.theme().palette.text_primary,
    );
    let mut painter = ui.painter();
    painter.text_in(rect, title, style, Align::Center);
}

pub fn header(ui: &mut Ui, state: &State<'_>) {
    let dataset = &state.scene.dataset;
    let name = dataset.name();
    let rect = ui.allocate_full(24.0);
    let style = TextStyle::new(
        ui.theme().font(grana_ui::Level::Title),
        ui.theme().palette.text_primary,
    )
    .truncating();
    {
        let mut painter = ui.painter();
        painter.text_in(rect, name, style, Align::Center);
    }
    let extent = dataset.extent();
    let spacing = dataset.spacing();
    let channels = dataset.channels().len();
    let frames = dataset.frame_count();
    let meta = format!(
        "{} x {} x {} · {:.3} um · {} channel{} · {} frame{}",
        extent.x(),
        extent.y(),
        extent.z(),
        spacing.x(),
        channels,
        plural(channels),
        frames,
        plural(frames as usize)
    );
    widget::caption(ui, &meta);
    let source = match dataset.origin() {
        grana_scene::Origin::Synthetic => "synthetic",
        grana_scene::Origin::Zarr => "OME-Zarr",
    };
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        widget::badge(ui, source, ui.theme().palette.accent);
        if let Some(project) = state.project {
            let marker = if state.dirty { " · edited" } else { "" };
            widget::caption(ui, &format!("{project}{marker}"));
        } else {
            widget::caption(ui, "untitled project");
        }
    });
    if let Some(message) = state.message {
        let style = TextStyle::new(
            ui.theme().font(grana_ui::Level::Caption),
            Color::rgb(1.0, 0.53, 0.44),
        );
        ui.text_style_wrapped(message.text(), style, ui.available().x);
    }
    if let Some(name) = state.loading {
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            widget::spinner(ui, 13.0);
            widget::caption(ui, name);
        });
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

pub fn channels(ui: &mut Ui, scene: &mut Scene) {
    heading(ui, "Channels");
    let selected = scene.selected;
    let count = scene.channels.len();
    let mut chosen = None;
    for index in 0..count {
        let info = &scene.dataset.channels()[index];
        let name = info.name().to_string();
        let tint = info.tint();
        let enabled = scene.channels[index].enabled;
        ui.push_id(&format!("channel.{index}"));
        let row = ui.allocate_full(CHANNEL_ROW);
        let response = ui.interact(ui.id("pick"), row);
        if response.clicked {
            chosen = Some(index);
        }
        let hover = ui.animate(
            ui.id("hover"),
            if response.hovered { 1.0 } else { 0.0 },
            grana_ui::Motion::quick(),
        );
        let active = selected == index;
        let accent = ui.theme().palette.accent;
        let primary = ui.theme().palette.text_primary;
        let secondary = ui.theme().palette.text_secondary;
        let body = ui.theme().font(grana_ui::Level::Body);
        {
            let mut painter = ui.painter();
            if active {
                painter.rect(row, Corners::all(9.0), accent.with_alpha(0.20));
            } else if hover > 0.01 {
                painter.rect(row, Corners::all(9.0), Color::white_alpha(0.05 * hover));
            }
            painter.rect(
                Rect::from_center_size(
                    Vec2::new(row.min.x + 12.0, row.center().y),
                    Vec2::splat(10.0),
                ),
                Corners::capsule(),
                Color::from_rgb_f32(tint).with_alpha(if enabled { 1.0 } else { 0.35 }),
            );
            let style = TextStyle::new(body, if active { primary } else { secondary });
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(row.min.x + 26.0, row.min.y),
                    Vec2::new(row.max.x - 52.0, row.max.y),
                ),
                &name,
                style,
                Align::Center,
            );
        }
        ui.place(
            Rect::from_center_size(
                Vec2::new(row.max.x - 24.0, row.center().y),
                Vec2::new(36.0, 20.0),
            ),
            |ui| {
                if let Some(value) = widget::toggle(ui, "enabled", enabled) {
                    scene.channels[index].enabled = value;
                }
            },
        );
        ui.pop_id();
    }
    if let Some(index) = chosen {
        scene.select_channel(index);
    }
    ui.space(2.0);
}

pub fn display(ui: &mut Ui, scene: &mut Scene) {
    heading(ui, "Display");
    let index = scene.selected;
    let name = scene.channel(index).name().to_string();
    let data = scene.channel(index).intensity();
    widget::caption(
        ui,
        &format!("{name} · data {:.0}..{:.0}", data.min(), data.max()),
    );
    let current = scene.channels[index].window;
    let mut start = current.min();
    let mut end = current.max();
    let available = data.min()..=data.max();
    let mut changed = false;
    if let Some(value) = widget::slider(ui, "black", Some("black"), start, available.clone(), false)
    {
        start = value;
        changed = true;
    }
    if let Some(value) = widget::slider(ui, "white", Some("white"), end, available, false) {
        end = value;
        changed = true;
    }
    if changed {
        scene.set_window(index, start, end);
    }
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        if widget::button(ui, "auto", "Auto", Role::Quiet) {
            scene.reset_window(index);
        }
        let tint = scene.channel(index).tint();
        if widget::button(ui, "preset.tinted", "Tinted", Role::Quiet) {
            *scene.channels[index].transfer_mut() = grana_render::TransferFunction::tinted(tint);
        }
        if widget::button(ui, "preset.gray", "Grayscale", Role::Quiet) {
            *scene.channels[index].transfer_mut() = grana_render::TransferFunction::grayscale();
        }
    });
    let open = ui.disclosure_open("transfer", false);
    let window = scene.channels[index].window;
    let tint = scene.channel(index).tint();
    let next = widget::disclosure(ui, "transfer", "Transfer curve", open, |ui| {
        crate::transfer::edit(
            ui,
            "transfer",
            scene.channels[index].transfer_mut(),
            tint,
            window,
        );
    });
    ui.store_open("transfer", next);
    ui.space(2.0);
}

pub fn section(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    heading(ui, "Section");
    let axis = state.viewport.section().axis();
    let options = ["XY", "XZ", "YZ"];
    let current = match axis {
        Axis::Z => 0,
        Axis::Y => 1,
        Axis::X => 2,
    };
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        if let Some(index) = widget::segmented(ui, "axis", &options, current) {
            let target = match index {
                0 => Axis::Z,
                1 => Axis::Y,
                _ => Axis::X,
            };
            actions.push(Action::SetSectionAxis(target));
        }
        if widget::button(ui, "fit.section", "Fit", Role::Quiet) {
            actions.push(Action::FitSection);
        }
    });
    let extent = state.scene.dataset.world_size()[axis.index()];
    let mut position = state.viewport.camera().focus_along(axis) + 0.5 * extent;
    if let Some(value) = widget::slider(ui, "plane", Some("plane"), position, 0.0..=extent, false) {
        position = value;
        state
            .viewport
            .camera_mut()
            .set_focus_along(axis, position - 0.5 * extent);
    }
    let spacing = state.scene.dataset.spacing();
    widget::caption(
        ui,
        &format!(
            "{} plane · {:.3} um steps",
            axis.name(),
            axis.spacing(spacing)
        ),
    );
    ui.space(2.0);
}

pub fn more(ui: &mut Ui, state: &mut State<'_>) {
    let open = ui.disclosure_open("more", false);
    let next = widget::disclosure(ui, "more", "More", open, |ui| {
        region(ui, state);
        rendering(ui, state.scene);
        streaming(ui, state);
    });
    ui.store_open("more", next);
}

pub fn region(ui: &mut Ui, state: &mut State<'_>) {
    let open = ui.disclosure_open("region", false);
    let next = widget::disclosure(ui, "region", "Region", open, |ui| {
        let world = state.scene.dataset.world_size();
        let half = 0.5 * world;
        let mut region = state.scene.region;
        let mut enabled = region.enabled();
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if let Some(value) = widget::toggle(ui, "region.enabled", enabled) {
                enabled = value;
                region.set_enabled(value);
            }
            widget::caption(ui, "limit analysis and display");
        });
        let minimum = (0.001 * world.min_element()).max(1.0e-4);
        let mut low = region.min();
        let mut high = region.max();
        for (axis, name) in [(Axis::X, "X"), (Axis::Y, "Y"), (Axis::Z, "Z")] {
            let index = axis.index();
            let limit = half[index];
            let speed = (0.01 * world[index]).max(minimum);
            let mut start = low[index];
            let mut end = high[index];
            let mut changed = false;
            ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                widget::field_label(ui, name);
                if let Some(value) = widget::drag_value(ui, "low", start, speed, 3) {
                    start = value;
                    changed = true;
                }
                widget::caption(ui, "-");
                if let Some(value) = widget::drag_value(ui, "high", end, speed, 3) {
                    end = value;
                    changed = true;
                }
            });
            ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                if let Some(value) = widget::slider_sized(
                    ui,
                    "low.slider",
                    None,
                    start,
                    -limit..=limit,
                    false,
                    Length::Fill,
                ) {
                    start = value;
                    changed = true;
                }
                if let Some(value) = widget::slider_sized(
                    ui,
                    "high.slider",
                    None,
                    end,
                    -limit..=limit,
                    false,
                    Length::Fill,
                ) {
                    end = value;
                    changed = true;
                }
            });
            if changed {
                start = start.clamp(-limit, (end - minimum).max(-limit));
                end = end.clamp((start + minimum).min(limit), limit);
                low[index] = start;
                high[index] = end;
            }
        }
        if low != region.min() || high != region.max() {
            region.set_bounds(low, high);
        }
        let size = region.size();
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if widget::button(ui, "region.reset", "Reset", Role::Quiet) {
                region.set_bounds(-half, half);
            }
            if enabled {
                widget::caption(
                    ui,
                    &format!("{:.2} x {:.2} x {:.2} um", size.x, size.y, size.z),
                );
            } else {
                widget::caption(ui, "the whole dataset");
            }
        });
        state.scene.region = region;
    });
    ui.store_open("region", next);
}

pub fn rendering(ui: &mut Ui, scene: &mut Scene) {
    let open = ui.disclosure_open("rendering", false);
    let next = widget::disclosure(ui, "rendering", "Rendering", open, |ui| {
        let mut exposure = scene.exposure;
        if let Some(value) = widget::slider(
            ui,
            "exposure",
            Some("exposure"),
            exposure,
            0.05..=20.0,
            true,
        ) {
            exposure = value;
        }
        scene.exposure = exposure;
        let mut step = scene.step_scale;
        if let Some(value) = widget::slider(ui, "step", Some("sampling"), step, 0.25..=2.0, true) {
            step = value;
        }
        scene.step_scale = step;
        let mut shadows = scene.shading.shadows;
        if let Some(value) =
            widget::slider(ui, "shadows", Some("shadows"), shadows, 0.0..=1.0, false)
        {
            shadows = value;
        }
        scene.shading.shadows = shadows;
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if widget::button(ui, "reset.look", "Reset", Role::Quiet) {
                scene.shading = grana_render::Shading::default();
                scene.exposure = 1.0;
                scene.step_scale = 1.0;
            }
        });
        let open = ui.disclosure_open("lighting", false);
        let next = widget::disclosure(ui, "lighting", "Lighting", open, |ui| {
            let mut ambient = scene.shading.ambient;
            if let Some(value) =
                widget::slider(ui, "ambient", Some("ambient"), ambient, 0.0..=1.0, false)
            {
                ambient = value;
            }
            scene.shading.ambient = ambient;
            let mut diffuse = scene.shading.diffuse;
            if let Some(value) =
                widget::slider(ui, "diffuse", Some("diffuse"), diffuse, 0.0..=1.0, false)
            {
                diffuse = value;
            }
            scene.shading.diffuse = diffuse;
            let mut specular = scene.shading.specular;
            if let Some(value) =
                widget::slider(ui, "specular", Some("specular"), specular, 0.0..=1.0, false)
            {
                specular = value;
            }
            scene.shading.specular = specular;
            let mut shininess = scene.shading.shininess;
            if let Some(value) = widget::slider(
                ui,
                "shininess",
                Some("shininess"),
                shininess,
                1.0..=128.0,
                true,
            ) {
                shininess = value;
            }
            scene.shading.shininess = shininess;
        });
        ui.store_open("lighting", next);
    });
    ui.store_open("rendering", next);
}

pub fn streaming(ui: &mut Ui, state: &State<'_>) {
    let open = ui.disclosure_open("streaming", false);
    let next = widget::disclosure(ui, "streaming", "Streaming", open, |ui| {
        widget::labelled(ui, "volume", &bricks(state.frame.volume));
        widget::labelled(ui, "section", &bricks(state.frame.section));
        widget::labelled(
            ui,
            "cache",
            &format!(
                "{:.0} MiB bricks",
                state.frame.bytes as f64 / (1024.0 * 1024.0)
            ),
        );
        widget::labelled(
            ui,
            "geometry",
            &format!(
                "{:.0} MiB packed · {:.0} MiB resident",
                state.scene.geometry_bytes() as f64 / (1024.0 * 1024.0),
                state.frame.geometry as f64 / (1024.0 * 1024.0)
            ),
        );
        if state.pending > 0 {
            ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                widget::spinner(ui, 12.0);
                widget::caption(ui, &format!("{} bricks in flight", state.pending));
            });
        }
    });
    ui.store_open("streaming", next);
}

fn bricks(view: grana_render::ViewInfo) -> String {
    if view.demand == view.visible {
        format!(
            "level {} · {}/{}",
            view.level + 1,
            view.resident,
            view.demand
        )
    } else {
        format!(
            "level {} · {}/{} of {}",
            view.level + 1,
            view.resident,
            view.demand,
            view.visible
        )
    }
}
