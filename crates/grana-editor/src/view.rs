use glam::Vec2;
use grana_session::Command;
use grana_ui::glass::GlassStyle;
use grana_ui::popover::{
    action_row as action_row_widget, menu_row as menu_row_widget, separator as separator_widget,
};
use grana_ui::{
    Align, BuildContext, Corners, Edges, Element, Icon, Key, Length, Level, Rect, Role, State,
    TextStyle, TypeStyle, icon,
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

pub fn card_title<'a>(
    ctx: &mut BuildContext<'_, Message>,
    symbol: Icon,
    title: &str,
    detail: Option<&str>,
) -> View<'a> {
    let palette = ctx.theme().palette;
    let title_style = TextStyle::new(TypeStyle::new(12.5, 600.0, 17.0), palette.text_primary);
    let detail_style = TextStyle::new(ctx.theme().font(Level::Caption), palette.text_tertiary)
        .aligned(Align::End)
        .truncating();
    let title = title.to_string();
    let detail = detail.map(str::to_string);
    grana_ui::canvas(Length::Fill, 22.0, move |painter, info| {
        let rect = info.rect;
        icon::paint(
            symbol,
            painter,
            Vec2::new(rect.min.x + 7.0, rect.center().y),
            13.0,
            palette.text_secondary,
            1.5,
        );
        painter.text_in(
            Rect::from_min_max(Vec2::new(rect.min.x + 20.0, rect.min.y), rect.max),
            &title,
            title_style,
            Align::Center,
        );
        if let Some(detail) = &detail {
            painter.text_in(
                Rect::from_min_max(Vec2::new(rect.min.x + 96.0, rect.min.y), rect.max),
                detail,
                detail_style,
                Align::Center,
            );
        }
    })
}

pub fn divider<'a>(ctx: &mut BuildContext<'_, Message>) -> View<'a> {
    let palette = ctx.theme().palette;
    let metrics = ctx.theme().metrics;
    let thickness = metrics.hairline;
    let height = thickness + metrics.gap;
    grana_ui::canvas(Length::Fill, height, move |painter, info| {
        painter.rect(
            Rect::from_min_size(
                Vec2::new(info.rect.min.x, info.rect.center().y),
                Vec2::new(info.rect.width(), thickness),
            ),
            Corners::ZERO,
            palette.separator,
        );
    })
}

pub fn section_header<'a>(
    ctx: &mut BuildContext<'_, Message>,
    title: &str,
    detail: Option<&str>,
) -> View<'a> {
    let palette = ctx.theme().palette;
    let font = TypeStyle {
        size: 10.0,
        weight: 600.0,
        tracking: 0.7,
        leading: 14.0,
    };
    let style = TextStyle::new(font, palette.text_tertiary);
    let detail_style =
        TextStyle::new(ctx.theme().font(Level::Caption), palette.text_tertiary).aligned(Align::End);
    let title = title.to_uppercase();
    let detail = detail.map(str::to_string);
    grana_ui::canvas(Length::Fill, 22.0, move |painter, info| {
        painter.text_in(info.rect, &title, style, Align::Center);
        if let Some(detail) = &detail {
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(info.rect.min.x + 64.0, info.rect.min.y),
                    info.rect.max,
                ),
                detail,
                detail_style,
                Align::Center,
            );
        }
    })
}

pub fn disclosure_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: Key,
    symbol: Icon,
    title: &str,
    detail: Option<&str>,
    children: Vec<View<'a>>,
    open: bool,
) -> View<'a> {
    let header = disclosure_header(ctx, key, symbol, title, detail, open, *ctx.theme());
    let mut items = vec![header];
    if open {
        items.push(divider(ctx));
        items.extend(children);
    }
    let surface = ctx.theme().surface(Role::Card, State::Rest);
    grana_ui::column()
        .padding(Edges::all(12.0))
        .gap(8.0)
        .background(surface)
        .children(items)
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
    let title_style = TextStyle::new(TypeStyle::new(12.5, 600.0, 17.0), palette.text_primary);
    let detail_style = TextStyle::new(theme.font(Level::Caption), palette.text_tertiary)
        .aligned(Align::End)
        .truncating();
    let _ = ctx;
    grana_ui::canvas(Length::Fill, 22.0, move |painter, info| {
        let rect = info.rect;
        icon::paint(
            symbol,
            painter,
            Vec2::new(rect.min.x + 7.0, rect.center().y),
            13.0,
            palette
                .text_secondary
                .lerp(palette.text_primary, info.hover),
            1.5,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 20.0, rect.min.y),
                Vec2::new(rect.max.x - 20.0, rect.max.y),
            ),
            &title,
            title_style,
            Align::Center,
        );
        if let Some(detail) = &detail {
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(rect.min.x + 96.0, rect.min.y),
                    Vec2::new(rect.max.x - 20.0, rect.max.y),
                ),
                detail,
                detail_style,
                Align::Center,
            );
        }
        icon::chevron(
            painter,
            Vec2::new(rect.max.x - 6.0, rect.center().y),
            8.0,
            std::f32::consts::FRAC_PI_2 * if open { 1.0 } else { 0.0 },
            palette
                .text_tertiary
                .lerp(palette.text_secondary, info.hover),
            1.5,
        );
    })
    .key(key)
    .press(Message::ToggleOpen(key))
    .focus_ring(Corners::all(8.0))
}

pub fn form_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    label: &str,
    content: View<'a>,
) -> View<'a> {
    let height = ctx.theme().metrics.control_height;
    let palette = ctx.theme().palette;
    let label = label.to_string();
    let label_style = TextStyle::new(ctx.theme().label_font(), palette.text_tertiary).truncating();
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

pub fn glass_icon_button<'a>(
    _ctx: &mut BuildContext<'_, Message>,
    key: &str,
    symbol: Icon,
    size: f32,
    style: GlassStyle,
    tooltip: &str,
    message: Message,
) -> View<'a> {
    grana_ui::widget::glass_icon_button(None, symbol, size, style, tooltip)
        .named(key)
        .press(message)
}

pub fn glass_icon_button_disabled<'a>(
    _ctx: &mut BuildContext<'_, Message>,
    key: &str,
    symbol: Icon,
    size: f32,
    style: GlassStyle,
) -> View<'a> {
    grana_ui::widget::glass_icon_button_disabled(None, symbol, size, style).named(key)
}

#[expect(clippy::too_many_arguments)]
pub fn slider_readout<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: Option<&str>,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    logarithmic: bool,
    readout: &str,
    command: impl Fn(f32) -> Command + 'static,
) -> View<'a> {
    let theme = *ctx.theme();
    grana_ui::widget::slider_readout(&theme, label, value, range.clone(), logarithmic, readout)
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
    let detail_style = TextStyle::new(theme.font(Level::Callout), palette.text_tertiary)
        .wrapping()
        .aligned(Align::Center);
    let detail_layout = ctx.layout_text(detail, detail_style, (width - 56.0).max(80.0));
    let height = 24.0 + 48.0 + 12.0 + 20.0 + 6.0 + detail_layout.size.y + 24.0;
    let title = title.to_string();
    let detail = detail.to_string();
    let detail_height = detail_layout.size.y;
    let corners = Corners::all(theme.metrics.card_radius);
    grana_ui::canvas(Length::Fill, height, move |painter, info| {
        let outer = info.rect;
        let rect = Rect::from_min_size(
            Vec2::new(outer.min.x + (outer.width() - width) * 0.5, outer.min.y),
            Vec2::new(width, height),
        );
        painter.outlined(
            rect,
            corners,
            grana_ui::Fill::Solid(palette.card),
            grana_ui::Border {
                width: 1.0,
                color: palette.material_border,
            },
            grana_ui::Shadow::NONE,
        );
        let art = Rect::from_center_size(
            Vec2::new(rect.center().x, rect.min.y + 24.0 + 24.0),
            Vec2::splat(48.0),
        );
        painter.outlined(
            art,
            Corners::all(14.0),
            grana_ui::Fill::Solid(palette.material_fill.amplify(0.8)),
            grana_ui::Border {
                width: 1.0,
                color: palette.material_border,
            },
            grana_ui::Shadow::NONE,
        );
        painter.rect(
            Rect::from_center_size(art.center(), Vec2::splat(34.0)),
            Corners::all(10.0),
            palette.accent.with_alpha(0.12),
        );
        icon::paint(
            symbol,
            painter,
            art.center(),
            20.0,
            palette.text_secondary,
            1.5,
        );
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(rect.min.x + 28.0, rect.min.y + 24.0 + 48.0 + 12.0),
                Vec2::new(rect.width() - 56.0, 20.0),
            ),
            &title,
            TextStyle::new(theme.font(Level::Headline), palette.text_primary)
                .aligned(Align::Center),
            Align::Center,
        );
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(
                    rect.min.x + 28.0,
                    rect.min.y + 24.0 + 48.0 + 12.0 + 20.0 + 6.0,
                ),
                Vec2::new(rect.width() - 56.0, detail_height),
            ),
            &detail,
            detail_style,
            Align::Center,
        );
    })
}

pub fn column_label(column: &grana_measure::Column) -> String {
    let raw = column.label();
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
    kind: grana_scene::ObjectKind,
    job: grana_scene::Job,
    progress: grana_scene::Progress,
    frames: u32,
) -> String {
    let unit = match job {
        grana_scene::Job::Detect(_) => match kind {
            grana_scene::ObjectKind::Spots => "blocks",
            grana_scene::ObjectKind::Surfaces => "tiles",
            grana_scene::ObjectKind::Filaments => "frames",
        },
        grana_scene::Job::Import(_) => "tiles",
        grana_scene::Job::Track(_)
        | grana_scene::Job::Recut(_)
        | grana_scene::Job::Relation { .. }
        | grana_scene::Job::Coloc
        | grana_scene::Job::ObjectColoc(_)
        | grana_scene::Job::Drift => "frames",
        grana_scene::Job::Calibrate => "beads",
    };
    let prefix = match job {
        grana_scene::Job::Detect(_) | grana_scene::Job::Import(_) if frames > 1 => {
            format!("Frame {} · ", progress.time + 1)
        }
        grana_scene::Job::Detect(_)
        | grana_scene::Job::Import(_)
        | grana_scene::Job::Recut(_)
        | grana_scene::Job::Track(_)
        | grana_scene::Job::Relation { .. }
        | grana_scene::Job::Coloc
        | grana_scene::Job::ObjectColoc(_)
        | grana_scene::Job::Drift
        | grana_scene::Job::Calibrate => String::new(),
    };
    if progress.total > 0 {
        format!("{prefix}{}/{} {unit}", progress.done, progress.total)
    } else {
        format!("{prefix}Preparing")
    }
}

pub fn scene_uv(rect: Rect, size: Vec2) -> [f32; 4] {
    [
        rect.min.x / size.x,
        rect.min.y / size.y,
        rect.max.x / size.x,
        rect.max.y / size.y,
    ]
}
