use super::*;

pub(super) fn slice<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
    count: usize,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let language = ctx.language();
    let key = Key::new("slice");
    let open = state.workspace.disclosures.is_open(key);
    let active = crate::fitting::slice_position(state.scene, state.camera());
    let detail = active.map(|(axis, slice_idx, total, _)| {
        format!("{} · {} / {}", axis_name(axis), slice_idx + 1, total)
    });
    let mut children = Vec::new();
    if open {
        children.push(
            grana_ui::widget::toggle_row(
                &theme,
                grana_i18n::inspector::slice_view(language),
                active.is_some(),
            )
            .named("slice.enabled")
            .on_toggle(Message::ToggleSlice),
        );
        if let Some((axis, _, _, at)) = active {
            children.push(view::segmented_fill(
                ctx,
                "slice.axis",
                &["X", "Y", "Z"],
                axis,
                Message::SetSliceAxis,
            ));
            let readout = detail.clone().unwrap_or_default();
            children.push(view::slider_event(
                ctx,
                "slice.depth",
                Some(grana_i18n::inspector::depth(language)),
                at,
                0.0..=1.0,
                &readout,
                Message::SetSliceDepth,
            ));
            children.push(view::caption(
                ctx,
                grana_i18n::inspector::slice_help(language),
            ));
        }
    }
    view::disclosure_group_card(
        ctx,
        (index, count),
        key,
        (
            Icon::Section,
            grana_i18n::inspector::slice_view(language),
            Some(
                detail
                    .as_deref()
                    .unwrap_or(grana_i18n::inspector::inactive(language)),
            ),
        ),
        children,
        open,
    )
}

pub(super) fn axis_name(axis: usize) -> &'static str {
    match axis {
        0 => "X",
        1 => "Y",
        _ => "Z",
    }
}

pub(super) fn clip<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
    count: usize,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let language = ctx.language();
    let current = state.scene.clip;
    let status = if current.enabled() {
        format!("{:+.1} µm", current.offset())
    } else {
        grana_i18n::inspector::inactive(language).to_string()
    };
    let key = Key::new("clip");
    let open = state.workspace.disclosures.is_open(key);
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
        let group_gap = theme.metrics.group_gap;
        let inner_sm = theme.metrics.corner_sm;
        let apply = move |azimuth: f32, elevation: f32, offset: f32, enabled: bool| {
            Message::Command(Command::SetClip(if enabled {
                Clip::new(direction(azimuth, elevation), offset.clamp(-reach, reach))
            } else {
                Clip::none()
            }))
        };
        children.push(
            grana_ui::widget::toggle_row(
                &theme,
                grana_i18n::inspector::cut_volume(language),
                enabled,
            )
            .named("clip.enabled")
            .on_toggle(move |value| apply(azimuth, elevation, offset, value)),
        );
        let azimuth_text = format!("{azimuth:.0}°");
        children.push(
            grana_ui::widget::slider_readout(
                &theme,
                Some(grana_i18n::inspector::azimuth(language)),
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
                Some(grana_i18n::inspector::elevation(language)),
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
                Some(grana_i18n::inspector::offset(language)),
                offset,
                -reach..=reach,
                false,
                &offset_text,
            )
            .named("clip.offset")
            .on_slide(move |value| apply(azimuth, elevation, value, true)),
        );
        let half = (width - group_gap) * 0.5;
        let flip = view::button_shaped(
            ctx,
            grana_i18n::inspector::flip_plane(language),
            Role::SurfaceHigh,
            Length::Fixed(half),
            Corners::split_leading(inner_sm),
        )
        .press(apply(azimuth + 180.0, -elevation, -offset, true));
        let reset = view::button_shaped(
            ctx,
            grana_i18n::common::reset(language),
            Role::SurfaceHigh,
            Length::Fixed(half),
            Corners::split_trailing(inner_sm),
        )
        .press(Message::Command(Command::SetClip(Clip::none())));
        children.push(
            grana_ui::row()
                .gap(group_gap)
                .align(Align::Center)
                .children([flip, reset]),
        );
    }
    view::disclosure_group_card(
        ctx,
        (index, count),
        key,
        (
            Icon::Cube,
            grana_i18n::inspector::clipping_plane(language),
            Some(&status),
        ),
        children,
        open,
    )
}

pub(super) fn direction(azimuth: f32, elevation: f32) -> Vec3 {
    let (sine_azimuth, cosine_azimuth) = azimuth.to_radians().sin_cos();
    let (sine_elevation, cosine_elevation) = elevation.to_radians().sin_cos();
    Vec3::new(
        cosine_elevation * cosine_azimuth,
        cosine_elevation * sine_azimuth,
        sine_elevation,
    )
}

pub(super) fn rendering<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
    count: usize,
) -> Element<'a, Message> {
    let scene = state.scene;
    let language = ctx.language();
    let shading = scene.shading;
    let key = Key::new("rendering");
    let open = state.workspace.disclosures.is_open(key);
    let mut children = Vec::new();
    if open {
        let exposure = format!("{:.2}×", scene.exposure);
        children.push(view::slider_readout(
            ctx,
            "exposure",
            Some(grana_i18n::inspector::exposure(language)),
            scene.exposure,
            (0.05..=20.0, true),
            &exposure,
            Command::SetExposure,
        ));
        let shadows = format!("{:.2}", shading.shadows);
        children.push(view::slider_readout(
            ctx,
            "shadows",
            Some(grana_i18n::inspector::shadows(language)),
            shading.shadows,
            (0.0..=1.0, false),
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
        let lighting_open = state.workspace.disclosures.is_open(lighting);
        let mut light = Vec::new();
        if lighting_open {
            let azimuth = format!("{:.0}°", shading.azimuth);
            light.push(view::slider_readout(
                ctx,
                "lighting.azimuth",
                Some(grana_i18n::inspector::azimuth(language)),
                shading.azimuth,
                (
                    -grana_display::AZIMUTH_LIMIT..=grana_display::AZIMUTH_LIMIT,
                    false,
                ),
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
                Some(grana_i18n::inspector::elevation(language)),
                shading.elevation,
                (
                    -grana_display::ELEVATION_LIMIT..=grana_display::ELEVATION_LIMIT,
                    false,
                ),
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
                Some(grana_i18n::inspector::ambient(language)),
                shading.ambient,
                (0.0..=1.0, false),
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
                Some(grana_i18n::inspector::diffuse(language)),
                shading.diffuse,
                (0.0..=1.0, false),
                &diffuse,
                move |value| {
                    Command::SetShading(grana_display::Shading {
                        diffuse: value,
                        ..shading
                    })
                },
            ));
        }
        children.push(disclosure(
            ctx,
            "rendering.lighting",
            grana_i18n::inspector::surface_lighting(language),
            lighting_open,
            light,
        ));
        children.push(
            view::button_with(
                ctx,
                grana_i18n::inspector::reset_rendering(language),
                Role::SurfaceHigh,
                Length::Fill,
            )
            .press(Message::Command(Command::ResetRendering)),
        );
    }
    let summary = format!("{:.2}×", scene.exposure);
    view::disclosure_group_card(
        ctx,
        (index, count),
        key,
        (
            Icon::Sparkles,
            grana_i18n::inspector::rendering(language),
            Some(&summary),
        ),
        children,
        open,
    )
}
