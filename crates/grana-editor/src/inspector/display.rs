use super::*;

const DEFAULT_APERTURE: f32 = 1.4;
const DEFAULT_WAVELENGTH: f32 = 0.52;
const DEFAULT_IMMERSION: f32 = 1.515;
const DEFAULT_SAMPLE: f32 = 1.33;
const DEFAULT_ITERATIONS: u32 = 10;

pub(super) fn restore_fields<'a>(
    ctx: &mut BuildContext<'_, Message>,
    channel: usize,
    spec: &grana_scene::RestoreSpec,
    depth_limit: f32,
) -> Vec<crate::view::View<'a>> {
    let language = ctx.language();
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
        Some(grana_i18n::inspector::iterations(language)),
        iterations as f32,
        (1.0..=50.0, false),
        &format!("{iterations}"),
        move |value| set(psf.clone(), value.round().max(1.0) as u32),
    ));
    match spec.psf() {
        Psf::Theoretical(params) => {
            let params = *params;
            rows.push(view::slider_readout(
                ctx,
                "restore.aperture",
                Some(grana_i18n::inspector::aperture(language)),
                params.numerical_aperture(),
                (0.3..=1.45, false),
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
                Some(grana_i18n::inspector::emission(language)),
                params.wavelength(),
                (0.35..=0.8, false),
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
                Some(grana_i18n::inspector::immersion(language)),
                params.immersion(),
                (1.3..=1.6, false),
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
                Some(grana_i18n::inspector::specimen(language)),
                params.sample(),
                (1.0..=1.6, false),
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
                Some(grana_i18n::inspector::depth(language)),
                params.depth(),
                (0.0..=depth_limit, false),
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
                &grana_i18n::inspector::measured_psf(
                    language,
                    psf.layers(),
                    dims[0],
                    dims[1],
                    dims[2],
                ),
            ));
            rows.push(
                view::button_with(
                    ctx,
                    grana_i18n::inspector::theoretical_model(language),
                    Role::SurfaceHigh,
                    Length::Fill,
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
                })),
            );
        }
    }
    rows
}

pub(super) fn display<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
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
    let group_gap = theme.metrics.group_gap;
    let inner_sm = theme.metrics.corner_sm;
    let language = ctx.language();
    let mut children = Vec::new();
    children.push(view::card_title(
        ctx,
        Icon::Sliders,
        grana_i18n::inspector::display(language),
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
    let minimum = grana_i18n::inspector::minimum_readout(language, start);
    let maximum = grana_i18n::inspector::maximum_readout(language, end);
    children.push(grana_ui::canvas(Length::Fill, 15.0).view(
        (minimum.clone(), maximum.clone()),
        move |_, painter, info| {
            let rect = info.rect;
            let style = TextStyle::new(mono, palette.on_surface_variant);
            painter.text_in(
                Rect::from_min_size(rect.min, Vec2::new(rect.width() * 0.5, rect.height())),
                &minimum,
                style,
                Align::Center,
            );
            painter.text_in(
                rect,
                &maximum,
                TextStyle::new(mono, palette.on_surface_variant).aligned(Align::End),
                Align::Center,
            );
        },
    ));
    let opacity_readout = format!("{:.0}%", opacity * 100.0);
    children.push(view::slider_readout(
        ctx,
        "display.opacity",
        Some(grana_i18n::inspector::opacity(language)),
        opacity,
        (0.0..=1.0, false),
        &opacity_readout,
        move |value| Command::SetChannelOpacity {
            channel: index,
            opacity: value.clamp(0.0, 1.0),
        },
    ));
    let volume = scene.channels[index].volume();
    let modes = VolumeMode::ALL.map(|mode| volume_mode_label(language, mode));
    children.push(view::segmented_fill(
        ctx,
        "display.volume",
        &modes,
        volume.mode().index(),
        move |selected| {
            Message::Command(Command::SetVolumeMode {
                channel: index,
                mode: VolumeMode::from_index(selected).expect("the segment names a volume mode"),
            })
        },
    ));
    if volume.mode() == VolumeMode::Isosurface {
        let isovalue = volume.isovalue();
        let value = format!("{:.0}", current.value(isovalue));
        children.push(view::slider_readout(
            ctx,
            "display.isovalue",
            Some(grana_i18n::inspector::iso_value(language)),
            isovalue,
            (0.0..=1.0, false),
            &value,
            move |value| Command::SetIsovalue {
                channel: index,
                isovalue: value.clamp(0.0, 1.0),
            },
        ));
    }
    let preset = (width - group_gap * 2.0) / 3.0;
    let auto = view::button_shaped(
        ctx,
        grana_i18n::inspector::auto(language),
        Role::SurfaceHigh,
        Length::Fixed(preset),
        Corners::group_horizontal(0, 3, Corners::CAPSULE, inner_sm, false),
    )
    .press(Message::Command(Command::ResetWindow(index)));
    let tinted = view::button_shaped(
        ctx,
        grana_i18n::inspector::tinted(language),
        Role::SurfaceHigh,
        Length::Fixed(preset),
        Corners::group_horizontal(1, 3, Corners::CAPSULE, inner_sm, false),
    )
    .press(Message::Command(Command::SetTransfer {
        channel: index,
        transfer: TransferFunction::tinted(tint),
    }));
    let gray = view::button_shaped(
        ctx,
        grana_i18n::inspector::gray(language),
        Role::SurfaceHigh,
        Length::Fixed(preset),
        Corners::group_horizontal(2, 3, Corners::CAPSULE, inner_sm, false),
    )
    .press(Message::Command(Command::SetTransfer {
        channel: index,
        transfer: TransferFunction::grayscale(),
    }));
    children.push(
        grana_ui::row()
            .gap(group_gap)
            .align(Align::Center)
            .children([auto, tinted, gray]),
    );
    let derived = (index >= scene.base_channels())
        .then(|| scene.derived().get(index - scene.base_channels()))
        .flatten()
        .map(|entry| entry.spec().clone());
    if let Some(operation) = derived.as_ref().map(|spec| spec.operation().clone()) {
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
                Some(grana_i18n::inspector::sigma(language)),
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
                    Some(grana_i18n::inspector::radius(language)),
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
                    &grana_i18n::inspector::divides(
                        language,
                        scene
                            .channel(derived.as_ref().expect("a ratio channel exists").source())
                            .name(),
                        &denominator,
                    ),
                ));
            }
            grana_scene::Operation::Mask { .. } => {}
            grana_scene::Operation::Bleach => {}
        }
    }
    if state.access.author() {
        let voxel = scene.dataset.voxel_size().min_element();
        let sigma = voxel.max(0.05);
        let radius = (10.0 * voxel).max(1.0);
        let half = (width - group_gap) * 0.5;
        children.push(view::caption(
            ctx,
            grana_i18n::inspector::channel_operators(language),
        ));
        children.push(
            grana_ui::row()
                .gap(group_gap)
                .align(Align::Center)
                .children([
                    view::button_shaped(
                        ctx,
                        grana_i18n::inspector::smooth(language),
                        Role::SurfaceHigh,
                        Length::Fixed(half),
                        Corners::split_leading(inner_sm),
                    )
                    .named("operator.smooth")
                    .press(Message::Command(Command::AddSmoothChannel {
                        source: index,
                        sigma,
                    })),
                    view::button_shaped(
                        ctx,
                        grana_i18n::inspector::background(language),
                        Role::SurfaceHigh,
                        Length::Fixed(half),
                        Corners::split_trailing(inner_sm),
                    )
                    .named("operator.background")
                    .press(Message::Command(Command::AddBackgroundChannel {
                        source: index,
                        radius,
                    })),
                ]),
        );
        children.push(
            grana_ui::row()
                .gap(group_gap)
                .align(Align::Center)
                .children([
                    view::button_shaped(
                        ctx,
                        grana_i18n::inspector::ratio(language),
                        Role::SurfaceHigh,
                        Length::Fixed(half),
                        Corners::split_leading(inner_sm),
                    )
                    .named("display.ratio")
                    .press(Message::Noop),
                    view::button_shaped(
                        ctx,
                        grana_i18n::inspector::bleach(language),
                        Role::SurfaceHigh,
                        Length::Fixed(half),
                        Corners::split_trailing(inner_sm),
                    )
                    .named("operator.bleach")
                    .press(Message::Command(Command::AddBleachChannel {
                        source: index,
                    })),
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
    }
    children.push(
        view::button_with(
            ctx,
            grana_i18n::inspector::export_dataset(language),
            Role::SurfaceHigh,
            Length::Fill,
        )
        .named("display.export")
        .press(Message::Command(Command::ExportDataset)),
    );
    if derived.is_some() && state.access.author() {
        children.push(
            view::button_with(
                ctx,
                grana_i18n::inspector::remove_channel(language),
                Role::Error,
                Length::Fill,
            )
            .named("display.derived.remove")
            .press(Message::Command(Command::RemoveDerivedChannel {
                channel: index,
            })),
        );
    } else if state.access.author() {
        let half = (width - group_gap) * 0.5;
        let depth = scene.dataset.world_size().z * 0.5;
        let psf_key = Key::new("display.psf");
        let psf_open = state.workspace.disclosures.is_open(psf_key);
        let mut calibration = Vec::new();
        if psf_open {
            calibration.push(view::slider_event(
                ctx,
                "restore.bead_threshold",
                Some(grana_i18n::inspector::bead_threshold(language)),
                state.workspace.psf_threshold,
                2.0..=20.0,
                &format!("{:.1} σ", state.workspace.psf_threshold),
                Message::SetPsfThreshold,
            ));
            calibration.push(view::slider_event(
                ctx,
                "restore.bead_separation",
                Some(grana_i18n::inspector::bead_spacing(language)),
                state.workspace.psf_separation,
                0.3..=5.0,
                &format!("{:.1} µm", state.workspace.psf_separation),
                Message::SetPsfSeparation,
            ));
        }
        children.push(disclosure(
            ctx,
            "display.psf",
            grana_i18n::inspector::bead_calibration(language),
            psf_open,
            calibration,
        ));
        children.push(
            grana_ui::row()
                .gap(group_gap)
                .align(Align::Center)
                .width(Length::Fill)
                .children([
                    view::button_shaped(
                        ctx,
                        grana_i18n::inspector::deconvolve(language),
                        Role::Tonal,
                        Length::Fixed(half),
                        Corners::split_leading(inner_sm),
                    )
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
                    view::button_shaped(
                        ctx,
                        grana_i18n::inspector::from_beads(language),
                        Role::SurfaceHigh,
                        Length::Fixed(half),
                        Corners::split_trailing(inner_sm),
                    )
                    .named("display.restore.beads")
                    .press(Message::Command(Command::CalibratePsf {
                        source: index,
                        threshold: state.workspace.psf_threshold,
                        separation: state.workspace.psf_separation,
                    })),
                ]),
        );
    }
    let key = Key::new("display.transfer");
    let open = state.workspace.disclosures.is_open(key);
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
        grana_i18n::inspector::transfer_curve(language),
        open,
        curve,
    ));
    view::card(ctx, 8.0, children)
}

fn volume_mode_label(language: grana_i18n::Language, mode: VolumeMode) -> &'static str {
    match mode {
        VolumeMode::Blend => grana_i18n::inspector::blend(language),
        VolumeMode::Maximum => grana_i18n::inspector::maximum_projection(language),
        VolumeMode::Isosurface => grana_i18n::inspector::isosurface(language),
    }
}

pub(super) fn transfer_colors(transfer: &TransferFunction) -> Vec<Color> {
    transfer
        .stops()
        .iter()
        .map(|stop| {
            let [red, green, blue] = stop.color();
            Color::rgba(red, green, blue, 1.0)
        })
        .collect()
}
