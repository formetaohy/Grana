use glam::Vec2;
use grana_scene::Scene;
use grana_ui::{Align, Block, Color, Corners, Length, Level, Rect, Role, TextStyle, TypeStyle, Ui};
use grana_volume::{Axis, Intensity};

use crate::state::State;
use crate::widget;
use grana_session::Command;

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

pub fn header(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
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
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        ui.text_style(&summary, style);
        series(ui, state, commands);
    });
    let source = match dataset.origin() {
        grana_scene::Origin::Synthetic => "Synthetic",
        grana_scene::Origin::Zarr => "OME-Zarr",
        grana_scene::Origin::Tiff => "OME-TIFF",
        grana_scene::Origin::Czi => "CZI",
    };
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        widget::badge(ui, source, ui.theme().palette.accent);
        if let Some(project) = state.io.project.as_deref() {
            let marker = if state.io.dirty { " · Edited" } else { "" };
            widget::caption(ui, &format!("{project}{marker}"));
        } else {
            widget::caption(ui, "Untitled project");
        }
    });
    if let Some(name) = state.io.loading {
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            widget::spinner(ui, 13.0);
            widget::caption(ui, name);
        });
    }
    if let Some((label, done, total)) = state.io.document {
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            widget::spinner(ui, 13.0);
            let text = done
                .checked_mul(100)
                .and_then(|percent| percent.checked_div(total))
                .map_or_else(
                    || label.to_string(),
                    |percent| format!("{label} · {percent}%"),
                );
            widget::caption(ui, &text);
        });
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn series(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let entries = state.series.entries;
    if entries.len() < 2 {
        return;
    }
    let unit = match state.scene.dataset.origin() {
        grana_scene::Origin::Czi => "Scene",
        _ => "Series",
    };
    let selected = state.series.selected;
    let label = format!("{unit} {} of {}", selected + 1, entries.len());
    let response = widget::button_with(
        ui,
        "dataset.series",
        &label,
        Role::Quiet,
        Length::Maximum(136.0),
    );
    let anchor = response.rect;
    if response.clicked {
        ui.toggle_popup(ui.id("dataset.series.menu"));
    }
    let chevron = ui.theme().palette.text_tertiary;
    {
        let mut painter = ui.painter();
        grana_ui::icon::paint(
            grana_ui::Icon::ChevronDown,
            &mut painter,
            Vec2::new(anchor.max.x - 10.0, anchor.center().y),
            10.0,
            chevron,
            1.5,
        );
    }
    let open = ui.popup_open(ui.id("dataset.series.menu"));
    let mut chosen = None;
    grana_ui::popover::popover(ui, "dataset.series.menu", open, anchor, 248.0, |ui| {
        for (index, entry) in entries.iter().enumerate() {
            let extent = entry.extent();
            let detail = format!(
                "{unit} {} · {} × {} × {}",
                index + 1,
                extent.x(),
                extent.y(),
                extent.z()
            );
            if grana_ui::popover::menu_row(ui, index, &detail, index == selected) {
                chosen = Some(index);
            }
        }
    });
    if let Some(index) = chosen {
        ui.close_popup();
        commands.push(Command::SelectSeries(index));
    }
}

pub fn channels(ui: &mut Ui, scene: &Scene, commands: &mut Vec<Command>) {
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
    });
    ui.space(2.0);
}

pub fn display(ui: &mut Ui, scene: &Scene, commands: &mut Vec<Command>) {
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
        commands.push(Command::SetWindow {
            channel: index,
            window: Intensity::new(start.min(end), start.max(end)),
        });
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
        commands.push(Command::SetChannelOpacity {
            channel: index,
            opacity: value.clamp(0.0, 1.0),
        });
    }
    ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
        if widget::button(ui, "auto", "Auto", Role::Quiet) {
            commands.push(Command::ResetWindow(index));
        }
        if widget::button(ui, "preset.tinted", "Tinted", Role::Quiet) {
            commands.push(Command::SetTransfer {
                channel: index,
                transfer: grana_display::TransferFunction::tinted(tint),
            });
        }
        if widget::button(ui, "preset.gray", "Grayscale", Role::Quiet) {
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
    ui.space(2.0);
}

pub fn section(ui: &mut Ui, state: &mut State<'_>) {
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
            state.viewport.section_mut().set_axis(target);
            crate::view::fit_section(state);
        }
        if widget::icon_button(
            ui,
            "fit.section",
            grana_ui::Icon::Fit,
            26.0,
            Role::Quiet,
            "Fit section",
        ) {
            crate::view::fit_section(state);
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

pub fn region(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
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
        if region != state.scene.region {
            commands.push(Command::SetRegion(region));
        }
    });
    ui.store_open("region", next);
}

pub fn mask(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let sources = state
        .scene
        .objects
        .iter()
        .map(|set| (set.id(), set.name().to_string(), set.detected()))
        .collect::<Vec<_>>();
    let current = state.scene.mask;
    let label = current
        .and_then(|id| {
            sources
                .iter()
                .find(|(set, _, _)| *set == id)
                .map(|(_, name, _)| name.clone())
        })
        .unwrap_or_else(|| "Whole dataset".to_string());
    let mut chosen = None;
    section_header(ui, "Analysis mask", None);
    let response = widget::button_with(ui, "mask.pick", &label, Role::Quiet, Length::Fill);
    let anchor = response.rect;
    if response.clicked {
        ui.toggle_popup(ui.id("mask.menu"));
    }
    let chevron = ui.theme().palette.text_tertiary;
    {
        let mut painter = ui.painter();
        grana_ui::icon::paint(
            grana_ui::Icon::ChevronDown,
            &mut painter,
            Vec2::new(anchor.max.x - 12.0, anchor.center().y),
            11.0,
            chevron,
            1.5,
        );
    }
    let open = ui.popup_open(ui.id("mask.menu"));
    grana_ui::popover::popover(ui, "mask.menu", open, anchor, 216.0, |ui| {
        if grana_ui::popover::menu_row(ui, 0, "Whole dataset", current.is_none()) {
            chosen = Some(None);
        }
        for (index, (set, name, detected)) in sources.iter().enumerate() {
            let detail = if *detected == 0 {
                format!("{name} · empty")
            } else {
                name.clone()
            };
            if grana_ui::popover::menu_row(ui, index + 1, &detail, current == Some(*set)) {
                chosen = Some(Some(*set));
            }
        }
    });
    if let Some(source) = chosen {
        ui.close_popup();
        commands.push(Command::SetMask(source));
    }
    match current.and_then(|id| sources.iter().find(|(set, _, _)| *set == id)) {
        Some((_, name, _)) => widget::caption(ui, &format!("detection reads only inside {name}")),
        None => widget::caption(ui, "detection reads the whole dataset"),
    }
}

pub fn rendering(ui: &mut Ui, scene: &Scene, commands: &mut Vec<Command>) {
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
        if exposure != scene.exposure {
            commands.push(Command::SetExposure(exposure));
        }
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
        if step != scene.step_scale {
            commands.push(Command::SetStepScale(step));
        }
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
        if shadows != scene.shading.shadows {
            commands.push(Command::SetShading(grana_display::Shading {
                shadows,
                ..scene.shading
            }));
        }
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if widget::button(ui, "reset.look", "Reset", Role::Quiet) {
                commands.push(Command::ResetRendering);
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
                Some("diffuse"),
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
                Some("specular"),
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
                Some("shininess"),
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

pub fn streaming(ui: &mut Ui, state: &State<'_>) {
    let open = ui.disclosure_open("streaming", false);
    let next = widget::disclosure(ui, "streaming", "Streaming", open, |ui| {
        widget::labelled(ui, "volume", &bricks(state.frame.volume));
        widget::labelled(ui, "section", &bricks(state.frame.section));
        widget::labelled(
            ui,
            "levels",
            &format!(
                "{} levels · {} stored",
                state.scene.dataset.level_count(),
                state.scene.dataset.stored_levels()
            ),
        );
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
                "{:.0} MiB packed · {:.0} MiB memory · {:.0} MiB disk",
                state.scene.geometry_bytes() as f64 / (1024.0 * 1024.0),
                state.io.store.resident as f64 / (1024.0 * 1024.0),
                state.io.store.spilled as f64 / (1024.0 * 1024.0)
            ),
        );
        widget::labelled(
            ui,
            "fragments",
            &format!(
                "{:.0} MiB transparent",
                state.frame.fragments as f64 / (1024.0 * 1024.0)
            ),
        );
        widget::labelled(
            ui,
            "cuts",
            &format!(
                "{:.0} MiB indexes",
                state.frame.indices as f64 / (1024.0 * 1024.0)
            ),
        );
        if state.io.pending > 0 {
            ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                widget::spinner(ui, 12.0);
                widget::caption(ui, &format!("{} bricks in flight", state.io.pending));
            });
        }
    });
    ui.store_open("streaming", next);
}

fn bricks(view: crate::state::ViewStats) -> String {
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
