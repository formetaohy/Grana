use glam::Vec2;
use grana_ui::{
    Align, BuildContext, Corners, Element, Fill, Icon, IconStyle, Key, Layer, Length, Level,
    Motion, Rect, State, Surface, TextStyle, icon,
};

use crate::message::Message;
use crate::state::Workspace;

const WIDTH: f32 = 368.0;
const HEIGHT: f32 = 228.0;
const PADDING: f32 = 20.0;
const HEADER: f32 = 34.0;
const LANGUAGE_CARD: f32 = 84.0;
const ABOUT_CARD: f32 = 54.0;
const GUTTER: f32 = 32.0;

pub(crate) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    full: Rect,
    workspace: &Workspace,
) -> Vec<Element<'a, Message>> {
    let open = workspace.settings.open;
    let target = if open { 1.0 } else { 0.0 };
    let spatial = ctx.animate(
        Key::new("settings.spatial").id(),
        target,
        Motion::spatial_default(),
    );
    let effect = ctx.animate(
        Key::new("settings.effect").id(),
        target,
        Motion::effect_default(),
    );
    if !open && effect <= 0.005 && spatial <= 0.005 {
        return Vec::new();
    }
    let theme = *ctx.theme();
    let palette = theme.palette;
    let alpha = effect.clamp(0.0, 1.0);
    let scale = (0.88 + 0.12 * spatial).max(0.5);
    let slide = (1.0 - spatial) * -18.0;
    let base_w = WIDTH.min((full.width() - GUTTER).max(280.0));
    let base_h = HEIGHT.min((full.height() - GUTTER).max(200.0));
    let rect = Rect::from_center_size(
        full.center() + Vec2::new(0.0, slide),
        Vec2::new(base_w * scale, base_h * scale),
    );
    let corners = Corners::all(40.0).lerp(
        Corners::all(theme.metrics.corner_xxl),
        spatial.clamp(0.0, 1.0),
    );
    let scrim = palette.scrim.with_alpha(palette.scrim.alpha * alpha);
    let mut surface = theme.surface(grana_ui::Role::Sheet, State::Rest);
    surface.fill = Fill::Solid(palette.surface_container_low.with_alpha(alpha));
    surface.corners = corners;

    let inner = rect.shrink(PADDING * scale);
    let header_rect = Rect::from_min_size(inner.min, Vec2::new(inner.width(), HEADER));
    let group_top = header_rect.max.y + 12.0 * scale;
    let lang_rect = Rect::from_min_size(
        Vec2::new(inner.min.x, group_top),
        Vec2::new(inner.width(), LANGUAGE_CARD),
    );
    let about_rect = Rect::from_min_size(
        Vec2::new(inner.min.x, lang_rect.max.y + theme.metrics.group_gap),
        Vec2::new(inner.width(), ABOUT_CARD),
    );

    let children = vec![
        title_bar(ctx, header_rect, alpha, open),
        language_block(ctx, lang_rect, workspace.language, alpha, open),
        about_block(ctx, about_rect, alpha, &workspace.update),
    ];

    let backdrop = grana_ui::canvas(Length::Fill, full.height())
        .view(alpha.to_bits(), move |_, painter, info| {
            painter.rect(info.rect, Corners::ZERO, scrim);
        })
        .named("settings.scrim")
        .at(full)
        .layer(Layer::Popup);
    let backdrop = if open {
        backdrop.press(Message::ToggleSettings(false))
    } else {
        backdrop
    };

    vec![
        backdrop,
        grana_ui::column()
            .at(rect)
            .named("settings.dialog")
            .background(surface)
            .clipped()
            .layer(Layer::Popup)
            .blocks_pointer()
            .modal()
            .children(children),
    ]
}

fn title_bar<'a>(
    ctx: &mut BuildContext<'_, Message>,
    header: Rect,
    alpha: f32,
    open: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let label = grana_i18n::settings::title(language).to_string();
    let style = TextStyle::new(
        theme.font(Level::Headline),
        palette.on_surface.with_alpha(alpha),
    );
    let title_rect = Rect::from_min_max(
        Vec2::new(header.min.x + 4.0, header.min.y),
        Vec2::new(header.max.x - 38.0, header.max.y),
    );
    let title = grana_ui::canvas(Length::Fill, HEADER)
        .view(
            (label, alpha.to_bits()),
            move |(held, _): &(String, u32), painter, info| {
                painter.text_in(info.rect, held, style, Align::Center);
            },
        )
        .named("settings.title")
        .at(title_rect);

    let close_rect = Rect::from_center_size(
        Vec2::new(header.max.x - 16.0, header.center().y),
        Vec2::splat(32.0),
    );
    let close = grana_ui::canvas(Length::Fixed(32.0), 32.0)
        .view(alpha.to_bits(), move |_, painter, info| {
            let corners = Corners::capsule()
                .resolved(info.rect.size())
                .lerp(Corners::all(theme.metrics.corner_sm), info.press);
            let fill = palette
                .surface_container_high
                .lerp(palette.surface_container_highest, info.hover)
                .lerp(palette.on_surface, 0.12 * info.press)
                .with_alpha(alpha);
            painter.rect(info.rect, corners, fill);
            icon::paint(
                Icon::Close,
                painter,
                info.rect.center(),
                16.0,
                palette
                    .on_surface_variant
                    .lerp(palette.on_surface, info.hover)
                    .with_alpha(alpha),
                IconStyle::REGULAR,
            );
        })
        .named("settings.close")
        .tooltip(grana_i18n::settings::close(language))
        .cursor(grana_ui::Cursor::Pointing)
        .focus_ring(Corners::capsule())
        .at(close_rect);
    let close = if open {
        close.press(Message::ToggleSettings(false))
    } else {
        close
    };

    grana_ui::column()
        .at(header)
        .named("settings.header")
        .children([title, close])
}

fn language_block<'a>(
    ctx: &mut BuildContext<'_, Message>,
    rect: Rect,
    language: grana_i18n::Language,
    alpha: f32,
    open: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let corners = Corners::group_vertical(
        0,
        2,
        theme.metrics.corner_lg,
        theme.metrics.corner_xs,
        false,
    );
    let surface = Surface::fill(palette.surface_container.with_alpha(alpha)).with_corners(corners);
    let title = grana_i18n::settings::language(language).to_string();
    let title_font = theme.font(Level::Title);
    let head_rect = Rect::from_min_size(
        Vec2::new(rect.min.x + 14.0, rect.min.y + 10.0),
        Vec2::new(rect.width() - 28.0, 22.0),
    );
    let head = grana_ui::canvas(Length::Fill, 22.0)
        .view(
            (title, alpha.to_bits()),
            move |(title, _): &(String, u32), painter, info| {
                icon::paint(
                    Icon::Globe,
                    painter,
                    Vec2::new(info.rect.min.x + 8.0, info.rect.center().y),
                    15.0,
                    palette.primary.with_alpha(alpha),
                    IconStyle::REGULAR,
                );
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(info.rect.min.x + 24.0, info.rect.min.y),
                        info.rect.max,
                    ),
                    title,
                    TextStyle::new(title_font, palette.on_surface.with_alpha(alpha)),
                    Align::Center,
                );
            },
        )
        .at(head_rect);

    let seg_rect = Rect::from_min_size(
        Vec2::new(rect.min.x + 14.0, rect.min.y + 40.0),
        Vec2::new(rect.width() - 28.0, 32.0),
    );
    let selected = grana_i18n::Language::ALL
        .iter()
        .position(|candidate| *candidate == language)
        .unwrap_or(0);
    let pill_t = ctx.animate(
        Key::new("settings.language.pill").id(),
        selected as f32,
        Motion::spatial_fast(),
    );
    let gap = theme.metrics.group_gap;
    let half = (seg_rect.width() - gap) * 0.5;
    let label_font = theme.font(Level::Label);
    let mut buttons = Vec::new();
    for (index, candidate) in grana_i18n::Language::ALL.into_iter().enumerate() {
        let cell = Rect::from_min_size(
            Vec2::new(seg_rect.min.x + index as f32 * (half + gap), seg_rect.min.y),
            Vec2::new(half, seg_rect.height()),
        );
        let active = index == selected;
        let cell_corners =
            Corners::group_horizontal(index, 2, Corners::CAPSULE, theme.metrics.corner_sm, active);
        let button = grana_ui::canvas(Length::Fixed(half), seg_rect.height())
            .view(
                (active, alpha.to_bits()),
                move |(active, _): &(bool, u32), painter, info| {
                    if !*active {
                        let fill = palette
                            .surface_container_high
                            .lerp(palette.surface_container_highest, info.hover)
                            .with_alpha(alpha);
                        let c = cell_corners
                            .resolved(info.rect.size())
                            .lerp(Corners::all(theme.metrics.corner_xs), info.press);
                        painter.rect(info.rect, c, fill);
                    }
                },
            )
            .key(Key::index("settings.language.option", index))
            .cursor(grana_ui::Cursor::Pointing)
            .focus_ring(Corners::capsule())
            .at(cell);
        let button = if open {
            button.press(Message::SetLanguage(candidate))
        } else {
            button
        };
        buttons.push(button);
    }

    let overlay = grana_ui::canvas(Length::Fixed(seg_rect.width()), seg_rect.height())
        .view(
            (selected, pill_t.to_bits(), alpha.to_bits()),
            move |_, painter, info| {
                let x = info.rect.min.x + pill_t.clamp(0.0, 1.0) * (half + gap);
                let pill = Rect::from_min_size(
                    Vec2::new(x, info.rect.min.y),
                    Vec2::new(half, info.rect.height()),
                );
                painter.rect(
                    pill,
                    Corners::capsule(),
                    palette.secondary_container.with_alpha(alpha),
                );
                for (index, candidate) in grana_i18n::Language::ALL.into_iter().enumerate() {
                    let cell = Rect::from_min_size(
                        Vec2::new(
                            info.rect.min.x + index as f32 * (half + gap),
                            info.rect.min.y,
                        ),
                        Vec2::new(half, info.rect.height()),
                    );
                    let color = if index == selected {
                        palette.on_secondary_container.with_alpha(alpha)
                    } else {
                        palette.on_surface_variant.with_alpha(alpha)
                    };
                    painter.text_in(
                        cell,
                        candidate.label(),
                        TextStyle::new(label_font, color).aligned(Align::Center),
                        Align::Center,
                    );
                }
            },
        )
        .named("settings.language")
        .at(seg_rect);

    let mut children = vec![head];
    children.extend(buttons);
    children.push(overlay);
    grana_ui::column()
        .at(rect)
        .background(surface)
        .children(children)
}

fn about_block<'a>(
    ctx: &mut BuildContext<'_, Message>,
    rect: Rect,
    alpha: f32,
    update: &crate::state::Update,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let noticed = update.phase.noticed();
    let corners = Corners::group_vertical(
        1,
        2,
        theme.metrics.corner_lg,
        theme.metrics.corner_xs,
        false,
    );
    let surface = Surface::fill(palette.surface_container.with_alpha(alpha)).with_corners(corners);
    let version = format!(
        "{} {}",
        grana_i18n::settings::version(language),
        env!("CARGO_PKG_VERSION")
    );
    let title_font = theme.font(Level::Title);
    let mono_font = theme.font(Level::Monospace);
    let content = grana_ui::canvas(Length::Fill, ABOUT_CARD)
        .view(
            (version, noticed, alpha.to_bits()),
            move |(version, noticed, _): &(String, bool, u32), painter, info| {
                let badge = Rect::from_center_size(
                    Vec2::new(info.rect.min.x + 28.0, info.rect.center().y),
                    Vec2::splat(30.0),
                );
                if alpha > 0.15 {
                    grana_brand::paint(painter, badge);
                }
                if *noticed {
                    painter.rect(
                        Rect::from_center_size(badge.max - Vec2::new(1.0, 1.0), Vec2::splat(9.0)),
                        Corners::capsule(),
                        palette.primary.with_alpha(alpha),
                    );
                }
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(info.rect.min.x + 52.0, info.rect.min.y),
                        Vec2::new(info.rect.max.x - 110.0, info.rect.max.y),
                    ),
                    "Grana",
                    TextStyle::new(title_font, palette.on_surface.with_alpha(alpha)),
                    Align::Center,
                );
                let pill_style =
                    TextStyle::new(mono_font, palette.on_secondary_container.with_alpha(alpha))
                        .aligned(Align::Center);
                let pill_w = painter.measure(version, pill_style).x + 18.0;
                let pill = Rect::from_min_size(
                    Vec2::new(info.rect.max.x - 14.0 - pill_w, info.rect.center().y - 11.0),
                    Vec2::new(pill_w, 22.0),
                );
                painter.rect(
                    pill,
                    Corners::capsule(),
                    palette.secondary_container.with_alpha(alpha),
                );
                painter.text_in(pill, version, pill_style, Align::Center);
            },
        )
        .named("settings.brand")
        .tooltip(grana_i18n::settings::open(language))
        .cursor(grana_ui::Cursor::Pointing)
        .at(rect);
    let content = if alpha > 0.5 {
        content.press(Message::Update(crate::message::UpdateAction::Open))
    } else {
        content
    };

    grana_ui::column()
        .at(rect)
        .background(surface)
        .children([content])
}
