use glam::Vec2;
use grana_ui::{
    Align, BuildContext, Corners, Element, Icon, Key, Layer, Length, Level, Motion, Rect, Role,
    State, TextStyle, icon,
};

use crate::message::Message;
use crate::state::{Settings, Workspace};

const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 528.0;
const HEADER: f32 = 68.0;
const RAIL: f32 = 200.0;
const PADDING: f32 = 20.0;
const SECTION_ROW: f32 = 40.0;
const SECTION_GAP: f32 = 18.0;
const GUTTER: f32 = 48.0;

pub(crate) fn window_floor() -> Vec2 {
    Vec2::new(WIDTH + GUTTER, HEIGHT + GUTTER)
}

fn section_icon(index: usize) -> Icon {
    match index {
        0 => Icon::Sliders,
        _ => Icon::Info,
    }
}

fn section_name(language: grana_i18n::Language, index: usize) -> &'static str {
    match index {
        0 => grana_i18n::settings::appearance(language),
        _ => grana_i18n::settings::about(language),
    }
}

pub(crate) fn dialog_rect(full: Rect) -> Rect {
    let width = WIDTH.min((full.width() - GUTTER).max(320.0));
    let height = HEIGHT.min((full.height() - GUTTER).max(280.0));
    Rect::from_center_size(full.center(), Vec2::new(width, height))
}

fn compact(rect: Rect) -> bool {
    rect.width() < RAIL + 320.0
}

pub(crate) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    full: Rect,
    workspace: &Workspace,
) -> Vec<Element<'a, Message>> {
    if !workspace.settings.open {
        return Vec::new();
    }
    let theme = *ctx.theme();
    let palette = theme.palette;
    let appear = ctx.animate(
        Key::new("settings.appear").id(),
        1.0,
        Motion::spatial_default(),
    );
    let scrim = palette.scrim.with_alpha(palette.scrim.alpha * appear);
    let rect = dialog_rect(full);
    let surface = theme
        .surface(Role::Sheet, State::Rest)
        .with_corners(Corners::all(theme.metrics.corner_xxl));
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEADER));
    let mut children = vec![title_bar(ctx, header, rect)];
    if compact(rect) {
        let tabs = Rect::from_min_size(
            Vec2::new(rect.min.x + PADDING, header.max.y),
            Vec2::new(rect.width() - 2.0 * PADDING, 40.0),
        );
        let labels = (0..Settings::SECTIONS)
            .map(|index| section_name(ctx.language(), index))
            .collect::<Vec<_>>();
        children.push(
            crate::view::segmented_fill(
                ctx,
                "settings.tabs",
                &labels,
                workspace.settings.section,
                Message::SetSettingsSection,
            )
            .at(tabs),
        );
        let body = Rect::from_min_max(
            Vec2::new(rect.min.x + PADDING, tabs.max.y + 12.0),
            Vec2::new(rect.max.x - PADDING, rect.max.y - PADDING),
        );
        children.push(body_view(ctx, body, workspace, false));
    } else {
        let rail = Rect::from_min_max(
            Vec2::new(rect.min.x + PADDING, header.max.y),
            Vec2::new(rect.min.x + PADDING + RAIL, rect.max.y - PADDING),
        );
        let body = Rect::from_min_max(
            Vec2::new(rail.max.x + PADDING, header.max.y + 4.0),
            Vec2::new(rect.max.x - PADDING, rect.max.y - PADDING),
        );
        children.push(rail_view(ctx, rail, workspace.settings.section));
        children.push(body_view(ctx, body, workspace, true));
    }
    vec![
        grana_ui::canvas(Length::Fill, full.height())
            .view_static(move |painter, info| {
                painter.rect(info.rect, Corners::ZERO, scrim);
            })
            .named("settings.scrim")
            .at(full)
            .layer(Layer::Popup)
            .press(Message::ToggleSettings(false)),
        grana_ui::column()
            .at(rect)
            .named("settings.dialog")
            .background(surface)
            .layer(Layer::Popup)
            .children(children),
    ]
}

fn title_bar<'a>(
    ctx: &mut BuildContext<'_, Message>,
    header: Rect,
    dialog: Rect,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let label = grana_i18n::settings::title(ctx.language()).to_string();
    let style = TextStyle::new(theme.font(Level::Headline), palette.on_surface);
    let title_rect = Rect::from_min_max(
        Vec2::new(header.min.x + PADDING + 8.0, header.min.y),
        Vec2::new(header.max.x - PADDING - 44.0, header.max.y),
    );
    let title = grana_ui::canvas(Length::Fill, HEADER)
        .view(label, move |held: &String, painter, info| {
            painter.text_in(info.rect, held, style, Align::Center);
        })
        .named("settings.title")
        .at(title_rect);
    let close = crate::view::icon_button(
        ctx,
        "settings.close",
        Icon::Close,
        32.0,
        Role::Standard,
        grana_i18n::settings::close(ctx.language()),
        Message::ToggleSettings(false),
    )
    .at(Rect::from_center_size(
        Vec2::new(dialog.max.x - PADDING - 16.0, header.center().y),
        Vec2::splat(32.0),
    ));
    grana_ui::column()
        .at(header)
        .named("settings.header")
        .children([title, close])
}

fn rail_view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    rail: Rect,
    active: usize,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let rows_rect = Rect::from_min_size(
        rail.min,
        Vec2::new(
            rail.width(),
            (SECTION_ROW + 4.0) * Settings::SECTIONS as f32,
        ),
    );
    let language = ctx.language();
    let mut rows = Vec::new();
    for index in 0..Settings::SECTIONS {
        let selected = index == active;
        let symbol = section_icon(index);
        let label = section_name(language, index).to_string();
        let font = theme.font(Level::Label);
        rows.push(
            grana_ui::canvas(Length::Fill, SECTION_ROW)
                .view(
                    (label.clone(), selected),
                    move |(held, selected): &(String, bool), painter, info| {
                        let selected = *selected;
                        let engaged = info.hover.max(if selected { 1.0 } else { 0.0 });
                        let rest = Corners::capsule().resolved(info.rect.size());
                        let pressed =
                            Corners::all(theme.metrics.corner_md).resolved(info.rect.size());
                        if engaged > 0.01 {
                            painter.rect(
                                info.rect,
                                rest.lerp(pressed, info.press),
                                if selected {
                                    palette.secondary_container
                                } else {
                                    palette.on_surface.with_alpha(0.08 * engaged)
                                },
                            );
                        }
                        let tint = if selected {
                            palette.on_secondary_container
                        } else {
                            palette.on_surface_variant.lerp(palette.on_surface, engaged)
                        };
                        icon::paint(
                            symbol,
                            painter,
                            Vec2::new(info.rect.min.x + 18.0, info.rect.center().y),
                            14.0,
                            tint,
                            1.6,
                        );
                        painter.text_in(
                            Rect::from_min_max(
                                Vec2::new(info.rect.min.x + 34.0, info.rect.min.y),
                                Vec2::new(info.rect.max.x - 10.0, info.rect.max.y),
                            ),
                            held,
                            TextStyle::new(font, tint).truncating(),
                            Align::Center,
                        );
                    },
                )
                .key(Key::index("settings.section", index))
                .press(Message::SetSettingsSection(index))
                .cursor(grana_ui::Cursor::Pointing)
                .focus_ring(Corners::capsule()),
        );
    }
    grana_ui::column().at(rows_rect).gap(4.0).children(rows)
}

fn body_view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    body: Rect,
    workspace: &Workspace,
    ruled: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let mut content = Vec::new();
    match workspace.settings.section {
        0 => content.push(appearance(ctx, body.width(), workspace.language)),
        _ => content.push(about(ctx, body.width())),
    }
    let scroll = Element::new(grana_ui::Layout::column())
        .named("settings.body")
        .at(body)
        .scroll(grana_ui::Axis::Column, Length::Fill, None)
        .clipped()
        .children(content);
    if !ruled {
        return scroll;
    }
    let divider = grana_ui::canvas(Length::Fixed(1.0), body.height())
        .view_static(move |painter, info| {
            painter.rect(
                info.rect,
                Corners::ZERO,
                palette.outline_variant.with_alpha(0.7),
            );
        })
        .at(Rect::from_min_size(
            Vec2::new(body.min.x - PADDING * 0.5, body.min.y),
            Vec2::new(1.0, body.height()),
        ));
    grana_ui::column()
        .at(body)
        .named("settings.pane")
        .children([divider, scroll])
}

fn appearance<'a>(
    ctx: &mut BuildContext<'_, Message>,
    width: f32,
    language: grana_i18n::Language,
) -> Element<'a, Message> {
    let labels = grana_i18n::Language::ALL.map(grana_i18n::Language::label);
    let selected = grana_i18n::Language::ALL
        .iter()
        .position(|candidate| *candidate == language)
        .unwrap_or(0);
    let selector =
        crate::view::segmented_fill(ctx, "settings.language", &labels, selected, |index| {
            match grana_i18n::Language::ALL.get(index) {
                Some(language) => Message::SetLanguage(*language),
                None => Message::Noop,
            }
        });
    grana_ui::column().gap(SECTION_GAP).children([
        section_title(
            ctx,
            grana_i18n::settings::language(ctx.language()),
            grana_i18n::settings::language_detail(ctx.language()),
        ),
        grana_ui::column()
            .width(Length::Fixed(width))
            .children([selector]),
    ])
}

fn about<'a>(ctx: &mut BuildContext<'_, Message>, width: f32) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let brand = grana_ui::canvas(Length::Fixed(56.0), 56.0)
        .view_static(|painter, info| {
            grana_brand::paint(
                painter,
                Rect::from_center_size(info.rect.center(), Vec2::splat(48.0)),
            );
        })
        .named("settings.brand");
    let name = grana_ui::text(
        "Grana",
        TextStyle::new(theme.font(Level::Display), palette.on_surface),
    );
    let version = grana_ui::text(
        format!(
            "{} {}",
            grana_i18n::settings::version(language),
            env!("CARGO_PKG_VERSION")
        ),
        TextStyle::new(theme.font(Level::Monospace), palette.outline),
    );
    let row = grana_ui::row()
        .width(Length::Fixed(width))
        .gap(14.0)
        .align(Align::Center)
        .children([
            brand,
            grana_ui::column()
                .gap(2.0)
                .justify(Align::Center)
                .children([name, version]),
        ]);
    grana_ui::column().gap(SECTION_GAP).children([
        section_title(
            ctx,
            grana_i18n::settings::about(language),
            grana_i18n::settings::motion_detail(language),
        ),
        row,
    ])
}

fn section_title<'a>(
    ctx: &mut BuildContext<'_, Message>,
    title: &str,
    detail: &str,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let title_style = TextStyle::new(theme.font(Level::Title), palette.on_surface);
    let detail_style =
        TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant).wrapping();
    let title_text = grana_ui::text(title, title_style);
    let detail_text = grana_ui::text(detail, detail_style);
    grana_ui::column()
        .gap(2.0)
        .children([title_text, detail_text])
}
