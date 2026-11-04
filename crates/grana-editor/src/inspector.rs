use glam::Vec2;
use grana_scene::Scene;
use grana_ui::{Align, Block, Color, Corners, Icon, Level, Rect, Role, TextStyle, Ui};
use grana_volume::{Axis, Intensity};

use crate::panel::{divider, section_header};
use crate::state::State;
use crate::widget;
use grana_session::Command;

const CHANNEL_ROW: f32 = 30.0;

pub fn dataset_height(state: &State<'_>) -> f32 {
    let mut height = 60.0;
    if state.series.entries.len() > 1 {
        height += 32.0;
    }
    if state.io.loading.is_some() {
        height += 22.0;
    }
    if state.io.document.is_some() {
        height += 22.0;
    }
    height
}

pub fn dataset(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let dataset = &state.scene.dataset;
    let palette = ui.theme().palette;
    let extent = dataset.extent();
    let spacing = dataset.spacing();
    let channels = dataset.channels().len();
    let frames = dataset.frame_count();
    ui.column(Block::column().gaps(4.0), |ui| {
        ui.text_style(
            dataset.name(),
            TextStyle::new(ui.theme().font(Level::Title), palette.text_primary).truncating(),
        );
        widget::caption(
            ui,
            &format!(
                "{} × {} × {} px · {:.3} µm/px",
                extent.x(),
                extent.y(),
                extent.z(),
                spacing.x()
            ),
        );
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            let source = match dataset.origin() {
                grana_scene::Origin::Synthetic => "Synthetic",
                grana_scene::Origin::Zarr => "OME-Zarr",
                grana_scene::Origin::Tiff => "OME-TIFF",
                grana_scene::Origin::Czi => "CZI",
            };
            widget::badge(ui, source, palette.text_secondary);
            widget::caption(
                ui,
                &format!(
                    "{} channel{} · {} frame{}",
                    channels,
                    plural(channels),
                    frames,
                    plural(frames as usize)
                ),
            );
        });
        series(ui, state, commands);
        if let Some(name) = state.io.loading {
            progress_row(ui, name);
        }
        if let Some((label, done, total)) = state.io.document {
            let text = done
                .checked_mul(100)
                .and_then(|percent| percent.checked_div(total))
                .map_or_else(
                    || label.to_string(),
                    |percent| format!("{label} · {percent}%"),
                );
            progress_row(ui, &text);
        }
    });
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn progress_row(ui: &mut Ui, text: &str) {
    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
        widget::spinner(ui, 12.0);
        widget::caption(ui, text);
    });
}

fn series(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let entries = state.series.entries;
    if entries.len() < 2 {
        return;
    }
    let unit = match state.scene.dataset.origin() {
        grana_scene::Origin::Czi => "Scene",
        _ => "Series",
    };
    let options = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let label = entry
                .name()
                .map_or_else(|| format!("{unit} {}", index + 1), |name| name.to_string());
            (index, label)
        })
        .collect::<Vec<_>>();
    let width = ui.available().x;
    if let Some(index) = grana_ui::popover::combo(
        ui,
        "dataset.series",
        width,
        &options,
        Some(state.series.selected),
    ) && index != state.series.selected
    {
        commands.push(Command::SelectSeries(index));
    }
}

pub fn view_body(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    channels(ui, state.scene, commands);
    divider(ui);
    display(ui, state.scene, commands);
    divider(ui);
    section(ui, state);
    divider(ui);
    rendering(ui, state.scene, commands);
}

pub fn objects_body(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    region(ui, state, commands);
    divider(ui);
    mask(ui, state, commands);
    divider(ui);
    crate::object_panel::build(ui, state, commands);
}

fn channels(ui: &mut Ui, scene: &Scene, commands: &mut Vec<Command>) {
    let palette = ui.theme().palette;
    section_header(ui, "Channels", Some(&scene.channels.len().to_string()));
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
        let control = Rect::from_min_max(row.min, Vec2::new(row.max.x - 30.0, row.max.y));
        let response = ui.interact(ui.id("pick"), control);
        if response.clicked {
            chosen = Some(index);
        }
        let hover = ui.animate(
            ui.id("hover"),
            if response.hovered { 1.0 } else { 0.0 },
            grana_ui::Motion::quick(),
        );
        let active = selected == index;
        let color = if !enabled {
            palette.text_quaternary
        } else if active {
            palette.text_primary
        } else {
            palette.text_secondary
        };
        let text_style = TextStyle::new(ui.theme().font(Level::Body), color).truncating();
        {
            let mut painter = ui.painter();
            if active {
                painter.rect(row, Corners::all(4.0), palette.selected);
            } else if hover > 0.01 {
                painter.rect(
                    row,
                    Corners::all(4.0),
                    palette.raised.with_alpha(0.8 * hover),
                );
            }
            let dot = Rect::from_center_size(
                Vec2::new(row.min.x + 11.0, row.center().y),
                Vec2::splat(8.0),
            );
            painter.rect(
                dot,
                Corners::capsule(),
                Color::from_rgb_f32(tint).with_alpha(if enabled { 1.0 } else { 0.28 }),
            );
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(row.min.x + 23.0, row.min.y),
                    Vec2::new(row.max.x - 34.0, row.max.y),
                ),
                &name,
                text_style,
                Align::Center,
            );
        }
        ui.place(
            Rect::from_center_size(
                Vec2::new(row.max.x - 15.0, row.center().y),
                Vec2::splat(22.0),
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
                    if enabled { Icon::Eye } else { Icon::EyeOff },
                    22.0,
                    Role::Quiet,
                    tooltip,
                ) {
                    commands.push(Command::SetChannelEnabled {
                        channel: index,
                        enabled: !enabled,
                    });
                }
            },
        );
        ui.pop_id();
    }
    if let Some(index) = chosen {
        commands.push(Command::SelectChannel(index));
    }
}

fn display(ui: &mut Ui, scene: &Scene, commands: &mut Vec<Command>) {
    let palette = ui.theme().palette;
    let index = scene.selected;
    let name = scene.channel(index).name().to_string();
    let tint = scene.channel(index).tint();
    section_header(ui, "Display", Some(&name));
    let data = scene.channel(index).intensity();
    let current = scene.channels[index].window;
    let mut start = current.min();
    let mut end = current.max();
    let available = data.min()..=data.max();
    let transfer = scene.channels[index].transfer().clone();
    let gradient = transfer_colors(&transfer);
    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
        widget::field_label(ui, "Range");
        widget::caption(ui, &format!("{:.0} – {:.0}", data.min(), data.max()));
    });
    if let Some((low, high)) =
        widget::range_slider(ui, "window", start, end, available.clone(), &gradient)
    {
        start = low;
        end = high;
        commands.push(Command::SetWindow {
            channel: index,
            window: Intensity::new(start.min(end), start.max(end)),
        });
    }
    let values = ui.allocate_full(14.0);
    {
        let style = TextStyle::new(ui.theme().font(Level::Monospace), palette.text_tertiary);
        let mut painter = ui.painter();
        painter.text_in(
            Rect::from_min_size(values.min, Vec2::new(values.width() * 0.5, values.height())),
            &format!("{start:.0}"),
            style,
            Align::Center,
        );
        painter.text_in(
            values,
            &format!("{end:.0}"),
            TextStyle::new(style.font, style.color).aligned(Align::End),
            Align::Center,
        );
    }
    let opacity = scene.channels[index].opacity;
    if let Some(value) = widget::slider_readout(
        ui,
        "opacity",
        Some("Opacity"),
        opacity,
        0.0..=1.0,
        false,
        &format!("{:.0}%", opacity * 100.0),
    ) {
        commands.push(Command::SetChannelOpacity {
            channel: index,
            opacity: value.clamp(0.0, 1.0),
        });
    }
    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
        if widget::button(ui, "preset.auto", "Auto", Role::Control) {
            commands.push(Command::ResetWindow(index));
        }
        if widget::button(ui, "preset.tinted", "Tinted", Role::Control) {
            commands.push(Command::SetTransfer {
                channel: index,
                transfer: grana_display::TransferFunction::tinted(tint),
            });
        }
        if widget::button(ui, "preset.gray", "Gray", Role::Control) {
            commands.push(Command::SetTransfer {
                channel: index,
                transfer: grana_display::TransferFunction::grayscale(),
            });
        }
    });
    let open = ui.disclosure_open("transfer", false);
    let window = scene.channels[index].window;
    let mut edited = transfer.clone();
    let next = widget::disclosure(ui, "transfer", "Transfer curve", open, |ui| {
        crate::transfer::edit(ui, "transfer", &mut edited, tint, window);
    });
    if edited != transfer {
        commands.push(Command::SetTransfer {
            channel: index,
            transfer: edited,
        });
    }
    ui.store_open("transfer", next);
}

fn section(ui: &mut Ui, state: &mut State<'_>) {
    let axis = state.viewport.section().axis();
    let options = ["XY", "XZ", "YZ"];
    let current = match axis {
        Axis::Z => 0,
        Axis::Y => 1,
        Axis::X => 2,
    };
    let spacing = state.scene.dataset.spacing();
    section_header(
        ui,
        "Section",
        Some(&format!("{:.3} µm steps", axis.spacing(spacing))),
    );
    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
        if let Some(index) = widget::segmented(ui, "axis", &options, current) {
            let target = match index {
                0 => Axis::Z,
                1 => Axis::Y,
                _ => Axis::X,
            };
            state.viewport.section_mut().set_axis(target);
            crate::view::fit_section(state);
        }
        ui.space(1.0);
        if widget::icon_button(
            ui,
            "fit.section",
            Icon::Fit,
            24.0,
            Role::Quiet,
            "Fit section",
        ) {
            crate::view::fit_section(state);
        }
    });
    let extent = state.scene.dataset.world_size()[axis.index()];
    let mut position = state.viewport.camera().focus_along(axis) + 0.5 * extent;
    if let Some(value) = widget::slider_readout(
        ui,
        "plane",
        Some("Plane"),
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
}

fn region(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let open = ui.disclosure_open("region", false);
    let next = widget::disclosure(ui, "region", "Region", open, |ui| {
        let world = state.scene.dataset.world_size();
        let half = 0.5 * world;
        let mut region = state.scene.region;
        let mut enabled = region.enabled();
        if let Some(value) = widget::toggle_row(ui, "region.enabled", "Limit analysis", enabled) {
            enabled = value;
            region.set_enabled(value);
        }
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
                widget::caption(ui, "–");
                if let Some(value) = widget::drag_value(ui, "high", end, speed, 3) {
                    end = value;
                    changed = true;
                }
            });
            if let Some((start_value, end_value)) = widget::range_slider(
                ui,
                "bounds",
                start,
                end,
                -limit..=limit,
                &[Color::white_alpha(0.16), Color::white_alpha(0.16)],
            ) {
                start = start_value;
                end = end_value;
                changed = true;
            }
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
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if widget::button(ui, "region.reset", "Reset", Role::Control) {
                region.set_bounds(-half, half);
            }
            let size = region.size();
            if enabled {
                widget::caption(
                    ui,
                    &format!("{:.2} × {:.2} × {:.2} µm", size.x, size.y, size.z),
                );
            } else {
                widget::caption(ui, "whole dataset");
            }
        });
        if region != state.scene.region {
            commands.push(Command::SetRegion(region));
        }
    });
    ui.store_open("region", next);
}

fn mask(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let sources = state
        .scene
        .objects
        .iter()
        .map(|set| (set.id(), set.name().to_string()))
        .collect::<Vec<_>>();
    let current = state.scene.mask;
    section_header(ui, "Analysis mask", None);
    let mut options = vec![(None, "Whole dataset".to_string())];
    options.extend(sources.iter().map(|(id, name)| (Some(*id), name.clone())));
    let width = ui.available().x;
    if let Some(chosen) = grana_ui::popover::combo(ui, "mask", width, &options, Some(current))
        && chosen != current
    {
        commands.push(Command::SetMask(chosen));
    }
    match current.and_then(|id| sources.iter().find(|(set, _)| *set == id)) {
        Some((_, name)) => widget::caption(ui, &format!("Detect inside {name} only")),
        None => widget::caption(ui, "Detect across the whole dataset"),
    }
}

fn rendering(ui: &mut Ui, scene: &Scene, commands: &mut Vec<Command>) {
    let open = ui.disclosure_open("rendering", false);
    let next = widget::disclosure(ui, "rendering", "Rendering", open, |ui| {
        let mut exposure = scene.exposure;
        if let Some(value) = widget::slider_readout(
            ui,
            "exposure",
            Some("Exposure"),
            exposure,
            0.05..=20.0,
            true,
            &format!("{exposure:.2}"),
        ) {
            exposure = value;
        }
        if exposure != scene.exposure {
            commands.push(Command::SetExposure(exposure));
        }
        let mut step = scene.step_scale;
        if let Some(value) = widget::slider_readout(
            ui,
            "step",
            Some("Sampling"),
            step,
            0.25..=2.0,
            true,
            &format!("{step:.2}"),
        ) {
            step = value;
        }
        if step != scene.step_scale {
            commands.push(Command::SetStepScale(step));
        }
        let mut shadows = scene.shading.shadows;
        if let Some(value) = widget::slider_readout(
            ui,
            "shadows",
            Some("Shadows"),
            shadows,
            0.0..=1.0,
            false,
            &format!("{shadows:.2}"),
        ) {
            shadows = value;
        }
        if shadows != scene.shading.shadows {
            commands.push(Command::SetShading(grana_display::Shading {
                shadows,
                ..scene.shading
            }));
        }
        ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
            if widget::button(ui, "reset.look", "Reset", Role::Control) {
                commands.push(Command::ResetRendering);
            }
        });
        let open = ui.disclosure_open("lighting", false);
        let next = widget::disclosure(ui, "lighting", "Lighting", open, |ui| {
            let mut ambient = scene.shading.ambient;
            if let Some(value) = widget::slider_readout(
                ui,
                "ambient",
                Some("Ambient"),
                ambient,
                0.0..=1.0,
                false,
                &format!("{ambient:.2}"),
            ) {
                ambient = value;
            }
            if ambient != scene.shading.ambient {
                commands.push(Command::SetShading(grana_display::Shading {
                    ambient,
                    ..scene.shading
                }));
            }
            let mut diffuse = scene.shading.diffuse;
            if let Some(value) = widget::slider_readout(
                ui,
                "diffuse",
                Some("Diffuse"),
                diffuse,
                0.0..=1.0,
                false,
                &format!("{diffuse:.2}"),
            ) {
                diffuse = value;
            }
            if diffuse != scene.shading.diffuse {
                commands.push(Command::SetShading(grana_display::Shading {
                    diffuse,
                    ..scene.shading
                }));
            }
            let mut specular = scene.shading.specular;
            if let Some(value) = widget::slider_readout(
                ui,
                "specular",
                Some("Specular"),
                specular,
                0.0..=1.0,
                false,
                &format!("{specular:.2}"),
            ) {
                specular = value;
            }
            if specular != scene.shading.specular {
                commands.push(Command::SetShading(grana_display::Shading {
                    specular,
                    ..scene.shading
                }));
            }
            let mut shininess = scene.shading.shininess;
            if let Some(value) = widget::slider_readout(
                ui,
                "shininess",
                Some("Shininess"),
                shininess,
                1.0..=128.0,
                true,
                &format!("{shininess:.0}"),
            ) {
                shininess = value;
            }
            if shininess != scene.shading.shininess {
                commands.push(Command::SetShading(grana_display::Shading {
                    shininess,
                    ..scene.shading
                }));
            }
        });
        ui.store_open("lighting", next);
    });
    ui.store_open("rendering", next);
}

fn transfer_colors(transfer: &grana_display::TransferFunction) -> Vec<Color> {
    transfer
        .stops()
        .iter()
        .map(|stop| {
            let [red, green, blue] = stop.color();
            Color::rgba(red, green, blue, 1.0)
        })
        .collect()
}
