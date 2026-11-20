use glam::Vec2;
use grana_figure::{self as overlay, Handle, Label, Legend, Viewport};
use grana_ui::{
    Align, BuildContext, Color, Corners, Edges, Element, Fill, Icon, IconStyle, Key, Layer, Length,
    Level, Motion, Rect, Role, State, TextStyle, icon,
};
use grana_volume::Calibration;

use crate::message::Message;
use crate::panel::{Layout, STAGE_INSET};
use crate::state::View;

const REGION: Color = Color::rgba(0.98, 0.75, 0.29, 0.45);
const REGION_ACTIVE: Color = Color::rgba(0.98, 0.85, 0.48, 0.98);
const RECORD: Color = Color::rgba(1.0, 0.54, 0.48, 1.0);

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
    full: Rect,
) -> Vec<Element<'a, Message>> {
    let mut elements = Vec::new();
    let theme = *ctx.theme();
    let palette = theme.palette;
    let backdrop = palette.surface_dim;
    let stage = state.stage;
    let region = stage.region();
    let quad = stage.quad();
    let content = grana_ui::canvas(Length::Fill, full.height())
        .view(region, move |_, painter, info| {
            painter.rect(info.rect, Corners::ZERO, backdrop);
            painter.scene(quad, Corners::ZERO, [0.0, 0.0, 1.0, 1.0]);
        })
        .named("stage.content")
        .at(full)
        .layer(Layer::Content);
    elements.push(content);
    elements.push(overlay_view(state, layout));
    if state.camera.slice().is_none() {
        elements.extend(region_handles(state, layout));
    }
    elements.extend(recording_chip(ctx, layout, state));
    elements.extend(viewer_chip(ctx, layout, state));
    elements.extend(mask_bar(ctx, state, layout));
    elements
}

fn viewer_chip<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    if state.access.author() || state.io.exporting.is_some() {
        return Vec::new();
    }
    let origin = layout.safe.shrink(STAGE_INSET).min;
    let tint = ctx.theme().palette.tertiary;
    let language = ctx.language();
    vec![
        chip(
            ctx,
            "figure.readonly",
            layout.safe,
            origin,
            Icon::Lock,
            grana_i18n::app::read_only(language),
            tint,
        )
        .tooltip(grana_i18n::app::read_only_detail(language)),
    ]
}

fn mask_bar<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    layout: &Layout,
) -> Vec<Element<'a, Message>> {
    let Some(brush) = state.workspace.mask else {
        return Vec::new();
    };
    let Some(set) = state.scene.objects.get(brush.set) else {
        return Vec::new();
    };
    let language = ctx.language();
    let names = crate::state::MaskBrushTool::ALL
        .map(|tool| crate::state::MaskBrushTool::name(tool, language));
    let tools = crate::view::segmented_fill(
        ctx,
        "mask.tool",
        &names,
        brush.tool.index(),
        Message::SetMaskTool,
    );
    let title = crate::view::card_title(
        ctx,
        Icon::Pencil,
        grana_i18n::annotation::paint_mask(language),
        Some(set.name()),
    );
    let mut rows = vec![title, tools];
    if brush.tool.wand() {
        rows.push(crate::view::slider_event(
            ctx,
            "mask.tolerance",
            Some(grana_i18n::annotation::tolerance(language)),
            brush.tolerance,
            0.0..=0.5,
            &format!("{:.02}", brush.tolerance),
            Message::SetMaskTolerance,
        ));
    } else {
        rows.push(crate::view::slider_event(
            ctx,
            "mask.radius",
            Some(grana_i18n::annotation::radius(language)),
            brush.radius,
            0.1..=20.0,
            &format!("{:.2} µm", brush.radius),
            Message::SetMaskRadius,
        ));
    }
    let inner = ctx.theme().metrics.corner_xs;
    let gap = ctx.theme().metrics.group_gap;
    rows.push(
        grana_ui::row().gap(gap).children([
            crate::view::button_shaped(
                ctx,
                grana_i18n::common::clear(language),
                Role::SurfaceHigh,
                Length::Fill,
                Corners::split_leading(inner),
            )
            .named("mask.clear")
            .press(Message::Command(grana_session::Command::ClearMask(
                brush.set,
            ))),
            crate::view::button_shaped(
                ctx,
                grana_i18n::common::done(language),
                Role::Filled,
                Length::Fill,
                Corners::split_trailing(inner),
            )
            .named("mask.done")
            .press(Message::DisarmMask),
        ]),
    );
    let width = 264.0;
    let height = 152.0;
    let safe = layout.safe.shrink(STAGE_INSET);
    let rect = Rect::from_min_size(
        Vec2::new(safe.min.x, (safe.max.y - height).max(safe.min.y)),
        Vec2::new(width, height),
    );
    let surface = ctx
        .theme()
        .surface(Role::Menu, State::Rest)
        .with_corners(Corners::all(ctx.theme().metrics.corner_xl));
    vec![
        grana_ui::column()
            .padding(Edges::all(12.0))
            .gap(8.0)
            .background(surface)
            .children(rows)
            .at(rect)
            .layer(Layer::Popup),
    ]
}

pub(crate) fn remote<'a>(
    ctx: &mut BuildContext<'_, Message>,
    stage: Rect,
    open: bool,
) -> Vec<Element<'a, Message>> {
    let target = if open { 1.0 } else { 0.0 };
    let spatial = ctx.animate(
        Key::new("remote.spatial").id(),
        target,
        Motion::spatial_default(),
    );
    let effect = ctx.animate(
        Key::new("remote.effect").id(),
        target,
        Motion::effect_default(),
    );
    if !open && effect <= 0.005 && spatial <= 0.005 {
        return Vec::new();
    }
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let alpha = effect.clamp(0.0, 1.0);
    let scale = (0.90 + 0.10 * spatial).max(0.5);
    let slide = (1.0 - spatial) * -14.0;
    let width = (stage.width() - 32.0).clamp(260.0, 400.0);
    let height = 104.0;
    let rect = Rect::from_center_size(
        stage.center() + Vec2::new(0.0, slide),
        Vec2::new(width * scale, height * scale),
    );
    let scrim = palette.scrim.with_alpha(palette.scrim.alpha * alpha);
    let backdrop = grana_ui::canvas(Length::Fill, stage.height())
        .view(alpha.to_bits(), move |_, painter, info| {
            painter.rect(info.rect, Corners::ZERO, scrim);
        })
        .named("remote.scrim")
        .at(stage)
        .layer(Layer::Popup);
    let backdrop = if open {
        backdrop.press(Message::CancelRemote)
    } else {
        backdrop
    };
    let head = grana_ui::row()
        .width(Length::Fill)
        .align(Align::Center)
        .children([
            crate::view::card_title(
                ctx,
                Icon::Globe,
                grana_i18n::annotation::open_remote(language),
                None,
            )
            .width(Length::Fill),
            crate::view::icon_button(
                ctx,
                "remote.close",
                Icon::Close,
                28.0,
                Role::SurfaceHigh,
                grana_i18n::common::cancel(language),
                Message::CancelRemote,
            ),
        ]);
    let field = grana_ui::widget::text_field(
        "",
        TextStyle::new(theme.font(Level::Body), palette.on_surface),
        (rect.width() - 32.0).max(160.0),
        false,
    )
    .named("remote.url")
    .editing(open)
    .on_submit(|text: &str| Message::OpenRemote(text.to_string()))
    .on_cancel(|| Message::CancelRemote);
    let mut surface = theme.surface(Role::Sheet, State::Rest);
    surface.fill = Fill::Solid(palette.surface_container_low.with_alpha(alpha));
    surface.corners = Corners::all(theme.metrics.corner_xxl);
    let sheet = grana_ui::column()
        .padding(Edges::all(16.0))
        .gap(10.0)
        .background(surface)
        .clipped()
        .children([head, field])
        .at(rect)
        .layer(Layer::Popup);
    vec![backdrop, sheet]
}

pub(crate) fn calibration<'a>(
    ctx: &mut BuildContext<'_, Message>,
    stage: Rect,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    const AXES: [&str; 3] = ["X", "Y", "Z"];
    let Some(sheet) = state.workspace.calibration.as_ref() else {
        return Vec::new();
    };
    let target = if sheet.open { 1.0 } else { 0.0 };
    let spatial = ctx.animate(
        Key::new("calibration.spatial").id(),
        target,
        Motion::spatial_default(),
    );
    let effect = ctx.animate(
        Key::new("calibration.effect").id(),
        target,
        Motion::effect_default(),
    );
    if !sheet.open && effect <= 0.005 && spatial <= 0.005 {
        return Vec::new();
    }
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let authoring = state.access.author();
    let alpha = effect.clamp(0.0, 1.0);
    let scale = (0.90 + 0.10 * spatial).max(0.5);
    let slide = (1.0 - spatial) * -14.0;
    let width = (stage.width() - 64.0).clamp(320.0, 520.0);
    let height = 320.0;
    let rect = Rect::from_center_size(
        stage.center() + Vec2::new(0.0, slide),
        Vec2::new(width * scale, height * scale),
    );
    let scrim = palette.scrim.with_alpha(palette.scrim.alpha * alpha);
    let backdrop = grana_ui::canvas(Length::Fill, stage.height())
        .view(alpha.to_bits(), move |_, painter, info| {
            painter.rect(info.rect, Corners::ZERO, scrim);
        })
        .named("calibration.scrim")
        .at(stage)
        .layer(Layer::Popup);
    let backdrop = if sheet.open {
        backdrop.press(Message::DismissCalibration)
    } else {
        backdrop
    };
    let head = grana_ui::row()
        .width(Length::Fill)
        .align(Align::Center)
        .children([
            crate::view::card_title(
                ctx,
                Icon::Ruler,
                grana_i18n::inspector::voxel_size(language),
                None,
            )
            .width(Length::Fill),
            crate::view::icon_button(
                ctx,
                "calibration.close",
                Icon::Close,
                28.0,
                Role::SurfaceHigh,
                grana_i18n::common::close(language),
                Message::DismissCalibration,
            ),
        ]);
    let detail = format!(
        "{} · {}×{}×{}",
        sheet.name, sheet.extent[0], sheet.extent[1], sheet.extent[2]
    );
    let reason = match sheet.reason {
        Calibration::Uncalibrated => grana_i18n::inspector::calibration_uncalibrated(language),
        Calibration::Assumed => grana_i18n::inspector::calibration_assumed(language),
        Calibration::Inferred => grana_i18n::inspector::calibration_inferred(language),
        Calibration::Recorded => grana_i18n::inspector::calibration_recorded(language),
    };
    let style = TextStyle::new(theme.font(Level::Body), palette.on_surface);
    let field_width = (width - 32.0 - 16.0) / 3.0;
    let mut fields = Vec::with_capacity(3);
    for (axis, text) in sheet.fields.iter().enumerate() {
        let name = format!("calibration.{}", AXES[axis].to_ascii_lowercase());
        let field = grana_ui::widget::text_field(text, style, field_width, false)
            .key(Key::new(&name))
            .select_all()
            .editing(authoring)
            .on_commit(move |text: &str| Message::SetVoxelField {
                axis,
                text: text.to_string(),
            });
        fields.push(
            grana_ui::column()
                .width(Length::Fixed(field_width))
                .gap(4.0)
                .children([crate::view::caption(ctx, AXES[axis]), field]),
        );
    }
    let mut rows = vec![
        head,
        crate::view::caption(ctx, &detail),
        crate::view::weak(ctx, reason),
        grana_ui::row().gap(8.0).children(fields),
        crate::view::caption(ctx, grana_i18n::inspector::voxel_size_unit(language)),
    ];
    if authoring {
        rows.push(crate::view::caption(
            ctx,
            if sheet.error {
                grana_i18n::inspector::voxel_size_invalid(language)
            } else {
                grana_i18n::inspector::voxel_size_hint(language)
            },
        ));
    }
    rows.push(
        grana_ui::row().gap(8.0).children([
            crate::view::button_with(
                ctx,
                grana_i18n::common::cancel(language),
                Role::SurfaceHigh,
                Length::Fill,
            )
            .named("calibration.dismiss")
            .press(Message::DismissCalibration),
            crate::view::button_with(
                ctx,
                grana_i18n::common::apply(language),
                Role::Filled,
                Length::Fill,
            )
            .named("calibration.apply")
            .press(Message::ApplyCalibration),
        ]),
    );
    let mut surface = theme.surface(Role::Sheet, State::Rest);
    surface.fill = Fill::Solid(palette.surface_container_low.with_alpha(alpha));
    surface.corners = Corners::all(theme.metrics.corner_xxl);
    let sheet_element = grana_ui::column()
        .padding(Edges::all(16.0))
        .gap(10.0)
        .background(surface)
        .clipped()
        .children(rows)
        .at(rect)
        .layer(Layer::Popup);
    vec![backdrop, sheet_element]
}

fn viewport<'a>(
    state: &'a View<'a>,
    layout: &Layout,
    legends: &'a [Legend],
    labels: &'a [Label],
    annotations: &'a [grana_figure::AnnotationItem],
) -> Viewport<'a> {
    let rect = layout.viewport;
    Viewport {
        camera: state.camera,
        world: state.scene.dataset.world_size(),
        rect,
        safe: layout.safe.shrink(STAGE_INSET),
        region: &state.scene.region,
        clip: state.scene.clip,
        legends,
        labels,
        annotations,
        pending: state.workspace.annotation_pending,
    }
}

#[derive(Clone, Copy, Hash)]
struct OverlayKey {
    revision: grana_scene::Revision,
    labels: u64,
    camera: u64,
    viewport: [u32; 4],
    safe: [u32; 4],
    world: [u32; 3],
    pending: [u32; 4],
    language: grana_i18n::Language,
}

impl OverlayKey {
    fn new(state: &View<'_>, layout: &Layout) -> Self {
        Self {
            revision: state.revision,
            labels: state.workspace.overlay.stamp(),
            camera: camera_digest(state.camera),
            viewport: edges(layout.viewport),
            safe: edges(layout.safe.shrink(STAGE_INSET)),
            world: state
                .scene
                .dataset
                .world_size()
                .to_array()
                .map(f32::to_bits),
            pending: state.workspace.annotation_pending.map_or([0; 4], |at| {
                [at[0].to_bits(), at[1].to_bits(), at[2].to_bits(), 1]
            }),
            language: state.workspace.language,
        }
    }
}

fn camera_digest(camera: &grana_display::Camera) -> u64 {
    use std::hash::{Hash, Hasher};
    let state = camera.state();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    state.pivot.map(f32::to_bits).hash(&mut hasher);
    state.yaw.to_bits().hash(&mut hasher);
    state.pitch.to_bits().hash(&mut hasher);
    state.distance.to_bits().hash(&mut hasher);
    state.fov_y.to_bits().hash(&mut hasher);
    state.projection.index().hash(&mut hasher);
    state.aperture.to_bits().hash(&mut hasher);
    state.focus.to_bits().hash(&mut hasher);
    if let Some(slice) = state.slice {
        slice.axis().hash(&mut hasher);
        slice.right().to_array().map(f32::to_bits).hash(&mut hasher);
        slice.up().to_array().map(f32::to_bits).hash(&mut hasher);
    }
    hasher.finish()
}

fn edges(rect: Rect) -> [u32; 4] {
    [
        rect.min.x.to_bits(),
        rect.min.y.to_bits(),
        rect.max.x.to_bits(),
        rect.max.y.to_bits(),
    ]
}

fn overlay_view<'a>(state: &'a View<'a>, layout: &Layout) -> Element<'a, Message> {
    let key = OverlayKey::new(state, layout);
    let slot = *layout;
    grana_ui::canvas(Length::Fill, slot.viewport.height())
        .view(key, move |_, painter, _info| {
            let mut legends = overlay::legends(state.scene);
            let language = state.workspace.language;
            for legend in &mut legends {
                legend.title = crate::text::legend(language, &legend.title);
            }
            let sliced = state.camera.slice().is_some();
            let labels = if sliced {
                &[][..]
            } else {
                state.workspace.overlay.labels()
            };
            let annotations = if sliced {
                Vec::new()
            } else {
                overlay::annotations(state.scene, state.scene.time())
            };
            let viewport = viewport(state, &slot, &legends, labels, &annotations);
            overlay::paint(painter, &viewport);
        })
        .named("stage.overlay")
        .at(slot.viewport)
        .layer(Layer::Content)
}

fn region_handles<'a>(state: &'a View<'a>, layout: &Layout) -> Vec<Element<'a, Message>> {
    let labels: [Label; 0] = [];
    let annotations: [grana_figure::AnnotationItem; 0] = [];
    let viewport = viewport(state, layout, &[], &labels, &annotations);
    overlay::handles(&viewport)
        .into_iter()
        .map(region_handle)
        .collect()
}

fn region_handle<'a>(handle: Handle) -> Element<'a, Message> {
    let Handle {
        axis,
        side,
        rect,
        screen,
        world_per_point,
    } = handle;
    let direction = screen / screen.length();
    grana_ui::canvas(Length::Fixed(rect.width()), rect.height())
        .view_static(move |painter, info| {
            let active = info.hover.max(info.press);
            painter.rect(
                info.rect,
                Corners::capsule(),
                REGION.lerp(REGION_ACTIVE, active),
            );
        })
        .key(Key::new(format!("region.handle.{}.{side}", axis)))
        .at(rect)
        .layer(Layer::Content)
        .on_drag_vector(move |delta| {
            Some(Message::Region {
                axis,
                side,
                delta: delta.dot(direction) * world_per_point,
            })
        })
}

fn recording_chip<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let Some((frame, frames)) = state.io.exporting else {
        return Vec::new();
    };
    let origin = layout.safe.shrink(STAGE_INSET).min;
    let title = grana_i18n::annotation::recording(ctx.language(), frame, frames);
    vec![chip(
        ctx,
        "figure.recording",
        layout.safe,
        origin,
        Icon::Record,
        &title,
        RECORD,
    )]
}

fn chip<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    pane: Rect,
    origin: Vec2,
    symbol: Icon,
    title: &str,
    tint: Color,
) -> Element<'a, Message> {
    let style = TextStyle::new(ctx.theme().font(Level::Label), tint);
    let maximum = (pane.width() - 64.0).max(96.0);
    let layout = ctx.layout_text(title, style, maximum);
    let width = (layout.size.x + 40.0).min(maximum + 24.0);
    let surface = ctx.theme().surface(Role::FloatingToolbar, State::Rest);
    grana_ui::canvas(Length::Fixed(width), 28.0)
        .view(
            (symbol, tint, title.to_string()),
            move |_, painter, info| {
                let rect = info.rect;
                icon::paint(
                    symbol,
                    painter,
                    Vec2::new(rect.min.x + 14.0, rect.center().y),
                    14.0,
                    tint,
                    IconStyle::FILLED,
                );
                painter.text(
                    &layout,
                    Vec2::new(rect.min.x + 25.0, rect.min.y + 6.0),
                    tint,
                );
            },
        )
        .named(key)
        .at(Rect::from_min_size(origin, Vec2::new(width, 28.0)))
        .background(surface)
}

pub(super) fn toast<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let (text, kind) = match (state.io.failure, state.io.message) {
        (Some(failure), _) => (failure.to_string(), grana_session::Kind::Error),
        (None, Some(message)) => (message.text().to_string(), message.kind()),
        (None, None) => return Vec::new(),
    };
    let palette = ctx.theme().palette;
    let stage = layout.safe.shrink(STAGE_INSET);
    let ceiling = stage.max.y;
    let width = (stage.width() - 32.0).min(420.0);
    let style = TextStyle::new(ctx.theme().font(Level::Body), palette.on_surface).wrapping();
    let layout_text = ctx.layout_text(&text, style, width - 56.0);
    let height = (layout_text.size.y + 20.0).max(38.0);
    let rect = Rect::from_min_size(
        Vec2::new(stage.center().x - width * 0.5, ceiling - height),
        Vec2::new(width, height),
    );
    let (mark, tint) = match kind {
        grana_session::Kind::Error => (Icon::Warning, palette.error),
        grana_session::Kind::Info => (Icon::Info, palette.tertiary),
    };
    let appear = ctx.animate(
        Key::new(&text).id(),
        1.0,
        grana_ui::Motion::effect_default(),
    );
    let surface = ctx.theme().surface(Role::FloatingToolbar, State::Rest);
    let key = text.clone();
    vec![
        grana_ui::canvas(Length::Fill, height)
            .view(
                (mark, tint, appear.to_bits(), key),
                move |_, painter, info| {
                    let rect = info.rect;
                    icon::paint(
                        mark,
                        painter,
                        Vec2::new(rect.min.x + 18.0, rect.center().y),
                        16.0,
                        tint.with_alpha(tint.alpha * appear),
                        IconStyle::REGULAR,
                    );
                    painter.text(
                        &layout_text,
                        Vec2::new(rect.min.x + 34.0, rect.min.y + 10.0),
                        palette
                            .on_surface
                            .with_alpha(palette.on_surface.alpha * appear),
                    );
                },
            )
            .named("stage.toast")
            .at(rect)
            .background(surface)
            .layer(Layer::Hud),
    ]
}
