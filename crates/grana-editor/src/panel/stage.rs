use glam::Vec2;
use grana_figure::{self as overlay, Handle, Label, Legend, Viewport};
use grana_ui::{
    BuildContext, Color, Corners, Edges, Element, Icon, IconStyle, Key, Layer, Length, Level, Rect,
    Role, State, TextStyle, icon,
};

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
    let frame = palette.surface_container_lowest;
    let corners = Corners::all(theme.metrics.corner_xl);
    let stage = state.stage;
    let region = stage.region();
    let quad = stage.quad();
    let pane = layout.viewport;
    let content = grana_ui::canvas(Length::Fill, full.height())
        .view(region, move |_, painter, info| {
            painter.rect(info.rect, Corners::ZERO, backdrop);
            painter.rect(pane, corners.resolved(pane.size()), frame);
            painter.scene(quad, corners.resolved(quad.size()), [0.0, 0.0, 1.0, 1.0]);
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
    let origin = layout.viewport.shrink(STAGE_INSET).min;
    let tint = ctx.theme().palette.tertiary;
    let language = ctx.language();
    vec![
        chip(
            ctx,
            "figure.readonly",
            layout.viewport,
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
    rows.push(
        grana_ui::row().gap(6.0).children([
            crate::view::button_with(
                ctx,
                grana_i18n::common::clear(language),
                Role::SurfaceHigh,
                Length::Fill,
            )
            .named("mask.clear")
            .press(Message::Command(grana_session::Command::ClearMask(
                brush.set,
            ))),
            crate::view::button_with(
                ctx,
                grana_i18n::common::done(language),
                Role::Filled,
                Length::Fill,
            )
            .named("mask.done")
            .press(Message::DisarmMask),
        ]),
    );
    let width = 264.0;
    let height = 152.0;
    let safe = layout.viewport.shrink(STAGE_INSET);
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
    if !open {
        return Vec::new();
    }
    let palette = ctx.theme().palette;
    let language = ctx.language();
    let width = (stage.width() - 40.0).min(520.0);
    let title = crate::view::card_title(
        ctx,
        Icon::Globe,
        grana_i18n::annotation::open_remote(language),
        Some(grana_i18n::annotation::http_range(language)),
    );
    let tip = crate::view::caption(ctx, grana_i18n::annotation::remote_tip(language));
    let field = grana_ui::widget::text_field(
        "",
        TextStyle::new(ctx.theme().font(Level::Body), palette.on_surface),
        width - 36.0,
        false,
    )
    .named("remote.url")
    .editing(true)
    .on_submit(|text: &str| Message::OpenRemote(text.to_string()))
    .on_cancel(|| Message::CancelRemote);
    let hint = crate::view::caption(ctx, grana_i18n::annotation::remote_hint(language));
    let surface = ctx
        .theme()
        .surface(Role::Menu, State::Rest)
        .with_corners(Corners::all(ctx.theme().metrics.corner_xl));
    let sheet = grana_ui::column()
        .padding(Edges::all(18.0))
        .gap(10.0)
        .background(surface)
        .children([title, tip, field, hint]);
    let rect = Rect::from_min_size(
        Vec2::new(stage.center().x - width * 0.5, stage.min.y + 92.0),
        Vec2::new(width, 160.0),
    );
    vec![sheet.at(rect).layer(Layer::Popup)]
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
        safe: layout.viewport.shrink(STAGE_INSET),
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
            safe: edges(layout.viewport.shrink(STAGE_INSET)),
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
    let origin = layout.viewport.shrink(STAGE_INSET).min;
    let title = grana_i18n::annotation::recording(ctx.language(), frame, frames);
    vec![chip(
        ctx,
        "figure.recording",
        layout.viewport,
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
    let stage = layout.viewport.shrink(STAGE_INSET);
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
