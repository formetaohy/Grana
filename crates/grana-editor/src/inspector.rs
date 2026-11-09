use glam::{Vec2, Vec3};
use grana_display::TransferFunction;
use grana_optics::{Psf, PsfParams};
use grana_scene::{MaskFilter, RelationField};
use grana_session::Command;
use grana_ui::{
    Align, Border, BuildContext, Color, Corners, Edges, Element, Fill, Icon, Key, Length, Level,
    Motion, Rect, Role, Shadow, State, TextStyle, icon,
};
use grana_volume::{Axis, Clip, Intensity};

use crate::message::Message;
use crate::state::View;
use crate::view;

const CHANNEL_ROW: f32 = 34.0;
const SWITCH_TRAIL: f32 = 4.0;
const DEFAULT_APERTURE: f32 = 1.4;
const DEFAULT_WAVELENGTH: f32 = 0.52;
const DEFAULT_IMMERSION: f32 = 1.515;
const DEFAULT_SAMPLE: f32 = 1.33;
const DEFAULT_ITERATIONS: u32 = 10;

fn restore_fields<'a>(
    ctx: &mut BuildContext<'_, Message>,
    channel: usize,
    spec: &grana_scene::RestoreSpec,
    depth_limit: f32,
) -> Vec<crate::view::View<'a>> {
    let iterations = spec.iterations();
    let psf = spec.psf().clone();
    let set = move |psf: Psf, iterations: u32| Command::SetRestore {
        channel,
        psf,
        iterations,
    };
    let mut rows = Vec::new();
    rows.push(view::slider_readout(
        ctx,
        "restore.iterations",
        Some("Iterations"),
        iterations as f32,
        1.0..=50.0,
        false,
        &format!("{iterations}"),
        move |value| set(psf.clone(), value.round().max(1.0) as u32),
    ));
    match spec.psf() {
        Psf::Theoretical(params) => {
            let params = *params;
            rows.push(view::slider_readout(
                ctx,
                "restore.aperture",
                Some("Aperture"),
                params.numerical_aperture(),
                0.3..=1.45,
                false,
                &format!("{:.2} NA", params.numerical_aperture()),
                move |value| {
                    set(
                        Psf::theoretical(PsfParams::new(
                            value,
                            params.wavelength(),
                            params.immersion(),
                            params.sample(),
                            params.depth(),
                        )),
                        iterations,
                    )
                },
            ));
            rows.push(view::slider_readout(
                ctx,
                "restore.wavelength",
                Some("Emission"),
                params.wavelength(),
                0.35..=0.8,
                false,
                &format!("{:.3} µm", params.wavelength()),
                move |value| {
                    set(
                        Psf::theoretical(PsfParams::new(
                            params.numerical_aperture(),
                            value,
                            params.immersion(),
                            params.sample(),
                            params.depth(),
                        )),
                        iterations,
                    )
                },
            ));
            rows.push(view::slider_readout(
                ctx,
                "restore.immersion",
                Some("Immersion"),
                params.immersion(),
                1.3..=1.6,
                false,
                &format!("{:.3}", params.immersion()),
                move |value| {
                    set(
                        Psf::theoretical(PsfParams::new(
                            params.numerical_aperture(),
                            params.wavelength(),
                            value,
                            params.sample(),
                            params.depth(),
                        )),
                        iterations,
                    )
                },
            ));
            rows.push(view::slider_readout(
                ctx,
                "restore.sample",
                Some("Specimen"),
                params.sample(),
                1.0..=1.6,
                false,
                &format!("{:.3}", params.sample()),
                move |value| {
                    set(
                        Psf::theoretical(PsfParams::new(
                            params.numerical_aperture(),
                            params.wavelength(),
                            params.immersion(),
                            value,
                            params.depth(),
                        )),
                        iterations,
                    )
                },
            ));
            rows.push(view::slider_readout(
                ctx,
                "restore.depth",
                Some("Depth"),
                params.depth(),
                0.0..=depth_limit,
                false,
                &format!("{:.1} µm", params.depth()),
                move |value| {
                    set(
                        Psf::theoretical(PsfParams::new(
                            params.numerical_aperture(),
                            params.wavelength(),
                            params.immersion(),
                            params.sample(),
                            value,
                        )),
                        iterations,
                    )
                },
            ));
        }
        Psf::Measured(psf) => {
            let dims = psf.dims();
            rows.push(view::caption(
                ctx,
                &format!(
                    "Measured from beads · {} depth bands · {}×{}×{}",
                    psf.layers(),
                    dims[0],
                    dims[1],
                    dims[2]
                ),
            ));
            rows.push(
                grana_ui::row()
                    .justify(Align::End)
                    .width(Length::Fill)
                    .children([view::button_with(
                        ctx,
                        "Theoretical Model",
                        Role::Quiet,
                        Length::Fixed(panel_width()),
                    )
                    .press(Message::Command(Command::SetRestore {
                        channel,
                        psf: Psf::theoretical(PsfParams::new(
                            DEFAULT_APERTURE,
                            DEFAULT_WAVELENGTH,
                            DEFAULT_IMMERSION,
                            DEFAULT_SAMPLE,
                            depth_limit * 0.5,
                        )),
                        iterations,
                    }))]),
            );
        }
    }
    rows
}

pub(crate) fn panel_width() -> f32 {
    crate::panel::SIDEBAR_WIDTH - crate::panel::SIDEBAR_PAD * 2.0
}

pub(crate) fn card_width() -> f32 {
    panel_width() - 24.0
}

pub fn source<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let has_series = state.series.entries.len() >= 2;
    let loading = state.io.loading;
    let document = state.io.document;
    let labels = state.io.label;
    if !has_series && loading.is_none() && document.is_none() && labels.is_none() {
        return hidden();
    }
    let mut children = Vec::new();
    if has_series {
        children.push(series(ctx, state));
    }
    if let Some(name) = loading {
        children.push(progress_row(ctx, name));
    }
    if let Some((label, done, total)) = document {
        let text = done
            .checked_mul(100)
            .and_then(|percent| percent.checked_div(total))
            .map_or_else(
                || label.to_string(),
                |percent| format!("{label} · {percent}%"),
            );
        children.push(progress_row(ctx, &text));
    }
    if let Some((label, done, total)) = labels {
        let text = done
            .checked_mul(100)
            .and_then(|percent| percent.checked_div(total))
            .map_or_else(
                || label.to_string(),
                |percent| format!("{label} · {percent}%"),
            );
        children.push(progress_row(ctx, &text));
    }
    view::card(ctx, 6.0, children)
}

pub fn view_body<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let mut elements = Vec::new();
    if state.plate.is_some() {
        elements.push(plate(ctx, state));
    }
    elements.extend([
        channels(ctx, state),
        display(ctx, state),
        section(ctx, state),
        clip(ctx, state),
        rendering(ctx, state),
    ]);
    elements
}

fn plate<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let Some(view) = state.plate else {
        return hidden();
    };
    let source = view.plate;
    let detail = format!(
        "{} × {} wells · {} fields",
        source.rows(),
        source.columns(),
        source.len()
    );
    let title = view::card_title(ctx, Icon::Grid, source.name(), Some(&detail));
    let current = view.selected_well().map_or_else(
        || "no field of view".to_string(),
        |well| {
            format!(
                "Well {} · {} field{}",
                well.label(),
                well.fields().len(),
                if well.fields().len() == 1 { "" } else { "s" }
            )
        },
    );
    let caption = view::caption(ctx, &current);
    let browse = view::button(ctx, "Browse Wells", Role::Control)
        .named("plate.browse")
        .press(Message::SetPanel(Some(crate::panel::Panel::Plate)));
    view::card(ctx, 6.0, vec![title, caption, browse])
}

pub fn objects_body<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let mut elements = vec![region(ctx, state), drift(ctx, state), mask(ctx, state)];
    elements.extend(crate::object_panel::view(ctx, state));
    elements
}

pub(crate) fn hidden<'a>() -> Element<'a, Message> {
    grana_ui::canvas(Length::Fixed(0.0), 0.0, |_painter, _info| {}).at(Rect::ZERO)
}

pub(crate) fn centered<'a>(height: f32, child: Element<'a, Message>) -> Element<'a, Message> {
    grana_ui::column()
        .width(Length::Auto)
        .height(Length::Fixed(height))
        .justify(Align::Center)
        .children([child])
}

fn series<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let unit = match state.scene.dataset.origin() {
        grana_scene::Origin::Czi => "Scene",
        _ => "Series",
    };
    let options = state
        .series
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let label = entry
                .name()
                .map_or_else(|| format!("{unit} {}", index + 1), |name| name.to_string());
            (index, label)
        })
        .collect::<Vec<_>>();
    let selected = state.series.selected;
    form_combo(
        ctx,
        "dataset.series",
        unit,
        &options,
        Some(selected),
        card_width() - crate::panel::FORM_LABEL,
        move |index| Message::Command(Command::SelectSeries(index)),
    )
}

fn progress_row<'a>(ctx: &mut BuildContext<'_, Message>, text: &str) -> Element<'a, Message> {
    let spinner = view::spinner(ctx, 12.0);
    let caption = view::caption(ctx, text);
    grana_ui::row()
        .gap(8.0)
        .align(Align::Center)
        .children([spinner, caption])
}

fn channels<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let scene = state.scene;
    let count = scene.channels.len();
    let enabled = scene
        .channels
        .iter()
        .filter(|channel| channel.enabled)
        .count();
    let detail = format!("{enabled}/{count} visible");
    let title = view::card_title(ctx, Icon::Layers, "Channels", Some(&detail));
    let mut children = Vec::new();
    children.push(
        grana_ui::column()
            .gap(0.0)
            .children([title, grana_ui::spacer(2.0)]),
    );
    for index in 0..count {
        children.push(channel_row(ctx, state, index));
    }
    view::card(ctx, 3.0, children)
}

fn channel_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let metrics = theme.metrics;
    let scene = state.scene;
    let name = scene.dataset.channels()[index].name().to_string();
    let tint = Color::from_rgb_f32(scene.dataset.channels()[index].tint());
    let enabled = scene.channels[index].enabled;
    let opacity = scene.channels[index].opacity;
    let active = scene.selected == index;
    let font = if active {
        theme.font(Level::Body).medium()
    } else {
        theme.font(Level::Body)
    };
    let mono = theme.font(Level::Monospace);
    let switch_left = metrics.switch_width + 10.0;
    let width = card_width() - switch_left;
    let canvas = grana_ui::canvas(Length::Fixed(width), CHANNEL_ROW, move |painter, info| {
        let rect = info.rect;
        let full = Rect::from_min_max(
            rect.min,
            Vec2::new(rect.max.x + switch_left + SWITCH_TRAIL, rect.max.y),
        );
        if active {
            painter.outlined(
                full,
                Corners::all(9.0),
                Fill::Solid(palette.accent.with_alpha(0.16)),
                Border {
                    width: 1.0,
                    color: palette.accent.with_alpha(0.34),
                },
                Shadow::NONE,
            );
        } else if info.hover > 0.01 {
            painter.rect(
                full,
                Corners::all(9.0),
                palette.material_hover.amplify(0.50 * info.hover),
            );
        }
        let dot = Rect::from_center_size(
            Vec2::new(rect.min.x + 12.0, rect.center().y),
            Vec2::splat(8.5),
        );
        if active {
            painter.ring(
                dot.center(),
                6.8,
                1.2,
                0.0,
                std::f32::consts::TAU,
                tint.with_alpha(if enabled { 0.48 } else { 0.20 }),
            );
        }
        painter.rect(
            dot,
            Corners::capsule(),
            tint.with_alpha(if enabled { 1.0 } else { 0.28 }),
        );
        let label_right = if enabled && opacity < 0.995 {
            let badge = Rect::from_min_max(
                Vec2::new(rect.max.x + switch_left - 42.0, rect.min.y),
                Vec2::new(rect.max.x + switch_left - 8.0, rect.max.y),
            );
            painter.text_in(
                badge,
                &format!("{:.0}%", opacity * 100.0),
                TextStyle::new(mono, palette.text_tertiary).aligned(Align::End),
                Align::Center,
            );
            badge.min.x - 4.0
        } else {
            rect.max.x + switch_left - 8.0
        };
        let color = if !enabled {
            palette.text_quaternary
        } else if active {
            palette.text_primary
        } else {
            palette.text_secondary
        };
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 25.0, rect.min.y),
                Vec2::new(label_right, rect.max.y),
            ),
            &name,
            TextStyle::new(font, color).truncating(),
            Align::Center,
        );
    })
    .key(Key::index("channel.pick", index))
    .press(Message::Command(Command::SelectChannel(index)));
    let switch = grana_ui::widget::toggle(&theme, enabled)
        .key(Key::index("channel.enabled", index))
        .on_toggle(move |value| {
            Message::Command(Command::SetChannelEnabled {
                channel: index,
                enabled: value,
            })
        });
    let gap = grana_ui::canvas(
        Length::Fixed(switch_left - metrics.switch_width - SWITCH_TRAIL),
        0.0,
        |_painter, _info| {},
    );
    grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(CHANNEL_ROW))
        .align(Align::Center)
        .padding(Edges {
            right: SWITCH_TRAIL,
            ..Edges::ZERO
        })
        .children([canvas, gap, centered(CHANNEL_ROW, switch)])
}

fn display<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let scene = state.scene;
    let index = scene.selected;
    let channel_name = scene.channel(index).name().to_string();
    let tint = scene.channel(index).tint();
    let data = scene.channel(index).intensity();
    let current = scene.channels[index].window;
    let start = current.min();
    let end = current.max();
    let available = data.min()..=data.max();
    let opacity = scene.channels[index].opacity;
    let transfer = scene.channels[index].transfer().clone();
    let gradient = transfer_colors(&transfer);
    let width = card_width();
    let mut children = Vec::new();
    children.push(view::card_title(
        ctx,
        Icon::Sliders,
        "Display",
        Some(&channel_name),
    ));
    children.push(
        grana_ui::widget::range_slider(start, end, available, &gradient)
            .key(Key::new("display.window"))
            .on_range(move |low, high| {
                Message::Command(Command::SetWindow {
                    channel: index,
                    window: Intensity::new(low.min(high), low.max(high)),
                })
            }),
    );
    let mono = theme.font(Level::Monospace);
    let minimum = format!("Min {start:.0}");
    let maximum = format!("Max {end:.0}");
    children.push(grana_ui::canvas(
        Length::Fill,
        14.0,
        move |painter, info| {
            let rect = info.rect;
            let style = TextStyle::new(mono, palette.text_tertiary);
            painter.text_in(
                Rect::from_min_size(rect.min, Vec2::new(rect.width() * 0.5, rect.height())),
                &minimum,
                style,
                Align::Center,
            );
            painter.text_in(
                rect,
                &maximum,
                TextStyle::new(mono, palette.text_tertiary).aligned(Align::End),
                Align::Center,
            );
        },
    ));
    let opacity_readout = format!("{:.0}%", opacity * 100.0);
    children.push(view::slider_readout(
        ctx,
        "display.opacity",
        Some("Opacity"),
        opacity,
        0.0..=1.0,
        false,
        &opacity_readout,
        move |value| Command::SetChannelOpacity {
            channel: index,
            opacity: value.clamp(0.0, 1.0),
        },
    ));
    let preset = (width - 12.0) / 3.0;
    let auto = view::button_with(ctx, "Auto", Role::Control, Length::Fixed(preset))
        .press(Message::Command(Command::ResetWindow(index)));
    let tinted = view::button_with(ctx, "Tinted", Role::Control, Length::Fixed(preset)).press(
        Message::Command(Command::SetTransfer {
            channel: index,
            transfer: TransferFunction::tinted(tint),
        }),
    );
    let gray = view::button_with(ctx, "Gray", Role::Control, Length::Fixed(preset)).press(
        Message::Command(Command::SetTransfer {
            channel: index,
            transfer: TransferFunction::grayscale(),
        }),
    );
    children.push(
        grana_ui::row()
            .gap(6.0)
            .align(Align::Center)
            .children([auto, tinted, gray]),
    );
    let derived = (index >= scene.base_channels())
        .then(|| scene.derived().get(index - scene.base_channels()))
        .flatten()
        .map(|entry| entry.spec().clone());
    if let Some(operation) = derived.as_ref().map(|spec| spec.operation().clone()) {
        children.push(view::divider(ctx));
        match &operation {
            grana_scene::Operation::Restore(restore) => children.extend(restore_fields(
                ctx,
                index,
                restore,
                scene.dataset.world_size().z.max(1.0),
            )),
            grana_scene::Operation::Smooth(smooth) => children.push(view::slider_event(
                ctx,
                "operator.sigma",
                Some("Sigma"),
                smooth.sigma(),
                0.05..=8.0,
                &format!("{:.2} µm", smooth.sigma()),
                move |value| {
                    Message::Command(Command::SetSmoothChannel {
                        channel: index,
                        sigma: value.clamp(0.05, 8.0),
                    })
                },
            )),
            grana_scene::Operation::Background(background) => {
                children.push(view::slider_event(
                    ctx,
                    "operator.radius",
                    Some("Radius"),
                    background.radius(),
                    0.2..=40.0,
                    &format!("{:.2} µm", background.radius()),
                    move |value| {
                        Message::Command(Command::SetBackgroundChannel {
                            channel: index,
                            radius: value.clamp(0.2, 40.0),
                        })
                    },
                ));
            }
            grana_scene::Operation::Ratio(ratio) => {
                let denominator = scene.channel(ratio.denominator()).name().to_string();
                children.push(view::weak(
                    ctx,
                    &format!(
                        "Divides {} by {denominator}",
                        scene
                            .channel(derived.as_ref().expect("a ratio channel exists").source())
                            .name()
                    ),
                ));
            }
            grana_scene::Operation::Mask { .. } => {}
        }
    }
    children.push(view::divider(ctx));
    let voxel = scene.dataset.voxel_size().min_element();
    let sigma = voxel.max(0.05);
    let radius = (10.0 * voxel).max(1.0);
    let third = (width - 12.0) / 3.0;
    children.push(view::caption(ctx, "Channel operators"));
    children.push(
        grana_ui::row().gap(6.0).align(Align::Center).children([
            view::button_with(ctx, "Smooth", Role::Quiet, Length::Fixed(third))
                .named("operator.smooth")
                .press(Message::Command(Command::AddSmoothChannel {
                    source: index,
                    sigma,
                })),
            view::button_with(ctx, "Background", Role::Quiet, Length::Fixed(third))
                .named("operator.background")
                .press(Message::Command(Command::AddBackgroundChannel {
                    source: index,
                    radius,
                })),
            view::button_with(ctx, "Ratio", Role::Quiet, Length::Fixed(third))
                .named("display.ratio")
                .press(Message::Noop),
        ]),
    );
    let ratio_edit = matches!(
        derived.as_ref().map(|spec| spec.operation()),
        Some(grana_scene::Operation::Ratio(_))
    );
    let mut ratio_rows = Vec::new();
    for (candidate, info) in scene.dataset.channels().iter().enumerate() {
        if candidate == index {
            continue;
        }
        let command = if ratio_edit {
            Command::SetRatioChannel {
                channel: index,
                denominator: candidate,
            }
        } else {
            Command::AddRatioChannel {
                numerator: index,
                denominator: candidate,
            }
        };
        ratio_rows.push(view::menu_row(
            ctx,
            &format!("display.ratio.{candidate}"),
            info.name(),
            false,
            Some(Message::Command(command)),
        ));
    }
    children.push(view::menu(
        ctx,
        Key::new("display.ratio"),
        220.0,
        ratio_rows,
    ));
    children.push(view::divider(ctx));
    children.push(
        grana_ui::row()
            .justify(Align::End)
            .width(Length::Fill)
            .children([view::button_with(
                ctx,
                "Export Dataset",
                Role::Quiet,
                Length::Fixed(width),
            )
            .named("display.export")
            .press(Message::Command(Command::ExportDataset))]),
    );
    if derived.is_some() {
        children.push(
            grana_ui::row()
                .justify(Align::End)
                .width(Length::Fill)
                .children([view::button_with(
                    ctx,
                    "Remove Channel",
                    Role::Quiet,
                    Length::Fixed(width),
                )
                .named("display.derived.remove")
                .press(Message::Command(Command::RemoveDerivedChannel {
                    channel: index,
                }))]),
        );
    } else {
        let depth = scene.dataset.world_size().z * 0.5;
        let button = (width - 6.0) * 0.5;
        let psf_key = Key::new("display.psf");
        let psf_open = state.workspace.sections.is_open(psf_key);
        let mut calibration = Vec::new();
        if psf_open {
            calibration.push(view::slider_event(
                ctx,
                "restore.bead_threshold",
                Some("Bead Threshold"),
                state.workspace.psf_threshold,
                2.0..=20.0,
                &format!("{:.1} σ", state.workspace.psf_threshold),
                Message::SetPsfThreshold,
            ));
            calibration.push(view::slider_event(
                ctx,
                "restore.bead_separation",
                Some("Bead Spacing"),
                state.workspace.psf_separation,
                0.3..=5.0,
                &format!("{:.1} µm", state.workspace.psf_separation),
                Message::SetPsfSeparation,
            ));
        }
        children.push(disclosure(
            ctx,
            "display.psf",
            "Bead Calibration",
            psf_open,
            calibration,
        ));
        children.push(
            grana_ui::row()
                .gap(6.0)
                .justify(Align::End)
                .width(Length::Fill)
                .children([
                    view::button_with(ctx, "Deconvolve", Role::Quiet, Length::Fixed(button))
                        .named("display.restore.add")
                        .press(Message::Command(Command::AddRestoreChannel {
                            source: index,
                            psf: Psf::theoretical(PsfParams::new(
                                DEFAULT_APERTURE,
                                DEFAULT_WAVELENGTH,
                                DEFAULT_IMMERSION,
                                DEFAULT_SAMPLE,
                                depth,
                            )),
                            iterations: DEFAULT_ITERATIONS,
                        })),
                    view::button_with(ctx, "From Beads", Role::Quiet, Length::Fixed(button))
                        .named("display.restore.beads")
                        .press(Message::Command(Command::CalibratePsf {
                            source: index,
                            threshold: state.workspace.psf_threshold,
                            separation: state.workspace.psf_separation,
                        })),
                ]),
        );
    }
    children.push(view::divider(ctx));
    let key = Key::new("display.transfer");
    let open = state.workspace.sections.is_open(key);
    let mut curve = Vec::new();
    if open {
        curve.extend(crate::transfer::edit(
            ctx,
            "display.transfer",
            index,
            &transfer,
            tint,
            current,
            width,
        ));
    }
    children.push(disclosure(
        ctx,
        "display.transfer",
        "Transfer Curve",
        open,
        curve,
    ));
    view::card(ctx, 7.0, children)
}

fn section<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let camera = state.viewport.section();
    let azimuth = camera.azimuth();
    let elevation = camera.elevation();
    let normal = camera.normal();
    let axis = if normal.z.abs() >= normal.y.abs() && normal.z.abs() >= normal.x.abs() {
        Axis::Z
    } else if normal.y.abs() >= normal.x.abs() {
        Axis::Y
    } else {
        Axis::X
    };
    let current = match axis {
        Axis::Z => 0,
        Axis::Y => 1,
        Axis::X => 2,
    };
    let spacing = state.scene.dataset.spacing();
    let step =
        spacing.x() * normal.x.abs() + spacing.y() * normal.y.abs() + spacing.z() * normal.z.abs();
    let detail = format!("{step:.3} µm step");
    let mut children = Vec::new();
    children.push(view::card_title(
        ctx,
        Icon::Section,
        "Section",
        Some(&detail),
    ));
    let options = ["XY", "XZ", "YZ"];
    let plane = view::segmented_fill(ctx, "section.plane", &options, current, |index| {
        Message::SetSectionPreset(index)
    });
    children.push(view::form_row(ctx, "Plane", plane));
    let world = state.scene.dataset.world_size();
    let extent = world.abs().dot(normal.abs());
    let projection = camera.projection();
    let modes = grana_display::Projection::ALL.map(grana_display::Projection::label);
    let view = view::segmented_fill(
        ctx,
        "section.projection",
        &modes,
        projection.index(),
        Message::SetSectionProjection,
    );
    children.push(view::form_row(ctx, "View", view));
    if projection.projected() {
        let thickness = camera.thickness();
        let maximum = extent.max(thickness).max(1.0e-3);
        children.push(
            grana_ui::widget::slider_readout(
                &theme,
                Some("Thickness"),
                thickness,
                0.0..=maximum,
                false,
                &format!("{thickness:.2} µm"),
            )
            .named("section.thickness")
            .on_slide(Message::SetSectionThickness),
        );
    }
    let elevation_degrees = elevation.to_degrees();
    children.push(
        grana_ui::widget::slider_readout(
            &theme,
            Some("Elevation"),
            elevation_degrees,
            -90.0..=90.0,
            false,
            &format!("{elevation_degrees:.0}°"),
        )
        .named("section.elevation")
        .on_slide(move |value| Message::SetSectionOrientation {
            azimuth,
            elevation: value.to_radians(),
        }),
    );
    let azimuth_degrees = azimuth.to_degrees();
    children.push(
        grana_ui::widget::slider_readout(
            &theme,
            Some("Azimuth"),
            azimuth_degrees,
            -180.0..=180.0,
            false,
            &format!("{azimuth_degrees:.0}°"),
        )
        .named("section.azimuth")
        .on_slide(move |value| Message::SetSectionOrientation {
            azimuth: value.to_radians(),
            elevation,
        }),
    );
    let position = state.viewport.section().center().dot(normal) + 0.5 * extent;
    let readout = format!("{position:.2} µm");
    children.push(
        grana_ui::widget::slider_readout(
            &theme,
            Some("Depth"),
            position,
            0.0..=extent,
            false,
            &readout,
        )
        .named("section.depth")
        .on_slide(Message::SetSectionDepth),
    );
    view::card(ctx, 8.0, children)
}

fn region<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let current = state.scene.region;
    let size = current.size();
    let summary = if current.enabled() {
        format!("{:.1}×{:.1}×{:.1} µm", size.x, size.y, size.z)
    } else {
        "Whole volume".to_string()
    };
    let key = Key::new("region");
    let open = state.workspace.sections.is_open(key);
    let mut children = Vec::new();
    if open {
        let world = state.scene.dataset.world_size();
        let half = 0.5 * world;
        let minimum = (0.001 * world.min_element()).max(1.0e-4);
        let reach = world.length().max(minimum);
        children.push(
            grana_ui::widget::toggle_row(&theme, "Limit Analysis", current.enabled())
                .named("region.enabled")
                .on_toggle(move |value| {
                    let mut region = current;
                    region.set_enabled(value);
                    Message::Command(Command::SetRegion(region))
                }),
        );
        for (name, index) in [("Center X", 0), ("Center Y", 1), ("Center Z", 2)] {
            let value = current.center()[index];
            let readout = format!("{value:.2} µm");
            children.push(
                grana_ui::widget::slider_readout(
                    &theme,
                    Some(name),
                    value,
                    -half[index]..=half[index],
                    false,
                    &readout,
                )
                .named(&format!("region.center.{index}"))
                .on_slide(move |value| {
                    let mut region = current;
                    let mut center = current.center();
                    center[index] = value.clamp(-half[index], half[index]);
                    region.set_center(center);
                    Message::Command(Command::SetRegion(region))
                }),
            );
        }
        for (name, index) in [("Size X", 0), ("Size Y", 1), ("Size Z", 2)] {
            let value = size[index];
            let readout = format!("{value:.2} µm");
            children.push(
                grana_ui::widget::slider_readout(
                    &theme,
                    Some(name),
                    value,
                    minimum..=2.0 * reach,
                    false,
                    &readout,
                )
                .named(&format!("region.size.{index}"))
                .on_slide(move |value| {
                    let mut region = current;
                    let mut extents = current.half();
                    extents[index] = 0.5 * value.clamp(minimum, 2.0 * reach);
                    region.set_half(extents);
                    Message::Command(Command::SetRegion(region))
                }),
            );
        }
        let align = {
            let section = state.viewport.section();
            let axes = [section.right(), section.up(), section.normal()];
            let region = grana_volume::Region::new(current.center(), axes, current.half());
            Message::Command(Command::SetRegion(region))
        };
        let fill = Message::Command(Command::SetRegion(grana_volume::Region::full(-half, half)));
        let buttons = grana_ui::row().gap(8.0).children([
            view::button_with(ctx, "Align to Section", Role::Control, Length::Fill).press(align),
            view::button_with(ctx, "Fill Volume", Role::Control, Length::Fill).press(fill),
        ]);
        children.push(buttons);
    }
    view::disclosure_card(
        ctx,
        key,
        Icon::Fit,
        "Region of Interest",
        Some(&summary),
        children,
        open,
    )
}

fn drift<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let drift = state.scene.drift();
    let running = view::progress(state.jobs, grana_scene::Job::Drift);
    let summary = if running.is_some() {
        "Aligning…".to_string()
    } else if drift.is_empty() {
        "Off".to_string()
    } else {
        format!("{} frames", drift.frames())
    };
    let key = Key::new("drift");
    let open = state.workspace.sections.is_open(key);
    let mut children = Vec::new();
    if open {
        match running {
            Some(progress) => {
                let text = if progress.total > 0 {
                    format!("Aligning · {}/{} frames", progress.done, progress.total)
                } else {
                    "Aligning frames…".to_string()
                };
                let spinner = view::spinner(ctx, 12.0);
                let caption = view::caption(ctx, &text);
                children.push(
                    grana_ui::row()
                        .gap(8.0)
                        .align(Align::Center)
                        .children([spinner, caption]),
                );
                children.push(
                    view::button_with(ctx, "Cancel", Role::Control, Length::Fill)
                        .press(Message::Command(Command::CancelDrift)),
                );
            }
            None => {
                children.push(
                    view::button_with(ctx, "Correct Drift", Role::Control, Length::Fill)
                        .press(Message::Command(Command::ComputeDrift)),
                );
                if drift.is_empty() {
                    let channel = state.scene.dataset.channels()[state.scene.selected]
                        .name()
                        .to_string();
                    children.push(view::weak(
                        ctx,
                        &format!("Aligns {channel} across all time frames."),
                    ));
                } else {
                    let frames = drift.frames();
                    let reference = drift.reference();
                    let time = state.scene.time();
                    children.push(view::caption(
                        ctx,
                        &format!(
                            "{frames} frames aligned · max {:.1} voxels",
                            drift.magnitude()
                        ),
                    ));
                    let reference_field = grana_ui::widget::drag_value(&theme, reference as f32, 0)
                        .named("drift.reference")
                        .on_drag_value(0.2, move |value| {
                            Message::Command(Command::SetDriftReference(
                                value.round().clamp(0.0, (frames - 1) as f32) as u32,
                            ))
                        });
                    let unit = view::caption(ctx, "frame");
                    let row = grana_ui::row()
                        .gap(6.0)
                        .align(Align::Center)
                        .children([reference_field, unit]);
                    children.push(view::form_row(ctx, "Reference", row));
                    children.push(frame_table(ctx, state, time));
                    children.push(
                        view::button_with(ctx, "Clear Drift", Role::Danger, Length::Fill)
                            .press(Message::Command(Command::ClearDrift)),
                    );
                }
            }
        }
    }
    view::disclosure_card(
        ctx,
        key,
        Icon::Target,
        "Drift Correction",
        Some(&summary),
        children,
        open,
    )
}

fn frame_table<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    time: u32,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let drift = state.scene.drift();
    let reference = drift.reference();
    let mut rows = Vec::new();
    for frame in 0..drift.frames() {
        let offset = drift.offset(frame);
        let mut label = format!("Frame {}", frame + 1);
        if frame == time {
            label.push_str(" •");
        }
        if frame == reference {
            label.push_str(" (ref)");
        }
        rows.push(view::labelled(
            ctx,
            &label,
            &format!("{:+0.2} {:+0.2} {:+0.2}", offset[0], offset[1], offset[2]),
        ));
    }
    let list = grana_ui::column().gap(0.0).children(rows);
    let mut children = vec![
        Element::new(grana_ui::Layout::column())
            .named("drift.frames")
            .scroll(grana_ui::Axis::Column, Length::Fixed(116.0), None)
            .clipped()
            .children([list]),
    ];
    if time == reference {
        children.push(view::weak(ctx, "Reference frame is fixed at zero offset."));
    } else {
        let offset = drift.offset(time);
        for (axis, name) in ["Offset X", "Offset Y", "Offset Z"].into_iter().enumerate() {
            let field = grana_ui::widget::drag_value(&theme, offset[axis], 2)
                .named(&format!("drift.offset.{axis}"))
                .on_drag_value(0.5, move |value| {
                    let mut next = offset;
                    next[axis] = (value * 100.0).round() / 100.0;
                    Message::Command(Command::SetDriftOffset { time, offset: next })
                });
            let unit = view::caption(ctx, "vx");
            let row = grana_ui::row()
                .gap(6.0)
                .align(Align::Center)
                .children([field, unit]);
            children.push(view::form_row(ctx, name, row));
        }
    }
    grana_ui::column().gap(6.0).children(children)
}

fn mask<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let sources = state
        .scene
        .objects
        .iter()
        .map(|set| (set.id(), set.name().to_string(), set.filtered()))
        .collect::<Vec<_>>();
    let current = state.scene.mask;
    let summary = current
        .and_then(|id| sources.iter().find(|(set, _, _)| *set == id))
        .map_or_else(|| "Whole volume".to_string(), |(_, name, _)| name.clone());
    let key = Key::new("mask.card");
    let open = state.workspace.sections.is_open(key);
    let mut children = Vec::new();
    if open {
        let mut options = vec![(None, "Whole Volume".to_string())];
        options.extend(
            sources
                .iter()
                .map(|(id, name, _)| (Some(*id), name.clone())),
        );
        children.push(form_combo(
            ctx,
            "mask",
            "Source",
            &options,
            Some(current),
            card_width() - crate::panel::FORM_LABEL,
            move |chosen| {
                if chosen == current {
                    Message::Noop
                } else {
                    Message::Command(Command::SetMask(chosen))
                }
            },
        ));
        let limit = state.scene.dataset.world_size().min_element().max(1.0);
        let radius = state.scene.mask_radius;
        let readout = format!("{radius:.2} µm");
        children.push(view::slider_readout(
            ctx,
            "mask.radius",
            Some("Grow by"),
            radius,
            0.0..=limit,
            false,
            &readout,
            Command::SetMaskRadius,
        ));
        if let Some(source) = current {
            let relations = state
                .scene
                .objects
                .get(source)
                .map(|set| {
                    set.relations()
                        .iter()
                        .filter_map(|relation| {
                            let target = state.scene.objects.get(relation.target())?;
                            let label = if relation.shift() == 0 {
                                target.name().to_string()
                            } else {
                                format!("{} · t{:+}", target.name(), relation.shift())
                            };
                            Some((Some((relation.target(), relation.shift())), label))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !relations.is_empty() {
                let mut options = vec![(None, "Any Relation".to_string())];
                options.extend(relations);
                let selected = state
                    .scene
                    .mask_filter
                    .map(|filter| (filter.target(), filter.shift()));
                let current_filter = state.scene.mask_filter;
                let world = state.scene.dataset.world_size();
                let width = card_width() - crate::panel::FORM_LABEL;
                children.push(form_combo(
                    ctx,
                    "mask.relation",
                    "Relation",
                    &options,
                    Some(selected),
                    width,
                    move |chosen| {
                        let Some((target, shift)) = chosen else {
                            return Message::Command(Command::SetMaskFilter(None));
                        };
                        let (field, range) = match current_filter {
                            Some(filter) => (filter.field(), filter.range()),
                            None => (
                                RelationField::Distance,
                                default_filter_range(RelationField::Distance, world),
                            ),
                        };
                        Message::Command(Command::SetMaskFilter(Some(MaskFilter::new(
                            target, shift, field, range[0], range[1],
                        ))))
                    },
                ));
                if let Some(filter) = current_filter {
                    let theme = *ctx.theme();
                    let field = filter.field();
                    let target = filter.target();
                    let shift = filter.shift();
                    let range = filter.range();
                    let fields = RelationField::CONTINUOUS
                        .into_iter()
                        .map(|field| (field, field.label().to_string()))
                        .collect::<Vec<_>>();
                    children.push(form_combo(
                        ctx,
                        "mask.field",
                        "Field",
                        &fields,
                        Some(field),
                        width,
                        move |chosen| {
                            let range = if chosen == field {
                                range
                            } else {
                                default_filter_range(chosen, world)
                            };
                            Message::Command(Command::SetMaskFilter(Some(MaskFilter::new(
                                target, shift, chosen, range[0], range[1],
                            ))))
                        },
                    ));
                    let suffix = field.unit().suffix();
                    for (index, (label, value)) in [("Minimum", range[0]), ("Maximum", range[1])]
                        .into_iter()
                        .enumerate()
                    {
                        let other = match index {
                            0 => range[1],
                            _ => range[0],
                        };
                        let action = move |value: f32| {
                            let (minimum, maximum) = if index == 0 {
                                (value.min(other), other)
                            } else {
                                (other, value.max(other))
                            };
                            Message::Command(Command::SetMaskFilter(Some(MaskFilter::new(
                                target, shift, field, minimum, maximum,
                            ))))
                        };
                        let widget =
                            grana_ui::widget::drag_value(&theme, value, field.decimals() as usize)
                                .named(&format!("mask.filter.{index}"))
                                .on_drag_value(unit_step(field), action);
                        let unit = view::caption(ctx, suffix);
                        let row = grana_ui::row()
                            .gap(6.0)
                            .align(Align::Center)
                            .children([widget, unit]);
                        children.push(view::form_row(ctx, label, row));
                    }
                    children.push(view::weak(
                        ctx,
                        &format!(
                            "Detects only where the mask relation keeps objects whose {} lies in {:.2}{suffix} to {:.2}{suffix}.",
                            field.label(),
                            range[0],
                            range[1]
                        ),
                    ));
                    let target_name = state.scene.objects.get(target).map_or_else(
                        || "the relation target".to_string(),
                        |set| set.name().to_string(),
                    );
                    children.push(view::weak(
                        ctx,
                        &format!(
                            "{target_name} is detected without the mask so its relation can limit the mask."
                        ),
                    ));
                }
            }
        }
        children.push(
            match current.and_then(|id| sources.iter().find(|(set, _, _)| *set == id)) {
                Some((_, name, true)) => {
                    view::weak(ctx, &format!("Detects inside filtered objects of {name}."))
                }
                Some((_, name, false)) => {
                    view::weak(ctx, &format!("Detects inside {name} objects only."))
                }
                None => view::weak(ctx, "Detects across the entire dataset."),
            },
        );
    }
    view::disclosure_card(
        ctx,
        key,
        Icon::Filter,
        "Analysis Mask",
        Some(&summary),
        children,
        open,
    )
}

fn default_filter_range(field: RelationField, world: Vec3) -> [f32; 2] {
    let reach = (0.5 * world.length()).max(1.0);
    match field {
        RelationField::Distance | RelationField::Center => [0.0, reach],
        RelationField::Inside => [0.0, 100.0],
        RelationField::Contact => [0.0, reach * reach],
        RelationField::Partner | RelationField::Overlapping => [0.0, 0.0],
    }
}

fn unit_step(field: RelationField) -> f32 {
    match field {
        RelationField::Distance | RelationField::Center => 0.1,
        RelationField::Inside
        | RelationField::Contact
        | RelationField::Partner
        | RelationField::Overlapping => 1.0,
    }
}

fn clip<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let current = state.scene.clip;
    let status = if current.enabled() { "Active" } else { "Off" };
    let key = Key::new("clip");
    let open = state.workspace.sections.is_open(key);
    let mut children = Vec::new();
    if open {
        let world = state.scene.dataset.world_size();
        let reach = 0.5 * world.length();
        let enabled = current.enabled();
        let normal = if enabled { current.normal() } else { Vec3::X };
        let offset = if enabled { current.offset() } else { 0.0 };
        let azimuth = normal.y.atan2(normal.x).to_degrees();
        let elevation = normal.z.clamp(-1.0, 1.0).asin().to_degrees();
        let width = card_width();
        let apply = move |azimuth: f32, elevation: f32, offset: f32, enabled: bool| {
            Message::Command(Command::SetClip(if enabled {
                Clip::new(direction(azimuth, elevation), offset.clamp(-reach, reach))
            } else {
                Clip::none()
            }))
        };
        children.push(
            grana_ui::widget::toggle_row(&theme, "Cut Volume", enabled)
                .named("clip.enabled")
                .on_toggle(move |value| apply(azimuth, elevation, offset, value)),
        );
        let azimuth_text = format!("{azimuth:.0}°");
        children.push(
            grana_ui::widget::slider_readout(
                &theme,
                Some("Azimuth"),
                azimuth,
                -180.0..=180.0,
                false,
                &azimuth_text,
            )
            .named("clip.azimuth")
            .on_slide(move |value| apply(value, elevation, offset, true)),
        );
        let elevation_text = format!("{elevation:.0}°");
        children.push(
            grana_ui::widget::slider_readout(
                &theme,
                Some("Elevation"),
                elevation,
                -90.0..=90.0,
                false,
                &elevation_text,
            )
            .named("clip.elevation")
            .on_slide(move |value| apply(azimuth, value, offset, true)),
        );
        let offset_text = format!("{offset:.2} µm");
        children.push(
            grana_ui::widget::slider_readout(
                &theme,
                Some("Offset"),
                offset,
                -reach..=reach,
                false,
                &offset_text,
            )
            .named("clip.offset")
            .on_slide(move |value| apply(azimuth, elevation, value, true)),
        );
        let half = (width - 6.0) * 0.5;
        let flip = view::button_with(ctx, "Flip Plane", Role::Control, Length::Fixed(half))
            .press(apply(azimuth + 180.0, -elevation, -offset, true));
        let reset = view::button_with(ctx, "Reset", Role::Control, Length::Fixed(half))
            .press(Message::Command(Command::SetClip(Clip::none())));
        children.push(
            grana_ui::row()
                .gap(6.0)
                .align(Align::Center)
                .children([flip, reset]),
        );
    }
    view::disclosure_card(
        ctx,
        key,
        Icon::Cube,
        "Clipping Plane",
        Some(status),
        children,
        open,
    )
}

fn direction(azimuth: f32, elevation: f32) -> Vec3 {
    let (sine_azimuth, cosine_azimuth) = azimuth.to_radians().sin_cos();
    let (sine_elevation, cosine_elevation) = elevation.to_radians().sin_cos();
    Vec3::new(
        cosine_elevation * cosine_azimuth,
        cosine_elevation * sine_azimuth,
        sine_elevation,
    )
}

fn rendering<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let scene = state.scene;
    let shading = scene.shading;
    let key = Key::new("rendering");
    let open = state.workspace.sections.is_open(key);
    let mut children = Vec::new();
    if open {
        let exposure = format!("{:.2}", scene.exposure);
        children.push(view::slider_readout(
            ctx,
            "exposure",
            Some("Exposure"),
            scene.exposure,
            0.05..=20.0,
            true,
            &exposure,
            Command::SetExposure,
        ));
        let step = format!("{:.2}", scene.step_scale);
        children.push(view::slider_readout(
            ctx,
            "step",
            Some("Sampling"),
            scene.step_scale,
            0.25..=2.0,
            true,
            &step,
            Command::SetStepScale,
        ));
        let shadows = format!("{:.2}", shading.shadows);
        children.push(view::slider_readout(
            ctx,
            "shadows",
            Some("Shadows"),
            shading.shadows,
            0.0..=1.0,
            false,
            &shadows,
            move |value| {
                Command::SetShading(grana_display::Shading {
                    shadows: value,
                    ..shading
                })
            },
        ));
        children.push(view::divider(ctx));
        let lighting = Key::new("rendering.lighting");
        let lighting_open = state.workspace.sections.is_open(lighting);
        let mut light = Vec::new();
        if lighting_open {
            let azimuth = format!("{:.0}°", shading.azimuth);
            light.push(view::slider_readout(
                ctx,
                "lighting.azimuth",
                Some("Azimuth"),
                shading.azimuth,
                -grana_display::AZIMUTH_LIMIT..=grana_display::AZIMUTH_LIMIT,
                false,
                &azimuth,
                move |value| {
                    Command::SetShading(grana_display::Shading {
                        azimuth: value,
                        ..shading
                    })
                },
            ));
            let elevation = format!("{:.0}°", shading.elevation);
            light.push(view::slider_readout(
                ctx,
                "lighting.elevation",
                Some("Elevation"),
                shading.elevation,
                -grana_display::ELEVATION_LIMIT..=grana_display::ELEVATION_LIMIT,
                false,
                &elevation,
                move |value| {
                    Command::SetShading(grana_display::Shading {
                        elevation: value,
                        ..shading
                    })
                },
            ));
            let ambient = format!("{:.2}", shading.ambient);
            light.push(view::slider_readout(
                ctx,
                "lighting.ambient",
                Some("Ambient"),
                shading.ambient,
                0.0..=1.0,
                false,
                &ambient,
                move |value| {
                    Command::SetShading(grana_display::Shading {
                        ambient: value,
                        ..shading
                    })
                },
            ));
            let diffuse = format!("{:.2}", shading.diffuse);
            light.push(view::slider_readout(
                ctx,
                "lighting.diffuse",
                Some("Diffuse"),
                shading.diffuse,
                0.0..=1.0,
                false,
                &diffuse,
                move |value| {
                    Command::SetShading(grana_display::Shading {
                        diffuse: value,
                        ..shading
                    })
                },
            ));
            let specular = format!("{:.2}", shading.specular);
            light.push(view::slider_readout(
                ctx,
                "lighting.specular",
                Some("Specular"),
                shading.specular,
                0.0..=1.0,
                false,
                &specular,
                move |value| {
                    Command::SetShading(grana_display::Shading {
                        specular: value,
                        ..shading
                    })
                },
            ));
            let shininess = format!("{:.0}", shading.shininess);
            light.push(view::slider_readout(
                ctx,
                "lighting.shininess",
                Some("Shininess"),
                shading.shininess,
                1.0..=128.0,
                true,
                &shininess,
                move |value| {
                    Command::SetShading(grana_display::Shading {
                        shininess: value,
                        ..shading
                    })
                },
            ));
        }
        children.push(disclosure(
            ctx,
            "rendering.lighting",
            "Surface Lighting",
            lighting_open,
            light,
        ));
        children.push(
            view::button_with(ctx, "Reset Rendering", Role::Control, Length::Fill)
                .press(Message::Command(Command::ResetRendering)),
        );
    }
    let summary = format!("{:.2}×", scene.exposure);
    view::disclosure_card(
        ctx,
        key,
        Icon::Sparkles,
        "Rendering",
        Some(&summary),
        children,
        open,
    )
}

pub(crate) fn disclosure<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    title: &str,
    open: bool,
    children: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let font = theme.font(Level::Callout).medium();
    let token = Key::new(key);
    let arrow = ctx.animate(
        token.then("arrow").id(),
        if open { 1.0 } else { 0.0 },
        Motion::response(0.32, 0.18),
    );
    let label = title.to_string();
    let header = grana_ui::canvas(Length::Fill, 26.0, move |painter, info| {
        let rect = info.rect;
        if info.hover > 0.01 {
            painter.rect(
                rect.expand(2.0),
                Corners::all(8.0),
                palette.material_hover.amplify(0.45 * info.hover),
            );
        }
        painter.text_in(
            Rect::from_min_max(rect.min, Vec2::new(rect.max.x - 18.0, rect.max.y)),
            &label,
            TextStyle::new(
                font,
                palette
                    .text_secondary
                    .lerp(palette.text_primary, info.hover),
            ),
            Align::Center,
        );
        icon::chevron(
            painter,
            Vec2::new(rect.max.x - 6.0, rect.center().y),
            8.0,
            std::f32::consts::FRAC_PI_2 * arrow,
            palette
                .text_tertiary
                .lerp(palette.text_secondary, info.hover),
            1.5,
        );
    })
    .key(token)
    .press(Message::ToggleOpen(token))
    .focus_ring(Corners::all(8.0))
    .cursor(grana_ui::Cursor::Pointing);
    let mut items = vec![header];
    if open {
        items.push(grana_ui::column().gap(6.0).children(children));
    }
    grana_ui::column().gap(0.0).children(items)
}

pub(crate) fn combo<'a, T: Copy + PartialEq + 'static>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    width: Length,
    popup_width: f32,
    items: &[(T, String)],
    selected: Option<T>,
    select: impl Fn(T) -> Message + 'static,
) -> Element<'a, Message> {
    let labels = items
        .iter()
        .map(|(_, label)| label.clone())
        .collect::<Vec<_>>();
    let index =
        selected.and_then(|selected| items.iter().position(|(value, _)| *value == selected));
    let shown = index.map_or_else(String::new, |index| labels[index].clone());
    let values = items.iter().map(|(value, _)| *value).collect::<Vec<_>>();
    let rows = view::popup_rows(ctx, key, &labels, index.unwrap_or(0), move |index| {
        select(values[index])
    });
    let anchor = combo_anchor(ctx, Key::new(key), shown, width);
    let menu = view::menu(ctx, Key::new(key), popup_width, rows);
    grana_ui::column().gap(0.0).children([anchor, menu])
}

fn combo_anchor<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: Key,
    value: String,
    width: Length,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let font = theme.font(Level::Callout);
    let height = theme.metrics.control_height;
    grana_ui::canvas(width, height, move |painter, info| {
        let rect = info.rect;
        let look = theme
            .surface(Role::Field, State::Rest)
            .lerp(theme.surface(Role::Field, State::Hover), info.hover);
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 9.0, rect.min.y),
                Vec2::new(rect.max.x - 22.0, rect.max.y),
            ),
            &value,
            TextStyle::new(font, look.text).truncating(),
            Align::Center,
        );
        icon::paint(
            Icon::ChevronDown,
            painter,
            Vec2::new(rect.max.x - 10.0, rect.center().y),
            9.0,
            palette.text_tertiary,
            1.5,
        );
    })
    .key(key)
    .press(Message::Noop)
    .focus_ring(Corners::all(10.0))
    .cursor(grana_ui::Cursor::Pointing)
}

pub(crate) fn form_combo<'a, T: Copy + PartialEq + 'static>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: &str,
    items: &[(T, String)],
    selected: Option<T>,
    popup_width: f32,
    select: impl Fn(T) -> Message + 'static,
) -> Element<'a, Message> {
    let content = combo(ctx, key, Length::Fill, popup_width, items, selected, select);
    view::form_row(ctx, label, content)
}

fn transfer_colors(transfer: &TransferFunction) -> Vec<Color> {
    transfer
        .stops()
        .iter()
        .map(|stop| {
            let [red, green, blue] = stop.color();
            Color::rgba(red, green, blue, 1.0)
        })
        .collect()
}
