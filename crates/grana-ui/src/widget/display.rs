use glam::Vec2;

use crate::color::Color;
use crate::element::{CanvasInfo, Element, Layout, Length, Size, spacer, text};
use crate::geometry::{Corners, Edges, Rect};
use crate::icon::{self, Icon};
use crate::paint::Painter;
use crate::style::{State, Surface};
use crate::text::{Align, TextStyle};
use crate::theme::{Level, Role, Theme};

const HEADING_HEIGHT: f32 = 24.0;

pub fn card<'a, A>(
    theme: &Theme,
    role: Role,
    padding: Edges,
    gap: f32,
    children: impl IntoIterator<Item = Element<'a, A>>,
) -> Element<'a, A> {
    Element::column()
        .padding(padding)
        .gap(gap)
        .background(theme.surface(role, State::Rest))
        .children(children)
}

pub fn group_card<'a, A>(
    theme: &Theme,
    slot: (usize, usize),
    active: bool,
    padding: Edges,
    gap: f32,
    children: impl IntoIterator<Item = Element<'a, A>>,
) -> Element<'a, A> {
    let role = if active {
        Role::CardElevated
    } else {
        Role::Card
    };
    let corners = Corners::group_vertical(
        slot.0,
        slot.1,
        theme.metrics.corner_lg,
        theme.metrics.corner_xs,
        active,
    );
    Element::column()
        .padding(padding)
        .gap(gap)
        .background(theme.surface(role, State::Rest).with_corners(corners))
        .children(children)
}

pub fn divider<'a, A>(theme: &Theme) -> Element<'a, A> {
    Element::column()
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .background(Surface::fill(
            theme.palette.outline_variant.with_alpha(0.45),
        ))
}

pub fn card_title<'a, A>(
    theme: &Theme,
    symbol: Icon,
    title: &str,
    detail: Option<&str>,
) -> Element<'a, A> {
    heading(Heading {
        symbol: Some(symbol),
        title: title.to_string(),
        detail: detail.map(str::to_string),
        title_style: TextStyle::new(theme.font(Level::Title), theme.palette.on_surface)
            .truncating(),
        detail_style: TextStyle::new(theme.font(Level::Caption), theme.palette.on_surface_variant)
            .aligned(Align::Center)
            .truncating(),
        accent: theme.palette.primary,
        pill: theme.palette.surface_container_high,
        pill_height: 20.0,
        inset: 22.0,
    })
}

pub fn section_header<'a, A>(theme: &Theme, title: &str, detail: Option<&str>) -> Element<'a, A> {
    heading(Heading {
        symbol: None,
        title: title.to_uppercase(),
        detail: detail.map(str::to_string),
        title_style: TextStyle::new(
            theme.font(Level::Overline),
            theme.palette.on_surface_variant,
        )
        .truncating(),
        detail_style: TextStyle::new(theme.font(Level::Caption), theme.palette.on_surface_variant)
            .aligned(Align::Center),
        accent: theme.palette.primary,
        pill: theme.palette.surface_container_high,
        pill_height: 19.0,
        inset: 4.0,
    })
}

pub fn form_row<'a, A>(
    theme: &Theme,
    width: f32,
    label: &str,
    content: Element<'a, A>,
) -> Element<'a, A> {
    let style = TextStyle::new(theme.label_font(), theme.palette.on_surface_variant).truncating();
    Element::row()
        .width(Length::Fill)
        .align(Align::Center)
        .children([text(label, style).width(Length::Fixed(width)), content])
}

pub fn empty_state<'a, A>(
    theme: &Theme,
    symbol: Icon,
    title: &str,
    detail: &str,
) -> Element<'a, A> {
    let palette = theme.palette;
    let art = Element::column()
        .width(Length::Fixed(48.0))
        .height(Length::Fixed(48.0))
        .align(Align::Center)
        .justify(Align::Center)
        .background(Surface::fill(palette.secondary_container).with_corners(Corners::all(14.0)))
        .children([super::icon::icon(
            symbol,
            21.0,
            palette.on_secondary_container,
            1.6,
        )]);
    let card = Element::column()
        .width(Length::Maximum(480.0))
        .padding(Edges::symmetric(28.0, 24.0))
        .gap(6.0)
        .align(Align::Center)
        .background(
            Surface::fill(palette.surface_container)
                .with_corners(Corners::all(theme.metrics.corner_lg)),
        )
        .children([
            art,
            spacer(6.0),
            text(
                title,
                TextStyle::new(theme.font(Level::Headline), palette.on_surface)
                    .aligned(Align::Center),
            ),
            text(
                detail,
                TextStyle::new(theme.font(Level::Body), palette.on_surface_variant)
                    .wrapping()
                    .aligned(Align::Center),
            ),
        ]);
    Element::column()
        .width(Length::Fill)
        .align(Align::Center)
        .children([card])
}

#[derive(Clone)]
struct Heading {
    symbol: Option<Icon>,
    title: String,
    detail: Option<String>,
    title_style: TextStyle,
    detail_style: TextStyle,
    accent: Color,
    pill: Color,
    pill_height: f32,
    inset: f32,
}

impl std::hash::Hash for Heading {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.symbol.hash(state);
        self.title.hash(state);
        self.detail.hash(state);
        self.title_style.hash(state);
        self.detail_style.hash(state);
        self.accent.hash(state);
        self.pill.hash(state);
        self.pill_height.to_bits().hash(state);
        self.inset.to_bits().hash(state);
    }
}

fn heading<'a, A>(props: Heading) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(HEADING_HEIGHT)),
        ..Layout::column()
    })
    .view(props, paint_heading)
}

fn paint_heading(props: &Heading, painter: &mut Painter, info: CanvasInfo<'_>) {
    let rect = info.rect;
    if let Some(symbol) = props.symbol {
        icon::paint(
            symbol,
            painter,
            Vec2::new(rect.min.x + 8.0, rect.center().y),
            14.0,
            props.accent,
            1.6,
        );
    }
    let detail = props
        .detail
        .as_ref()
        .map(|detail| (detail, painter.measure(detail, props.detail_style).x + 14.0));
    let right = match &detail {
        Some((_, width)) => rect.max.x - width - 8.0,
        None => rect.max.x,
    };
    painter.text_in(
        Rect::from_min_max(
            Vec2::new(rect.min.x + props.inset, rect.min.y),
            Vec2::new(right.max(rect.min.x + props.inset), rect.max.y),
        ),
        &props.title,
        props.title_style,
        Align::Center,
    );
    if let Some((detail, width)) = detail {
        let pill = Rect::from_min_size(
            Vec2::new(
                rect.max.x - width,
                rect.center().y - props.pill_height * 0.5,
            ),
            Vec2::new(width, props.pill_height),
        );
        painter.rect(pill, Corners::capsule(), props.pill);
        painter.text_in(pill, detail, props.detail_style, Align::Center);
    }
}
