use grana_ui::{
    Align, BuildContext, Corners, Cursor, Edges, Element, Icon, IconStyle, Length, Level, Surface,
    TextStyle,
};

use crate::message::Message;

const ROW_HEIGHT: f32 = 56.0;
const ROW_INSET: f32 = 14.0;
const ROW_GAP: f32 = 12.0;
const ROW_ICON: f32 = 20.0;
const ROW_RADIUS: f32 = 14.0;
const CARD_INSET: f32 = 6.0;
const CARD_RADIUS: f32 = 20.0;
const SECTION_GAP: f32 = 10.0;

pub(super) fn section<'a>(
    ctx: &BuildContext<'_, Message>,
    title: Option<&str>,
    rows: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let card = grana_ui::column()
        .width(Length::Fill)
        .padding(Edges::all(CARD_INSET))
        .gap(2.0)
        .background(
            Surface::fill(theme.palette.surface_container).with_corners(Corners::all(CARD_RADIUS)),
        )
        .clipped()
        .children(rows);
    let Some(title) = title else {
        return card;
    };
    let label = ctx.text(
        title,
        TextStyle::new(theme.font(Level::Label), theme.palette.on_surface_variant),
    );
    let head = grana_ui::row()
        .width(Length::Fill)
        .padding(Edges {
            left: ROW_INSET + CARD_INSET,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        })
        .children([label]);
    grana_ui::column()
        .width(Length::Fill)
        .gap(SECTION_GAP)
        .children([head, card])
}

pub(super) fn action<'a>(
    ctx: &BuildContext<'_, Message>,
    icon: Icon,
    title: &str,
    detail: Option<&str>,
    trailing: Icon,
    message: Message,
    enabled: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let color = if enabled {
        theme.palette.on_surface_variant
    } else {
        theme.palette.on_surface_variant.with_alpha(0.38)
    };
    let row = frame(
        ctx,
        icon,
        title,
        detail,
        enabled,
        vec![ctx.icon(trailing, 18.0, color, IconStyle::REGULAR)],
    );
    if enabled {
        let corners = Corners::all(ROW_RADIUS);
        row.hover_highlight(hover(theme))
            .cursor(Cursor::Pointing)
            .focus_ring(corners)
            .press(message)
    } else {
        row
    }
}

pub(super) fn switch<'a>(
    ctx: &BuildContext<'_, Message>,
    icon: Icon,
    title: &str,
    value: bool,
    enabled: bool,
    on_toggle: impl Fn(bool) -> Message + 'static,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let message = on_toggle(!value);
    let row = frame(
        ctx,
        icon,
        title,
        None,
        enabled,
        vec![ctx.toggle(value).enabled(enabled).on_toggle(on_toggle)],
    );
    if enabled {
        let corners = Corners::all(ROW_RADIUS);
        row.hover_highlight(hover(theme))
            .cursor(Cursor::Pointing)
            .focus_ring(corners)
            .press(message)
    } else {
        row
    }
}

pub(super) fn button<'a>(
    ctx: &BuildContext<'_, Message>,
    icon: Icon,
    title: &str,
    detail: Option<&str>,
    control: Element<'a, Message>,
    enabled: bool,
) -> Element<'a, Message> {
    frame(ctx, icon, title, detail, enabled, vec![control])
}

pub(super) fn value<'a>(
    ctx: &BuildContext<'_, Message>,
    icon: Icon,
    title: &str,
    detail: &str,
) -> Element<'a, Message> {
    frame(ctx, icon, title, Some(detail), true, Vec::new())
}

pub(super) fn control<'a>(
    ctx: &BuildContext<'_, Message>,
    icon: Icon,
    title: &str,
    control: Element<'a, Message>,
    enabled: bool,
) -> Element<'a, Message> {
    frame(ctx, icon, title, None, enabled, vec![control])
}

fn frame<'a>(
    ctx: &BuildContext<'_, Message>,
    icon: Icon,
    title: &str,
    detail: Option<&str>,
    enabled: bool,
    trailing: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let title_color = if enabled {
        palette.on_surface
    } else {
        palette.on_surface.with_alpha(0.38)
    };
    let detail_color = if enabled {
        palette.outline
    } else {
        palette.outline.with_alpha(0.38)
    };
    let icon_color = if enabled {
        palette.on_surface_variant
    } else {
        palette.on_surface_variant.with_alpha(0.38)
    };
    let mut lines = vec![
        ctx.text(title, TextStyle::new(theme.font(Level::Body), title_color))
            .width(Length::Fill),
    ];
    if let Some(detail) = detail {
        lines.push(
            ctx.text(
                detail,
                TextStyle::new(theme.font(Level::Monospace), detail_color).truncating(),
            )
            .width(Length::Fill),
        );
    }
    let mut children = vec![
        ctx.icon(icon, ROW_ICON, icon_color, IconStyle::REGULAR),
        grana_ui::column()
            .width(Length::Fill)
            .gap(1.0)
            .children(lines),
    ];
    children.extend(trailing);
    grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(ROW_HEIGHT))
        .padding(Edges::symmetric(ROW_INSET, 0.0))
        .gap(ROW_GAP)
        .align(Align::Center)
        .children(children)
}

fn hover(theme: grana_ui::Theme) -> Surface {
    Surface::fill(theme.palette.surface_container_high).with_corners(Corners::all(ROW_RADIUS))
}
