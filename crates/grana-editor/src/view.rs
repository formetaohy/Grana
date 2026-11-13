use glam::Vec2;
use grana_session::Command;
use grana_ui::popover::{
    action_row as action_row_widget, menu_row as menu_row_widget, separator as separator_widget,
};
use grana_ui::{
    Align, BuildContext, Corners, Edges, Element, Icon, Key, Length, Level, Motion, Rect, Role,
    State, TextStyle, icon,
};

use crate::message::Message;

pub type View<'a> = Element<'a, Message>;

pub fn card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    gap: f32,
    children: Vec<View<'a>>,
) -> View<'a> {
    let surface = ctx.theme().surface(Role::Card, State::Rest);
    grana_ui::column()
        .padding(Edges::all(12.0))
        .gap(gap)
        .background(surface)
        .children(children)
}

pub fn group_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    index: usize,
    count: usize,
    active: bool,
    gap: f32,
    children: Vec<View<'a>>,
) -> View<'a> {
    let theme = *ctx.theme();
    let role = if active {
        Role::CardElevated
    } else {
        Role::Card
    };
    let corners = Corners::group_vertical(
        index,
        count,
        theme.metrics.corner_lg,
        theme.metrics.corner_xs,
        active,
    );
    let surface = theme.surface(role, State::Rest).with_corners(corners);
    grana_ui::column()
        .padding(Edges::all(12.0))
        .gap(gap)
        .background(surface)
        .children(children)
}

pub fn card_title<'a>(
    ctx: &mut BuildContext<'_, Message>,
    symbol: Icon,
    title: &str,
    detail: Option<&str>,
) -> View<'a> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let title_style = TextStyle::new(theme.font(Level::Title), palette.on_surface);
    let detail_style = TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant)
        .aligned(Align::Center)
        .truncating();
    let title = title.to_string();
    let detail = detail.map(str::to_string);
    grana_ui::canvas(Length::Fill, 24.0, move |painter, info| {
        let rect = info.rect;
        icon::paint(
            symbol,
            painter,
            Vec2::new(rect.min.x + 8.0, rect.center().y),
            14.0,
            palette.primary,
            1.6,
        );
        let detail_width = detail
            .as_ref()
            .map_or(0.0, |detail| painter.measure(detail, detail_style).x + 14.0);
        let title_right = rect.max.x
            - if detail.is_some() {
                detail_width + 8.0
            } else {
                0.0
            };
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 22.0, rect.min.y),
                Vec2::new(title_right.max(rect.min.x + 22.0), rect.max.y),
            ),
            &title,
            title_style.truncating(),
            Align::Center,
        );
        if let Some(detail) = &detail {
            let pill = Rect::from_min_size(
                Vec2::new(rect.max.x - detail_width, rect.center().y - 10.0),
                Vec2::new(detail_width, 20.0),
            );
            painter.rect(pill, Corners::capsule(), palette.surface_container_high);
            painter.text_in(pill, detail, detail_style, Align::Center);
        }
    })
}

pub fn divider<'a>(ctx: &mut BuildContext<'_, Message>) -> View<'a> {
    let palette = ctx.theme().palette;
    let metrics = ctx.theme().metrics;
    let height = 1.0 + metrics.gap;
    grana_ui::canvas(Length::Fill, height, move |painter, info| {
        painter.rect(
            Rect::from_min_size(
                Vec2::new(info.rect.min.x, info.rect.center().y),
                Vec2::new(info.rect.width(), 1.0),
            ),
            Corners::ZERO,
            palette.outline_variant.with_alpha(0.45),
        );
    })
    .cached(grana_ui::CanvasKey::LAYOUT)
}

pub fn section_header<'a>(
    ctx: &mut BuildContext<'_, Message>,
    title: &str,
    detail: Option<&str>,
) -> View<'a> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let style = TextStyle::new(theme.font(Level::Overline), palette.on_surface_variant);
    let detail_style = TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant)
        .aligned(Align::Center);
    let title = title.to_uppercase();
    let detail = detail.map(str::to_string);
    grana_ui::canvas(Length::Fill, 24.0, move |painter, info| {
        let detail_width = detail
            .as_ref()
            .map_or(0.0, |detail| painter.measure(detail, detail_style).x + 14.0);
        let title_right = info.rect.max.x
            - if detail.is_some() {
                detail_width + 8.0
            } else {
                0.0
            };
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(info.rect.min.x + 4.0, info.rect.min.y),
                Vec2::new(title_right.max(info.rect.min.x + 4.0), info.rect.max.y),
            ),
            &title,
            style.truncating(),
            Align::Center,
        );
        if let Some(detail) = &detail {
            let pill = Rect::from_min_size(
                Vec2::new(info.rect.max.x - detail_width, info.rect.center().y - 9.5),
                Vec2::new(detail_width, 19.0),
            );
            painter.rect(pill, Corners::capsule(), palette.surface_container_high);
            painter.text_in(pill, detail, detail_style, Align::Center);
        }
    })
}

pub fn disclosure_group_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    slot: (usize, usize),
    key: Key,
    head: (Icon, &str, Option<&str>),
    children: Vec<View<'a>>,
    open: bool,
) -> View<'a> {
    let theme = *ctx.theme();
    let header = disclosure_header(ctx, key, head.0, head.1, head.2, open, theme);
    let mut items = vec![header];
    if open && !children.is_empty() {
        items.extend(children);
    }
    group_card(ctx, slot.0, slot.1, open, 8.0, items)
}

fn disclosure_header<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: Key,
    symbol: Icon,
    title: &str,
    detail: Option<&str>,
    open: bool,
    theme: grana_ui::Theme,
) -> View<'a> {
    let palette = theme.palette;
    let title = title.to_string();
    let detail = detail.map(str::to_string);
    let title_style = TextStyle::new(theme.font(Level::Title), palette.on_surface);
    let detail_style = TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant)
        .aligned(Align::End)
        .truncating();
    let arrow = ctx.animate(
        key.then("chevron").id(),
        if open { 1.0 } else { 0.0 },
        Motion::spatial_fast(),
    );
    grana_ui::canvas(Length::Fill, 24.0, move |painter, info| {
        let rect = info.rect;
        icon::paint(
            symbol,
            painter,
            Vec2::new(rect.min.x + 8.0, rect.center().y),
            14.0,
            if open {
                palette.primary
            } else {
                palette
                    .on_surface_variant
                    .lerp(palette.on_surface, info.hover)
            },
            1.6,
        );
        let detail_width = detail
            .as_ref()
            .map_or(0.0, |detail| painter.measure(detail, detail_style).x);
        let detail_left = rect.max.x - 28.0 - detail_width;
        let title_right = rect.max.x
            - 28.0
            - if detail.is_some() {
                detail_width + 8.0
            } else {
                0.0
            };
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 22.0, rect.min.y),
                Vec2::new(title_right.max(rect.min.x + 22.0), rect.max.y),
            ),
            &title,
            title_style.truncating(),
            Align::Center,
        );
        if let Some(detail) = &detail {
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(detail_left.max(rect.min.x + 22.0), rect.min.y),
                    Vec2::new(rect.max.x - 28.0, rect.max.y),
                ),
                detail,
                detail_style,
                Align::Center,
            );
        }
        let badge = Rect::from_center_size(
            Vec2::new(rect.max.x - 11.0, rect.center().y),
            Vec2::splat(22.0),
        );
        painter.rect(
            badge,
            Corners::capsule(),
            if open {
                palette.secondary_container
            } else {
                palette
                    .surface_container_high
                    .lerp(palette.surface_container_highest, info.hover)
            },
        );
        icon::chevron(
            painter,
            badge.center(),
            8.0,
            std::f32::consts::FRAC_PI_2 * arrow,
            if open {
                palette.on_secondary_container
            } else {
                palette.on_surface_variant
            },
            1.6,
        );
    })
    .key(key)
    .press(Message::ToggleOpen(key))
    .focus_ring(Corners::capsule())
}

pub fn form_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    label: &str,
    content: View<'a>,
) -> View<'a> {
    let height = ctx.theme().metrics.control_height;
    let palette = ctx.theme().palette;
    let label = label.to_string();
    let label_style =
        TextStyle::new(ctx.theme().label_font(), palette.on_surface_variant).truncating();
    let label_element = grana_ui::canvas(
        Length::Fixed(crate::panel::FORM_LABEL),
        height,
        move |painter, info| {
            painter.text_in(
                Rect::from_min_max(
                    info.rect.min,
                    Vec2::new(info.rect.max.x - 6.0, info.rect.max.y),
                ),
                &label,
                label_style,
                Align::Center,
            );
        },
    );
    grana_ui::row()
        .gap(0.0)
        .align(Align::Center)
        .width(Length::Fill)
        .children([label_element, content])
}

pub fn menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    anchor: Key,
    width: f32,
    rows: Vec<View<'a>>,
) -> View<'a> {
    let _ = ctx;
    grana_ui::popover::popup(anchor, width, rows)
}

pub fn action_menu_message<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    symbol: Icon,
    label: &str,
    shortcut: Option<&str>,
    message: Message,
) -> View<'a> {
    let theme = *ctx.theme();
    action_row_widget(&theme, symbol, label, shortcut)
        .named(key)
        .press(message)
}

pub fn menu_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: &str,
    checked: bool,
    message: Option<Message>,
) -> View<'a> {
    let theme = *ctx.theme();
    let row = menu_row_widget(&theme, label, checked).named(key);
    match message {
        Some(message) => row.press(message),
        None => row,
    }
}

pub fn menu_separator<'a>(_ctx: &mut BuildContext<'_, Message>) -> View<'a> {
    separator_widget()
}

pub fn popup_rows<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    items: &[String],
    selected: usize,
    select: impl Fn(usize) -> Message,
) -> Vec<View<'a>> {
    let theme = *ctx.theme();
    let mut rows = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        rows.push(
            menu_row_widget(&theme, item, index == selected)
                .key(Key::index(key, index))
                .press(select(index)),
        );
    }
    rows
}

pub fn labelled<'a>(ctx: &mut BuildContext<'_, Message>, label: &str, value: &str) -> View<'a> {
    let theme = *ctx.theme();
    grana_ui::widget::labelled(&theme, label, value)
}

pub fn caption<'a>(ctx: &mut BuildContext<'_, Message>, text: &str) -> View<'a> {
    let theme = *ctx.theme();
    grana_ui::widget::caption(&theme, text)
}

pub fn weak<'a>(ctx: &mut BuildContext<'_, Message>, text: &str) -> View<'a> {
    let theme = *ctx.theme();
    grana_ui::widget::weak(&theme, text)
}

pub fn spinner<'a>(_ctx: &mut BuildContext<'_, Message>, size: f32) -> View<'a> {
    grana_ui::widget::spinner(size)
}

pub fn button<'a>(ctx: &mut BuildContext<'_, Message>, label: &str, role: Role) -> View<'a> {
    let theme = *ctx.theme();
    grana_ui::widget::button(&theme, label, role)
}

pub fn button_with<'a>(
    ctx: &mut BuildContext<'_, Message>,
    label: &str,
    role: Role,
    width: Length,
) -> View<'a> {
    let theme = *ctx.theme();
    grana_ui::widget::button_width(&theme, label, role, width)
}

pub fn button_shaped<'a>(
    ctx: &mut BuildContext<'_, Message>,
    label: &str,
    role: Role,
    width: Length,
    corners: Corners,
) -> View<'a> {
    button_with(ctx, label, role, width).shaped(corners)
}

pub fn icon_button<'a>(
    _ctx: &mut BuildContext<'_, Message>,
    key: &str,
    symbol: Icon,
    size: f32,
    role: Role,
    tooltip: &str,
    message: Message,
) -> View<'a> {
    grana_ui::widget::icon_button(symbol, size, role, tooltip)
        .named(key)
        .press(message)
}

pub fn split_button<'a>(
    ctx: &mut BuildContext<'_, Message>,
    symbol: Option<Icon>,
    label: &str,
    role: Role,
    leading: (&str, Message),
    trailing: (Key, Message),
) -> View<'a> {
    let open = ctx.popup_open(trailing.0);
    grana_ui::widget::split_button(
        ctx.theme(),
        grana_ui::widget::Split {
            symbol,
            label,
            role,
            leading_key: Key::new(leading.0),
            leading_action: leading.1,
            trailing_key: trailing.0,
            trailing_action: trailing.1,
            open,
        },
    )
}

pub fn slider_readout<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: Option<&str>,
    value: f32,
    range: (std::ops::RangeInclusive<f32>, bool),
    readout: &str,
    command: impl Fn(f32) -> Command + 'static,
) -> View<'a> {
    let theme = *ctx.theme();
    grana_ui::widget::slider_readout(&theme, label, value, range.0, range.1, readout)
        .named(key)
        .on_slide(move |value| Message::Command(command(value)))
}

pub fn slider_event<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: Option<&str>,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    readout: &str,
    message: impl Fn(f32) -> Message + 'static,
) -> View<'a> {
    let theme = *ctx.theme();
    grana_ui::widget::slider_readout(&theme, label, value, range, false, readout)
        .named(key)
        .on_slide(message)
}

pub fn toggle_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: &str,
    value: bool,
    command: impl Fn(bool) -> Command + 'static,
) -> View<'a> {
    let theme = *ctx.theme();
    grana_ui::widget::toggle_row(&theme, label, value)
        .named(key)
        .on_toggle(move |value| Message::Command(command(value)))
}

pub fn segmented<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    options: &[&str],
    selected: usize,
    message: impl Fn(usize) -> Message + 'static,
) -> View<'a> {
    let theme = *ctx.theme();
    let labels = options
        .iter()
        .map(|option| option.to_string())
        .collect::<Vec<_>>();
    grana_ui::widget::segmented(&theme, &labels, selected)
        .named(key)
        .on_segment(message)
}

pub fn segmented_fill<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    options: &[&str],
    selected: usize,
    message: impl Fn(usize) -> Message + 'static,
) -> View<'a> {
    let theme = *ctx.theme();
    let labels = options
        .iter()
        .map(|option| option.to_string())
        .collect::<Vec<_>>();
    grana_ui::widget::segmented_fill(&theme, &labels, selected)
        .named(key)
        .on_segment(message)
}

pub fn tabs<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    options: &[&str],
    selected: usize,
    message: impl Fn(usize) -> Message + 'static,
) -> View<'a> {
    let theme = *ctx.theme();
    let labels = options
        .iter()
        .map(|option| option.to_string())
        .collect::<Vec<_>>();
    grana_ui::widget::tabs(&theme, &labels, selected)
        .named(key)
        .on_segment(message)
}

pub fn empty_state<'a>(
    ctx: &mut BuildContext<'_, Message>,
    symbol: Icon,
    title: &str,
    detail: &str,
) -> View<'a> {
    let palette = ctx.theme().palette;
    let theme = *ctx.theme();
    let available = ctx.size();
    let width = available.x.min(480.0);
    let detail_style = TextStyle::new(theme.font(Level::Body), palette.on_surface_variant)
        .wrapping()
        .aligned(Align::Center);
    let detail_layout = ctx.layout_text(detail, detail_style, (width - 56.0).max(80.0));
    let height = 24.0 + 48.0 + 12.0 + 22.0 + 6.0 + detail_layout.size.y + 24.0;
    let title = title.to_string();
    let detail = detail.to_string();
    let detail_height = detail_layout.size.y;
    let corners = Corners::all(theme.metrics.corner_lg);
    grana_ui::canvas(Length::Fill, height, move |painter, info| {
        let outer = info.rect;
        let rect = Rect::from_min_size(
            Vec2::new(outer.min.x + (outer.width() - width) * 0.5, outer.min.y),
            Vec2::new(width, height),
        );
        painter.rect(rect, corners, palette.surface_container);
        let art = Rect::from_center_size(
            Vec2::new(rect.center().x, rect.min.y + 24.0 + 24.0),
            Vec2::splat(48.0),
        );
        painter.rect(art, Corners::all(14.0), palette.secondary_container);
        icon::paint(
            symbol,
            painter,
            art.center(),
            21.0,
            palette.on_secondary_container,
            1.6,
        );
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(rect.min.x + 28.0, rect.min.y + 24.0 + 48.0 + 12.0),
                Vec2::new(rect.width() - 56.0, 22.0),
            ),
            &title,
            TextStyle::new(theme.font(Level::Headline), palette.on_surface).aligned(Align::Center),
            Align::Center,
        );
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(
                    rect.min.x + 28.0,
                    rect.min.y + 24.0 + 48.0 + 12.0 + 22.0 + 6.0,
                ),
                Vec2::new(rect.width() - 56.0, detail_height),
            ),
            &detail,
            detail_style,
            Align::Center,
        );
    })
}

pub fn column_label(language: grana_i18n::Language, column: &grana_measure::Column) -> String {
    let raw = crate::text::column(language, column.label());
    let mut chars = raw.chars();
    let label = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    let unit = column.unit().suffix();
    if unit.is_empty() {
        label
    } else {
        format!("{label} ({unit})")
    }
}

pub fn progress(
    analysis: &[grana_scene::Progress],
    job: grana_scene::Job,
) -> Option<grana_scene::Progress> {
    analysis
        .iter()
        .copied()
        .find(|progress| progress.job == job)
}

pub fn progress_caption(
    language: grana_i18n::Language,
    kind: grana_scene::ObjectKind,
    job: grana_scene::Job,
    progress: grana_scene::Progress,
    frames: u32,
) -> String {
    let unit = match job {
        grana_scene::Job::Detect(_) => match kind {
            grana_scene::ObjectKind::Spots => grana_i18n::common::blocks(language),
            grana_scene::ObjectKind::Surfaces => grana_i18n::common::tiles(language),
            grana_scene::ObjectKind::Filaments => grana_i18n::common::frames(language),
            grana_scene::ObjectKind::Cells => grana_i18n::common::frames(language),
        },
        grana_scene::Job::Import(_) => grana_i18n::common::tiles(language),
        grana_scene::Job::Localizations(_) => grana_i18n::common::steps(language),
        grana_scene::Job::Clusters(_) => grana_i18n::common::frames(language),
        grana_scene::Job::Track(_)
        | grana_scene::Job::Recut(_)
        | grana_scene::Job::Relation { .. }
        | grana_scene::Job::Coloc
        | grana_scene::Job::ObjectColoc(_)
        | grana_scene::Job::Drift
        | grana_scene::Job::Register => grana_i18n::common::frames(language),
        grana_scene::Job::Calibrate => grana_i18n::common::beads(language),
    };
    let prefix = match job {
        grana_scene::Job::Detect(_) | grana_scene::Job::Import(_) if frames > 1 => {
            grana_i18n::common::frame_prefix(language, progress.time + 1)
        }
        grana_scene::Job::Detect(_)
        | grana_scene::Job::Import(_)
        | grana_scene::Job::Recut(_)
        | grana_scene::Job::Track(_)
        | grana_scene::Job::Relation { .. }
        | grana_scene::Job::Coloc
        | grana_scene::Job::ObjectColoc(_)
        | grana_scene::Job::Localizations(_)
        | grana_scene::Job::Clusters(_)
        | grana_scene::Job::Drift
        | grana_scene::Job::Register
        | grana_scene::Job::Calibrate => String::new(),
    };
    if progress.total > 0 {
        format!(
            "{prefix}{}",
            grana_i18n::common::progress(language, progress.done, progress.total, unit)
        )
    } else {
        format!("{prefix}{}", grana_i18n::common::preparing(language))
    }
}
