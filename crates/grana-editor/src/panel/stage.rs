use glam::{Vec2, Vec3};
use grana_figure::{self as overlay, Handle, Label, Legend, Overlay};
use grana_ui::glass::GlassStyle;
use grana_ui::{
    BuildContext, Color, Corners, Cursor, Element, Icon, Key, Layer, Length, Phase, Rect,
    TextStyle, TypeStyle, icon,
};

use crate::message::Message;
use crate::panel::{GAP, Layout, MARGIN};
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
    section_stage: Rect,
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
    let top = layout.toolbar_left.max.y + GAP;
    elements.push(pane_fit(
        ctx,
        layout.volume,
        top,
        "volume.fit",
        "Fit volume",
        Message::FitVolume,
    ));
    elements.push(pane_fit(
        ctx,
        layout.section,
        top,
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
    let axis = state.viewport.section().axis();
    let plane = state.viewport.camera().focus()[axis.index()] + 0.5 * geometry.world[axis.index()];
    elements.push(chip(
        ctx,
        "figure.section",
        geometry.section_rect,
        geometry.section_safe.min,
        Icon::Section,
        &format!("{} · {} = {plane:.2} µm", plane_name(axis), axis.name()),
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
    elements
}

fn geometry(layout: &Layout, state: &View<'_>) -> Geometry {
    let axis = state.viewport.section().axis();
    let (plane_u, plane_v) = axis.in_plane();
    let world = state.scene.dataset.world_size();
    let points_per_micron = layout.section.height() / state.viewport.section().span();
    let section_stage = Rect::from_center_size(
        layout.section.center(),
        Vec2::new(
            world[plane_u.index()] * points_per_micron,
            world[plane_v.index()] * points_per_micron,
        ),
    )
    .intersect(layout.section);
    let safe_left = if layout.sidebar_visible {
        layout.sidebar.max.x.max(layout.stage.min.x) + GAP
    } else {
        layout.stage.min.x + MARGIN
    };
    let safe_top = layout.toolbar_left.max.y + GAP;
    let safe_bottom = layout
        .dock
        .or(layout.transport)
        .map_or(layout.stage.max.y - MARGIN, |rect| rect.min.y - GAP);
    let volume_safe = Rect::from_min_max(
        Vec2::new(safe_left, safe_top),
        Vec2::new(layout.volume.max.x - MARGIN, safe_bottom),
    );
    let section_safe = Rect::from_min_max(
        Vec2::new(layout.section.min.x + MARGIN, safe_top),
        Vec2::new(layout.section.max.x - MARGIN, safe_bottom),
    );
    Geometry {
        volume_rect: layout.volume,
        section_rect: layout.section,
        volume_safe,
        section_safe,
        section_stage,
        world,
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
        section_stage: geometry.section_stage,
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
    let annotations = overlay::annotations(state.scene);
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
    pane: Rect,
    top: f32,
    key: &str,
    tooltip: &str,
    message: Message,
) -> Element<'a, Message> {
    let rect = Rect::from_min_size(
        Vec2::new(pane.max.x - PANE_CONTROL - MARGIN, top),
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

fn plane_name(axis: grana_volume::Axis) -> &'static str {
    match axis {
        grana_volume::Axis::X => "YZ",
        grana_volume::Axis::Y => "XZ",
        grana_volume::Axis::Z => "XY",
    }
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
    let ceiling = layout
        .dock
        .or(layout.transport)
        .map_or(stage.max.y - 24.0, |rect| rect.min.y - 14.0);
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
