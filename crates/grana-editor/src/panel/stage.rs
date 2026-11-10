use glam::Vec2;
use grana_display::{PANE_COUNT, PaneKind, SectionCamera};
use grana_figure::{self as overlay, Handle, Label, Legend, Pane};
use grana_ui::glass::GlassStyle;
use grana_ui::{
    BuildContext, Color, Corners, Cursor, Edges, Element, Icon, Key, Layer, Length, Level, Rect,
    TextStyle, TypeStyle, icon,
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

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
    full: Rect,
) -> Vec<Element<'a, Message>> {
    let mut elements = Vec::new();
    let palette = ctx.theme().palette;
    let size = full.size();
    let rects = layout.panes;
    let stage_backdrop = palette.scene_backdrop;
    let content = grana_ui::canvas(Length::Fill, full.height(), move |painter, info| {
        painter.rect(info.rect, Corners::ZERO, stage_backdrop);
        for rect in rects {
            painter.scene(rect, Corners::ZERO, scene_uv(rect, size));
        }
    })
    .named("stage.content")
    .at(full)
    .layer(Layer::Content);
    elements.push(content);
    elements.push(overlay_view(state, layout, full));
    elements.extend(region_handles(state, layout));
    for index in 0..PANE_COUNT {
        let safe = layout.panes[index].shrink(STAGE_INSET);
        elements.push(pane_fit(ctx, safe, index));
        elements.extend(pane_chip(ctx, layout, state, index));
    }
    elements.extend(split_handle(ctx, layout));
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
    let safe = layout.panes[state.active()].shrink(STAGE_INSET);
    let floor = safe.max.y;
    let rect = Rect::from_min_size(
        Vec2::new(safe.min.x, (floor - height).max(safe.min.y)),
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

fn plane_label(section: &SectionCamera) -> &'static str {
    let normal = section.normal().abs();
    if normal.z >= normal.y && normal.z >= normal.x {
        "XY"
    } else if normal.y >= normal.x {
        "XZ"
    } else {
        "YZ"
    }
}

fn pane<'a>(
    state: &'a View<'a>,
    layout: &Layout,
    index: usize,
    planes: &'a [SectionCamera],
    legends: &'a [Legend],
    labels: &'a [Label],
    annotations: &'a [grana_figure::AnnotationItem],
) -> Pane<'a> {
    let pane = state.panes.pane(index);
    Pane {
        kind: pane.kind(),
        camera: pane.camera(),
        section: pane.section(),
        plane: if pane.kind().volume() { planes } else { &[] },
        world: state.scene.dataset.world_size(),
        voxel: state.scene.dataset.voxel_size(),
        rect: layout.panes[index],
        safe: layout.panes[index].shrink(STAGE_INSET),
        region: &state.scene.region,
        clip: state.scene.clip,
        legends,
        labels,
        annotations,
        pending: state.workspace.annotation_pending,
        recording: state.io.exporting,
    }
}

fn overlay_view<'a>(state: &'a View<'a>, layout: &Layout, full: Rect) -> Element<'a, Message> {
    let legends = overlay::legends(state.scene);
    let labels = overlay::labels(state.scene, state.scene.time());
    let annotations = overlay::annotations(state.scene, state.scene.time());
    let sections = sections(state);
    let layout = *layout;
    grana_ui::canvas(Length::Fill, full.height(), move |painter, _info| {
        for index in 0..PANE_COUNT {
            let pane = pane(
                state,
                &layout,
                index,
                &sections,
                if index == 0 { &legends } else { &[] },
                &labels,
                &annotations,
            );
            overlay::paint(painter, &pane);
        }
    })
    .named("stage.overlay")
    .at(full)
    .layer(Layer::Content)
}

fn sections(state: &View<'_>) -> Vec<SectionCamera> {
    (0..PANE_COUNT)
        .filter(|index| state.panes.pane(*index).kind() == PaneKind::Section)
        .map(|index| state.panes.pane(index).section().clone())
        .collect()
}

fn region_handles<'a>(state: &'a View<'a>, layout: &Layout) -> Vec<Element<'a, Message>> {
    let sections = sections(state);
    let labels: [Label; 0] = [];
    let annotations: [grana_figure::AnnotationItem; 0] = [];
    let Some(index) = (0..PANE_COUNT).find(|index| state.panes.pane(*index).kind().volume()) else {
        return Vec::new();
    };
    let pane = pane(state, layout, index, &sections, &[], &labels, &annotations);
    overlay::handles(&pane)
        .into_iter()
        .map(|handle| region_handle(index, handle))
        .collect()
}

fn region_handle<'a>(pane: usize, handle: Handle) -> Element<'a, Message> {
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
    .key(Key::new(format!(
        "region.handle.{pane}.{}",
        axis * 2 + side
    )))
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

fn pane_fit<'a>(
    ctx: &mut BuildContext<'_, Message>,
    safe: Rect,
    index: usize,
) -> Element<'a, Message> {
    let rect = Rect::from_min_size(
        Vec2::new((safe.max.x - PANE_CONTROL).max(safe.min.x), safe.min.y),
        Vec2::splat(PANE_CONTROL),
    );
    crate::view::glass_icon_button(
        ctx,
        &format!("pane.fit.{index}"),
        Icon::Fit,
        PANE_CONTROL,
        GlassStyle::stage_control(),
        "Fit this view",
        Message::FitPane(index),
    )
    .at(rect)
}

fn split_handle<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
) -> Vec<Element<'a, Message>> {
    let left = layout.panes[0];
    let right = layout.panes[1];
    let x = (left.max.x + right.min.x) * 0.5;
    let grab = Rect::from_center_size(
        Vec2::new(x, left.center().y),
        Vec2::new(16.0, left.height()),
    );
    let total = (left.width() + right.width()).max(1.0);
    let origin = left.min.x;
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
            Vec2::new(x, left.center().y),
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
        (pointer.phase == grana_ui::Phase::Move)
            .then_some(Message::SetSplit((pointer.position.x - origin) / total))
    })
    .layer(Layer::Labels);
    vec![line, lens, bar]
}

fn pane_chip<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
    index: usize,
) -> Vec<Element<'a, Message>> {
    let pane = state.panes.pane(index);
    let rect = layout.panes[index];
    let safe = rect.shrink(STAGE_INSET);
    let active = state.active() == index;
    let title = match pane.kind() {
        PaneKind::Volume => "3D".to_string(),
        PaneKind::Section => format!("Slice · {}", plane_label(pane.section())),
    };
    let tint = if active {
        LABEL
    } else {
        Color::rgba(1.0, 1.0, 1.0, 0.62)
    };
    let key = format!("pane.menu.{index}");
    let open = state.workspace.sections.is_open(Key::new(&key));
    chip_control(
        ctx,
        &key,
        index,
        rect,
        safe,
        pane.kind(),
        &title,
        tint,
        open,
    )
}

#[allow(clippy::too_many_arguments)]
fn chip_control<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    index: usize,
    pane_rect: Rect,
    safe: Rect,
    kind: PaneKind,
    title: &str,
    tint: Color,
    open: bool,
) -> Vec<Element<'a, Message>> {
    let style = TextStyle::new(TypeStyle::new(11.5, 500.0, 15.0), tint);
    let maximum = (pane_rect.width() - 64.0).max(96.0);
    let layout = ctx.layout_text(title, style, maximum);
    let width = (layout.size.x + 46.0).min(maximum + 24.0);
    let anchor = grana_ui::canvas(Length::Fixed(width), 26.0, move |painter, info| {
        let rect = info.rect;
        icon::paint(
            match kind {
                PaneKind::Volume => Icon::Cube,
                PaneKind::Section => Icon::Section,
            },
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
        icon::chevron(
            painter,
            Vec2::new(rect.max.x - 12.0, rect.center().y),
            4.0,
            std::f32::consts::FRAC_PI_2,
            tint,
            1.5,
        );
    })
    .named(key)
    .at(Rect::from_min_size(safe.min, Vec2::new(width, 26.0)))
    .glass(GlassStyle::hud_chip())
    .cursor(Cursor::Pointing)
    .press(Message::ToggleOpen(Key::new(key)));
    if !open {
        return vec![anchor];
    }
    let theme = *ctx.theme();
    let options = [
        (
            "3D Volume",
            Message::SetPaneKind {
                index,
                kind: PaneKind::Volume,
            },
        ),
        ("XY Slice", Message::SetPaneSlice { index, preset: 0 }),
        ("XZ Slice", Message::SetPaneSlice { index, preset: 1 }),
        ("YZ Slice", Message::SetPaneSlice { index, preset: 2 }),
    ];
    let mut rows = Vec::new();
    for (slot, (label, message)) in options.into_iter().enumerate() {
        rows.push(
            grana_ui::widget::menu_row(&theme, label, false)
                .key(Key::index(key, slot))
                .press(message),
        );
    }
    let popup = grana_ui::popover::popup(Key::new(key), width.max(150.0), rows);
    vec![anchor, popup]
}
fn recording_chip<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let Some((frame, frames)) = state.io.exporting else {
        return Vec::new();
    };
    let safe = layout.panes[0].shrink(STAGE_INSET);
    vec![chip(
        ctx,
        "figure.recording",
        layout.panes[0],
        safe.min + Vec2::new(0.0, 34.0),
        Icon::Record,
        &format!("REC · frame {frame} of {frames}"),
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
