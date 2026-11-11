use glam::Vec2;
use grana_figure::{self as overlay, Handle, Label, Legend, Viewport};
use grana_ui::glass::GlassStyle;
use grana_ui::{
    BuildContext, Color, Corners, Edges, Element, Icon, Key, Layer, Length, Level, Rect, TextStyle,
    TypeStyle, icon,
};

use crate::message::Message;
use crate::panel::{Layout, STAGE_INSET};
use crate::state::View;

const REGION: Color = Color::rgba(0.98, 0.75, 0.29, 0.40);
const REGION_ACTIVE: Color = Color::rgba(0.98, 0.83, 0.45, 0.95);
const RECORD: Color = Color::rgba(1.0, 0.52, 0.44, 1.0);

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
    full: Rect,
) -> Vec<Element<'a, Message>> {
    let mut elements = Vec::new();
    let palette = ctx.theme().palette;
    let backdrop = palette.scene_backdrop;
    let content = grana_ui::canvas(Length::Fill, full.height(), move |painter, info| {
        painter.rect(info.rect, Corners::ZERO, backdrop);
        painter.scene(info.rect, Corners::ZERO, [0.0, 0.0, 1.0, 1.0]);
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
    elements.extend(mask_bar(ctx, state, layout));
    elements
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
    let tools = crate::view::segmented(
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
            crate::view::button(
                ctx,
                grana_i18n::common::clear(language),
                grana_ui::Role::Control,
            )
            .named("mask.clear")
            .press(Message::Command(grana_session::Command::ClearMask(
                brush.set,
            ))),
            crate::view::button(
                ctx,
                grana_i18n::common::done(language),
                grana_ui::Role::Tinted,
            )
            .named("mask.done")
            .press(Message::DisarmMask),
        ]),
    );
    let width = 240.0;
    let height = 34.0 + 30.0 + 34.0 + 6.0 + 30.0 + 12.0;
    let safe = layout.content().shrink(STAGE_INSET);
    let rect = Rect::from_min_size(
        Vec2::new(safe.min.x, (safe.max.y - height).max(safe.min.y)),
        Vec2::new(width, height),
    );
    vec![
        grana_ui::column()
            .padding(Edges::all(12.0))
            .gap(6.0)
            .glass(GlassStyle::sheet())
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
        TextStyle::new(ctx.theme().font(Level::Body), palette.text_primary),
        width - 36.0,
        false,
    )
    .named("remote.url")
    .editing(true)
    .on_commit(|text: &str| Message::OpenRemote(text.to_string()))
    .on_cancel(|| Message::CancelRemote);
    let hint = crate::view::caption(ctx, grana_i18n::annotation::remote_hint(language));
    let sheet = grana_ui::column()
        .padding(Edges::all(18.0))
        .gap(8.0)
        .glass(GlassStyle::sheet())
        .children([title, tip, field, hint]);
    let rect = Rect::from_min_size(
        Vec2::new(stage.center().x - width * 0.5, stage.min.y + 96.0),
        Vec2::new(width, 158.0),
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
        safe: layout.content().shrink(STAGE_INSET),
        region: &state.scene.region,
        clip: state.scene.clip,
        legends,
        labels,
        annotations,
        pending: state.workspace.annotation_pending,
    }
}

fn overlay_view<'a>(state: &'a View<'a>, layout: &Layout) -> Element<'a, Message> {
    let mut legends = overlay::legends(state.scene);
    let language = state.workspace.language;
    for legend in &mut legends {
        legend.title = crate::text::legend(language, &legend.title);
    }
    let sliced = state.camera.slice().is_some();
    let labels = if sliced {
        Vec::new()
    } else {
        overlay::labels(state.scene, state.scene.time())
    };
    let annotations = if sliced {
        Vec::new()
    } else {
        overlay::annotations(state.scene, state.scene.time())
    };
    let layout = *layout;
    grana_ui::canvas(
        Length::Fill,
        layout.viewport.height(),
        move |painter, _info| {
            let viewport = viewport(state, &layout, &legends, &labels, &annotations);
            overlay::paint(painter, &viewport);
        },
    )
    .named("stage.overlay")
    .at(layout.viewport)
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
    grana_ui::canvas(
        Length::Fixed(rect.width()),
        rect.height(),
        move |painter, info| {
            let active = info.hover.max(info.press);
            painter.rect(
                info.rect,
                Corners::all(2.5),
                REGION.lerp(REGION_ACTIVE, active),
            );
        },
    )
    .key(Key::new(format!("region.handle.{}.{side}", axis)))
    .at(rect)
    .layer(Layer::Content)
    .on_pointer(move |pointer| {
        (pointer.phase == grana_ui::Phase::Move).then_some(Message::Region {
            axis,
            side,
            delta: pointer.delta.dot(direction) * world_per_point,
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
    let origin = layout.content().shrink(STAGE_INSET).min;
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
    let style = TextStyle::new(TypeStyle::new(11.5, 500.0, 15.0), tint);
    let maximum = (pane.width() - 64.0).max(96.0);
    let layout = ctx.layout_text(title, style, maximum);
    let width = (layout.size.x + 42.0).min(maximum + 24.0);
    grana_ui::canvas(Length::Fixed(width), 26.0, move |painter, info| {
        let rect = info.rect;
        icon::paint(
            symbol,
            painter,
            Vec2::new(rect.min.x + 15.0, rect.center().y),
            13.0,
            tint,
            1.5,
        );
        painter.text(
            &layout,
            Vec2::new(rect.min.x + 25.0, rect.min.y + 5.5),
            tint,
        );
    })
    .named(key)
    .at(Rect::from_min_size(origin, Vec2::new(width, 26.0)))
    .glass(GlassStyle::hud_chip())
}

pub(super) fn toast<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let Some(message) = state.io.message else {
        return Vec::new();
    };
    let palette = ctx.theme().palette;
    let stage = layout.content().shrink(STAGE_INSET);
    let ceiling = stage.max.y;
    let width = (stage.width() - 32.0).min(420.0);
    let style = TextStyle::new(
        ctx.theme().font(grana_ui::Level::Callout),
        palette.text_primary,
    )
    .wrapping();
    let layout_text = ctx.layout_text(message.text(), style, width - 56.0);
    let height = (layout_text.size.y + 22.0).max(42.0);
    let rect = Rect::from_min_size(
        Vec2::new(stage.center().x - width * 0.5, ceiling - height),
        Vec2::new(width, height),
    );
    let (mark, tint) = match message.kind() {
        grana_session::Kind::Error => (Icon::Warning, palette.warning),
        grana_session::Kind::Info => (Icon::Info, palette.text_secondary),
    };
    let appear = ctx.animate(
        Key::new(message.text()).id(),
        1.0,
        grana_ui::Motion::response(0.25, 0.0),
    );
    vec![
        grana_ui::canvas(Length::Fill, height, move |painter, info| {
            let rect = info.rect;
            icon::paint(
                mark,
                painter,
                Vec2::new(rect.min.x + 20.0, rect.center().y),
                15.0,
                tint.with_alpha(tint.alpha * appear),
                1.6,
            );
            painter.text(
                &layout_text,
                Vec2::new(rect.min.x + 38.0, rect.min.y + 11.0),
                palette
                    .text_primary
                    .with_alpha(palette.text_primary.alpha * appear),
            );
        })
        .named("stage.toast")
        .at(rect)
        .glass(GlassStyle::toast())
        .layer(Layer::Hud),
    ]
}
