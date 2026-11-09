use glam::{Vec2, Vec3};
use grana_figure::{self as overlay, Handle, Label, Legend, Overlay};
use grana_ui::glass::GlassStyle;
use grana_ui::{
    BuildContext, Color, Corners, Cursor, Edges, Element, Icon, Key, Layer, Length, Level, Phase,
    Rect, TextStyle, TypeStyle, icon,
};

use crate::message::Message;
use crate::panel::{Layout, STAGE_INSET};
use crate::state::View;
use crate::view::scene_uv;

const PANE_CONTROL: f32 = 26.0;
const LABEL: Color = Color::rgba(1.0, 1.0, 1.0, 0.82);
const REGION: Color = Color::rgba(0.98, 0.75, 0.29, 0.40);
const REGION_ACTIVE: Color = Color::rgba(0.98, 0.83, 0.45, 0.95);
const RECORD: Color = Color::rgba(1.0, 0.52, 0.44, 1.0);

#[derive(Clone, Copy)]
struct Geometry {
    volume_rect: Rect,
    section_rect: Rect,
    volume_safe: Rect,
    section_safe: Rect,
    world: Vec3,
    recording: Option<(u32, u32)>,
}

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
    full: Rect,
) -> Vec<Element<'a, Message>> {
    let mut elements = Vec::new();
    let palette = ctx.theme().palette;
    let size = full.size();
    let geometry = geometry(layout, state);
    let volume = layout.volume;
    let section = layout.section;
    let stage_backdrop = palette.scene_backdrop;
    let content = grana_ui::canvas(Length::Fill, full.height(), move |painter, info| {
        painter.rect(info.rect, Corners::ZERO, stage_backdrop);
        painter.scene(volume, Corners::ZERO, scene_uv(volume, size));
        painter.scene(section, Corners::ZERO, scene_uv(section, size));
    })
    .named("stage.content")
    .at(full)
    .layer(Layer::Content);
    elements.push(content);
    elements.push(overlay_view(state, geometry, full));
    elements.extend(region_handles(state, geometry));
    elements.push(pane_fit(
        ctx,
        geometry.volume_safe,
        "volume.fit",
        "Fit volume",
        Message::FitVolume,
    ));
    elements.push(pane_fit(
        ctx,
        geometry.section_safe,
        "section.fit",
        "Fit section",
        Message::FitSection,
    ));
    elements.extend(split_handle(ctx, layout));
    elements.push(chip(
        ctx,
        "figure.volume",
        geometry.volume_safe,
        geometry.volume_safe.min,
        Icon::Cube,
        "3D",
        LABEL,
    ));
    let section = state.viewport.section();
    let extent = geometry.world.abs().dot(section.normal().abs());
    let depth = section.center().dot(section.normal()) + 0.5 * extent;
    let mode = if section.projected() {
        format!(
            "{} · {:.2} µm",
            section.projection().label(),
            section.thickness()
        )
    } else {
        section.projection().label().to_string()
    };
    elements.push(chip(
        ctx,
        "figure.section",
        geometry.section_rect,
        geometry.section_safe.min,
        Icon::Section,
        &format!(
            "{mode} · E{:.0}° A{:.0}° · {depth:.2} µm",
            section.elevation().to_degrees(),
            section.azimuth().to_degrees()
        ),
        LABEL,
    ));
    if let Some((frame, frames)) = geometry.recording {
        elements.push(chip(
            ctx,
            "figure.recording",
            geometry.volume_rect,
            geometry.volume_safe.min + Vec2::new(0.0, 34.0),
            Icon::Record,
            &format!("REC · frame {frame} of {frames}"),
            RECORD,
        ));
    }
    elements.extend(mask_bar(ctx, state, geometry));
    elements
}

fn mask_bar<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    geometry: Geometry,
) -> Vec<Element<'a, Message>> {
    let Some(brush) = state.workspace.mask else {
        return Vec::new();
    };
    let Some(set) = state.scene.objects.get(brush.set) else {
        return Vec::new();
    };
    let names = crate::state::MaskBrushTool::ALL.map(crate::state::MaskBrushTool::name);
    let tools = crate::view::segmented(
        ctx,
        "mask.tool",
        &names,
        brush.tool.index(),
        Message::SetMaskTool,
    );
    let title = crate::view::card_title(ctx, Icon::Pencil, "Paint Mask", Some(set.name()));
    let mut rows = vec![title, tools];
    if brush.tool.wand() {
        rows.push(crate::view::slider_event(
            ctx,
            "mask.tolerance",
            Some("Tolerance"),
            brush.tolerance,
            0.0..=0.5,
            &format!("{:.02}", brush.tolerance),
            Message::SetMaskTolerance,
        ));
    } else {
        rows.push(crate::view::slider_event(
            ctx,
            "mask.radius",
            Some("Radius"),
            brush.radius,
            0.1..=20.0,
            &format!("{:.2} µm", brush.radius),
            Message::SetMaskRadius,
        ));
    }
    rows.push(
        grana_ui::row().gap(6.0).children([
            crate::view::button(ctx, "Clear", grana_ui::Role::Control)
                .named("mask.clear")
                .press(Message::Command(grana_session::Command::ClearMask(
                    brush.set,
                ))),
            crate::view::button(ctx, "Done", grana_ui::Role::Tinted)
                .named("mask.done")
                .press(Message::DisarmMask),
        ]),
    );
    let width = 240.0;
    let height = 34.0 + 30.0 + 34.0 + 6.0 + 30.0 + 12.0;
    let floor = geometry.section_safe.max.y;
    let rect = Rect::from_min_size(
        Vec2::new(
            geometry.section_safe.min.x,
            (floor - height).max(geometry.section_safe.min.y),
        ),
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
    let width = (stage.width() - 40.0).min(520.0);
    let title = crate::view::card_title(
        ctx,
        Icon::Globe,
        "Open Remote OME-Zarr",
        Some("HTTP range access"),
    );
    let tip = crate::view::caption(ctx, "Paste the https url of an OME-Zarr dataset or plate");
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
    let hint = crate::view::caption(ctx, "Return opens the dataset · Escape cancels");
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

fn geometry(layout: &Layout, state: &View<'_>) -> Geometry {
    Geometry {
        volume_rect: layout.volume,
        section_rect: layout.section,
        volume_safe: layout.volume.shrink(STAGE_INSET),
        section_safe: layout.section.shrink(STAGE_INSET),
        world: state.scene.dataset.world_size(),
        recording: state.io.export,
    }
}

fn overlay_of<'a>(
    state: &'a View<'a>,
    geometry: Geometry,
    legends: &'a [Legend],
    labels: &'a [Label],
    annotations: &'a [grana_figure::AnnotationItem],
) -> Overlay<'a> {
    Overlay {
        camera: state.viewport.camera(),
        section: state.viewport.section(),
        world: geometry.world,
        voxel: state.scene.dataset.voxel_size(),
        volume_safe: geometry.volume_safe,
        section_safe: geometry.section_safe,
        volume_rect: geometry.volume_rect,
        section_rect: geometry.section_rect,
        region: &state.scene.region,
        clip: state.scene.clip,
        legends,
        labels,
        annotations,
        pending: state.workspace.annotation_pending,
        recording: geometry.recording,
    }
}

fn overlay_view<'a>(state: &'a View<'a>, geometry: Geometry, full: Rect) -> Element<'a, Message> {
    let legends = overlay::legends(state.scene);
    let labels = overlay::labels(state.scene, state.scene.time());
    let annotations = overlay::annotations(state.scene, state.scene.time());
    grana_ui::canvas(Length::Fill, full.height(), move |painter, info| {
        let _ = info;
        let overlay = overlay_of(state, geometry, &legends, &labels, &annotations);
        overlay::paint(painter, &overlay);
    })
    .named("stage.overlay")
    .at(full)
    .layer(Layer::Content)
}

fn region_handles<'a>(state: &'a View<'a>, geometry: Geometry) -> Vec<Element<'a, Message>> {
    let overlay = overlay_of(state, geometry, &[], &[], &[]);
    overlay::handles(&overlay)
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
    .key(Key::index("region.handle", axis * 2 + side))
    .at(rect)
    .layer(Layer::Content)
    .on_pointer(move |pointer| {
        (pointer.phase == Phase::Move).then_some(Message::Region {
            axis,
            side,
            delta: pointer.delta.dot(direction) * world_per_point,
        })
    })
}

fn pane_fit<'a>(
    ctx: &mut BuildContext<'_, Message>,
    safe: Rect,
    key: &str,
    tooltip: &str,
    message: Message,
) -> Element<'a, Message> {
    let rect = Rect::from_min_size(
        Vec2::new((safe.max.x - PANE_CONTROL).max(safe.min.x), safe.min.y),
        Vec2::splat(PANE_CONTROL),
    );
    crate::view::glass_icon_button(
        ctx,
        key,
        Icon::Fit,
        PANE_CONTROL,
        GlassStyle::stage_control(),
        tooltip,
        message,
    )
    .at(rect)
}

fn split_handle<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
) -> Vec<Element<'a, Message>> {
    let volume = layout.volume;
    let section = layout.section;
    let x = (volume.max.x + section.min.x) * 0.5;
    let grab = Rect::from_center_size(
        Vec2::new(x, volume.center().y),
        Vec2::new(16.0, volume.height()),
    );
    let total = (volume.width() + section.width()).max(1.0);
    let origin = volume.min.x;
    let palette = ctx.theme().palette;
    let line = grana_ui::canvas(Length::Fill, grab.height(), move |painter, info| {
        let rect = info.rect;
        painter.rect(
            Rect::from_center_size(rect.center(), Vec2::new(1.0, rect.height())),
            Corners::ZERO,
            palette.separator.amplify(0.85),
        );
    })
    .at(grab)
    .named("split.line");
    let lens = grana_ui::canvas(Length::Fill, 36.0, |_painter, _info| {})
        .at(Rect::from_center_size(
            Vec2::new(x, volume.center().y),
            Vec2::new(8.0, 36.0),
        ))
        .named("split.lens")
        .glass(GlassStyle::lens());
    let bar = grana_ui::canvas(Length::Fill, grab.height(), move |painter, info| {
        let rect = info.rect;
        let engaged = info.hover.max(info.press);
        let size = Vec2::new(8.0 + engaged * 4.0, 36.0 + engaged * 18.0);
        let body = Rect::from_center_size(rect.center(), size);
        painter.rect(
            Rect::from_center_size(body.center(), Vec2::new(1.4, size.y * 0.34)),
            Corners::capsule(),
            palette.text_primary.with_alpha(0.30 + engaged * 0.40),
        );
        if engaged > 0.01 {
            painter.rect(
                Rect::from_center_size(body.center(), Vec2::new(1.0, 26.0 + 10.0 * engaged)),
                Corners::capsule(),
                Color::white_alpha(0.12 * engaged),
            );
        }
    })
    .at(grab)
    .named("split.bar")
    .cursor(Cursor::ResizeHorizontal)
    .on_pointer(move |pointer| {
        (pointer.phase == Phase::Move)
            .then_some(Message::SetSplit((pointer.position.x - origin) / total))
    })
    .layer(Layer::Labels);
    vec![line, lens, bar]
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
    let stage = layout.stage;
    let ceiling = stage.max.y - 24.0;
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
