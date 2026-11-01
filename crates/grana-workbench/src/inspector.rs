use glam::Vec2;
use grana_scene::Scene;
use grana_ui::{Align, Block, Color, Corners, Length, Level, Rect, Role, TextStyle, TypeStyle, Ui};
use grana_volume::Axis;

use crate::action::Action;
use crate::state::State;
use crate::widget;

const CHANNEL_ROW: f32 = 34.0;

fn section_header(ui: &mut Ui, title: &str, detail: Option<&str>) {
    let rect = ui.allocate_full(22.0);
    let style = TextStyle::new(
        ui.theme().font(Level::Micro),
        ui.theme().palette.text_tertiary,
    );
    let detail = detail.map(str::to_string);
    let detail_style = TextStyle::new(
        TypeStyle::new(10.0, 460.0, 13.0),
        ui.theme().palette.text_tertiary,
    )
    .aligned(Align::End);
    let mut painter = ui.painter();
    painter.text_in(rect, &title.to_uppercase(), style, Align::Center);
    if let Some(detail) = detail {
        painter.text_in(rect, &detail, detail_style, Align::Center);
    }
}

pub fn header(ui: &mut Ui, state: &State<'_>) {
    let dataset = &state.scene.dataset;
    widget::field_label(ui, "Dataset");
    let name = dataset.name();
    let rect = ui.allocate_full(26.0);
    let style = TextStyle::new(
        ui.theme().font(Level::Title),
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
    let dims = format!(
        "{} × {} × {} · {:.3} µm/px",
        extent.x(),
        extent.y(),
        extent.z(),
        spacing.x()
    );
    let summary = format!(
        "{} channel{} · {} frame{}",
        channels,
        plural(channels),
        frames,
        plural(frames as usize)
    );
    widget::caption(ui, &dims);
    let style = TextStyle::new(
        TypeStyle::new(11.0, 430.0, 14.0),
        ui.theme().palette.text_tertiary,
    );
    ui.text_style(&summary, style);
    let source = match dataset.origin() {
        grana_scene::Origin::Synthetic => "Synthetic",
        grana_scene::Origin::Zarr => "OME-Zarr",
    };
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        widget::badge(ui, source, ui.theme().palette.accent);
        if let Some(project) = state.project {
            let marker = if state.dirty { " · Edited" } else { "" };
            widget::caption(ui, &format!("{project}{marker}"));
        } else {
            widget::caption(ui, "Untitled project");
        }
    });
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
    section_header(ui, "Channels", Some(&scene.channels.len().to_string()));
    ui.column(Block::column().gaps(3.0), |ui| {
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
            let quaternary = ui.theme().palette.text_quaternary;
            let body = ui.theme().font(Level::Body);
            {
                let mut painter = ui.painter();
                if active {
                    painter.rect(row, Corners::all(9.0), accent.with_alpha(0.16));
                    painter.rect(
                        Rect::from_center_size(
                            Vec2::new(row.min.x + 3.0, row.center().y),
                            Vec2::new(2.4, 15.0),
                        ),
                        Corners::capsule(),
                        accent,
                    );
                } else if hover > 0.01 {
                    painter.rect(row, Corners::all(9.0), Color::white_alpha(0.045 * hover));
                }
                let dot = Vec2::new(row.min.x + 17.0, row.center().y);
                painter.rect(
                    Rect::from_center_size(dot, Vec2::splat(11.0)),
                    Corners::capsule(),
                    Color::from_rgb_f32(tint).with_alpha(if enabled { 1.0 } else { 0.35 }),
                );
                painter.outlined(
                    Rect::from_center_size(dot, Vec2::splat(15.0)),
                    Corners::capsule(),
                    grana_ui::Fill::None,
                    grana_ui::Border {
                        width: 0.5,
                        color: Color::white_alpha(if enabled { 0.22 } else { 0.10 }),
                    },
                    grana_ui::Shadow::NONE,
                );
                let color = if !enabled {
                    quaternary
                } else if active {
                    primary
                } else {
                    secondary
                };
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(row.min.x + 32.0, row.min.y),
                        Vec2::new(row.max.x - 46.0, row.max.y),
                    ),
                    &name,
                    TextStyle::new(body, color),
                    Align::Center,
                );
            }
            ui.place(
                Rect::from_center_size(
                    Vec2::new(row.max.x - 20.0, row.center().y),
                    Vec2::splat(26.0),
                ),
                |ui| {
                    let tooltip = if enabled {
                        "Hide channel"
                    } else {
                        "Show channel"
                    };
                    if widget::icon_button(
                        ui,
                        "enabled",
                        if enabled {
                            grana_ui::Icon::Eye
                        } else {
                            grana_ui::Icon::EyeOff
                        },
                        24.0,
                        Role::Quiet,
                        tooltip,
                    ) {
                        scene.channels[index].enabled = !enabled;
                    }
                },
            );
            ui.pop_id();
        }
        if let Some(index) = chosen {
            scene.select_channel(index);
        }
    });
    ui.space(2.0);
}

pub fn display(ui: &mut Ui, scene: &mut Scene) {
    let index = scene.selected;
    let name = scene.channel(index).name().to_string();
    let tint = scene.channel(index).tint();
    {
        let rect = ui.allocate_full(22.0);
        let style = TextStyle::new(
            ui.theme().font(Level::Micro),
            ui.theme().palette.text_tertiary,
        );
        let tag_style = TextStyle::new(
            TypeStyle::new(11.0, 540.0, 14.0),
            ui.theme().palette.text_secondary,
        )
        .aligned(Align::End);
        let mut painter = ui.painter();
        painter.text_in(rect, "DISPLAY", style, Align::Center);
        painter.rect(
            Rect::from_center_size(
                Vec2::new(rect.max.x - 6.0, rect.center().y),
                Vec2::splat(7.0),
            ),
            Corners::capsule(),
            Color::from_rgb_f32(tint),
        );
        painter.text_in(
            Rect::from_min_max(rect.min, Vec2::new(rect.max.x - 14.0, rect.max.y)),
            &name,
            tag_style,
            Align::Center,
        );
    }
    let data = scene.channel(index).intensity();
    let current = scene.channels[index].window;
    let mut start = current.min();
    let mut end = current.max();
    let available = data.min()..=data.max();
    let transfer = scene.channels[index].transfer().clone();
    let gradient = transfer_colors(&transfer);
    widget::caption(ui, &format!("data {:.0} – {:.0}", data.min(), data.max()));
    if let Some((low, high)) =
        widget::range_slider(ui, "window", start, end, available.clone(), &gradient)
    {
        start = low;
        end = high;
        scene.set_window(index, start, end);
    }
    let readout = TextStyle::new(
        ui.theme().font(Level::Monospace),
        ui.theme().palette.text_secondary,
    );
    let low_style = TextStyle::new(readout.font, ui.theme().palette.text_tertiary);
    let values = Rect::from_min_size(ui.cursor_position(), Vec2::new(ui.available().x, 16.0));
    {
        let mut painter = ui.painter();
        painter.text_in(
            Rect::from_min_size(values.min, Vec2::new(values.width() * 0.5, values.height())),
            &format!("{:.0}", start),
            low_style,
            Align::Center,
        );
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(values.min.x + values.width() * 0.5, values.min.y),
                Vec2::new(values.width() * 0.5, values.height()),
            ),
            &format!("{:.0}", end),
            TextStyle::new(readout.font, readout.color).aligned(Align::End),
            Align::Center,
        );
    }
    ui.space(16.0);
    let opacity = scene.channels[index].opacity;
    if let Some(value) = widget::slider_readout(
        ui,
        "opacity",
        Some("opacity"),
        opacity,
        0.0..=1.0,
        false,
        &format!("{:.0}%", opacity * 100.0),
    ) {
        scene.channels[index].opacity = value.clamp(0.0, 1.0);
    }
    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
        if widget::button(ui, "auto", "Auto", Role::Quiet) {
            scene.reset_window(index);
        }
        if widget::button(ui, "preset.tinted", "Tinted", Role::Quiet) {
            *scene.channels[index].transfer_mut() = grana_render::TransferFunction::tinted(tint);
        }
        if widget::button(ui, "preset.gray", "Grayscale", Role::Quiet) {
            *scene.channels[index].transfer_mut() = grana_render::TransferFunction::grayscale();
        }
    });
    let open = ui.disclosure_open("transfer", false);
    let window = scene.channels[index].window;
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
    let axis = state.viewport.section().axis();
    let options = ["XY", "XZ", "YZ"];
    let current = match axis {
        Axis::Z => 0,
        Axis::Y => 1,
        Axis::X => 2,
    };
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        widget::field_label(ui, "SECTION");
        ui.space(2.0);
        if let Some(index) = widget::segmented(ui, "axis", &options, current) {
            let target = match index {
                0 => Axis::Z,
                1 => Axis::Y,
                _ => Axis::X,
            };
            actions.push(Action::SetSectionAxis(target));
        }
        if widget::icon_button(
            ui,
            "fit.section",
            grana_ui::Icon::Fit,
            26.0,
            Role::Quiet,
            "Fit section",
        ) {
            actions.push(Action::FitSection);
        }
    });
    let extent = state.scene.dataset.world_size()[axis.index()];
    let mut position = state.viewport.camera().focus_along(axis) + 0.5 * extent;
    let spacing = state.scene.dataset.spacing();
    if let Some(value) = widget::slider_readout(
        ui,
        "plane",
        Some("plane"),
        position,
        0.0..=extent,
        false,
        &format!("{position:.2} µm"),
    ) {
        position = value;
        state
            .viewport
            .camera_mut()
            .set_focus_along(axis, position - 0.5 * extent);
    }
    widget::caption(
        ui,
        &format!(
            "{} plane · {:.3} µm steps",
            axis.name(),
            axis.spacing(spacing)
        ),
    );
    ui.space(2.0);
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
                    &format!("{:.2} × {:.2} × {:.2} µm", size.x, size.y, size.z),
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
    let open = ui.disclosure_open("rendering", true);
    let next = widget::disclosure(ui, "rendering", "Rendering", open, |ui| {
        let mut exposure = scene.exposure;
        if let Some(value) = widget::slider_readout(
            ui,
            "exposure",
            Some("exposure"),
            exposure,
            0.05..=20.0,
            true,
            &format!("{exposure:.2}"),
        ) {
            exposure = value;
        }
        scene.exposure = exposure;
        let mut step = scene.step_scale;
        if let Some(value) = widget::slider_readout(
            ui,
            "step",
            Some("sampling"),
            step,
            0.25..=2.0,
            true,
            &format!("{step:.2}"),
        ) {
            step = value;
        }
        scene.step_scale = step;
        let mut shadows = scene.shading.shadows;
        if let Some(value) = widget::slider_readout(
            ui,
            "shadows",
            Some("shadows"),
            shadows,
            0.0..=1.0,
            false,
            &format!("{shadows:.2}"),
        ) {
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
            if let Some(value) = widget::slider_readout(
                ui,
                "ambient",
                Some("ambient"),
                ambient,
                0.0..=1.0,
                false,
                &format!("{ambient:.2}"),
            ) {
                ambient = value;
            }
            scene.shading.ambient = ambient;
            let mut diffuse = scene.shading.diffuse;
            if let Some(value) = widget::slider_readout(
                ui,
                "diffuse",
                Some("diffuse"),
                diffuse,
                0.0..=1.0,
                false,
                &format!("{diffuse:.2}"),
            ) {
                diffuse = value;
            }
            scene.shading.diffuse = diffuse;
            let mut specular = scene.shading.specular;
            if let Some(value) = widget::slider_readout(
                ui,
                "specular",
                Some("specular"),
                specular,
                0.0..=1.0,
                false,
                &format!("{specular:.2}"),
            ) {
                specular = value;
            }
            scene.shading.specular = specular;
            let mut shininess = scene.shading.shininess;
            if let Some(value) = widget::slider_readout(
                ui,
                "shininess",
                Some("shininess"),
                shininess,
                1.0..=128.0,
                true,
                &format!("{shininess:.0}"),
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

fn transfer_colors(transfer: &grana_render::TransferFunction) -> Vec<Color> {
    transfer
        .stops()
        .iter()
        .map(|stop| {
            let [red, green, blue] = stop.color();
            Color::rgba(red, green, blue, 1.0)
        })
        .collect()
}
