use super::*;

const CHANNEL_ROW: f32 = 38.0;
const SWITCH_TRAIL: f32 = 10.0;

pub(super) fn channels<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let scene = state.scene;
    let count = scene.channels.len();
    let enabled = scene
        .channels
        .iter()
        .filter(|channel| channel.enabled)
        .count();
    let language = ctx.language();
    let detail = format!("{enabled} / {count}");
    let header = view::section_header(
        ctx,
        grana_i18n::inspector::channels(language),
        Some(&detail),
    );
    let mut rows = Vec::with_capacity(count);
    for index in 0..count {
        rows.push(channel_row(ctx, state, index, count));
    }
    let group_gap = ctx.theme().metrics.group_gap;
    let list = grana_ui::column().gap(group_gap).children(rows);
    grana_ui::column().gap(4.0).children([header, list])
}

pub(super) fn channel_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
    count: usize,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let metrics = theme.metrics;
    let scene = state.scene;
    let name = scene.dataset.channels()[index].name().to_string();
    let tint = Color::from_rgb_f32(scene.dataset.channels()[index].tint());
    let enabled = scene.channels[index].enabled;
    let opacity = scene.channels[index].opacity;
    let active = scene.selected == index;
    let font = if active {
        theme.font(Level::Title)
    } else {
        theme.font(Level::Body).medium()
    };
    let mono = theme.font(Level::Monospace);
    let switch_left = metrics.switch_width + 12.0;
    let width = panel_width() - switch_left;
    let corners =
        Corners::group_vertical(index, count, metrics.corner_lg, metrics.corner_xs, active);
    let canvas = grana_ui::canvas(Length::Fixed(width), CHANNEL_ROW)
        .view(
            (
                name.clone(),
                tint,
                enabled,
                opacity.to_bits(),
                active,
                corners,
            ),
            move |_, painter, info| {
                let rect = info.rect;
                let full =
                    Rect::from_min_max(rect.min, Vec2::new(rect.max.x + switch_left, rect.max.y));
                let fill = if active {
                    palette
                        .secondary_container
                        .lerp(palette.on_secondary_container, 0.06 * info.hover)
                } else {
                    palette
                        .surface_container
                        .lerp(palette.surface_container_high, info.hover)
                };
                painter.rect(full, corners, fill);
                let dot = Rect::from_center_size(
                    Vec2::new(rect.min.x + 15.0, rect.center().y),
                    Vec2::splat(10.0),
                );
                if active {
                    painter.ring(
                        dot.center(),
                        7.8,
                        1.5,
                        0.0,
                        std::f32::consts::TAU,
                        tint.with_alpha(if enabled { 0.65 } else { 0.25 }),
                    );
                }
                painter.rect(
                    dot,
                    Corners::capsule(),
                    tint.with_alpha(if enabled { 1.0 } else { 0.30 }),
                );
                let label_right = if enabled && opacity < 0.995 {
                    let badge = Rect::from_min_max(
                        Vec2::new(rect.max.x + switch_left - 96.0, rect.center().y - 9.0),
                        Vec2::new(rect.max.x + switch_left - 58.0, rect.center().y + 9.0),
                    );
                    painter.rect(
                        badge,
                        Corners::capsule(),
                        if active {
                            palette.on_secondary_container.with_alpha(0.12)
                        } else {
                            palette.surface_container_highest
                        },
                    );
                    painter.text_in(
                        badge,
                        &format!("{:.0}%", opacity * 100.0),
                        TextStyle::new(
                            mono,
                            if active {
                                palette.on_secondary_container
                            } else {
                                palette.on_surface_variant
                            },
                        )
                        .aligned(Align::Center),
                        Align::Center,
                    );
                    badge.min.x - 6.0
                } else {
                    rect.max.x - 6.0
                };
                let color = if !enabled {
                    palette.outline
                } else if active {
                    palette.on_secondary_container
                } else {
                    palette.on_surface
                };
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(rect.min.x + 30.0, rect.min.y),
                        Vec2::new(label_right.max(rect.min.x + 30.0), rect.max.y),
                    ),
                    &name,
                    TextStyle::new(font, color).truncating(),
                    Align::Center,
                );
            },
        )
        .key(Key::index("channel.pick", index))
        .press(Message::Command(Command::SelectChannel(index)));
    let switch = grana_ui::widget::toggle(&theme, enabled)
        .key(Key::index("channel.enabled", index))
        .on_toggle(move |value| {
            Message::Command(Command::SetChannelEnabled {
                channel: index,
                enabled: value,
            })
        });
    let gap = grana_ui::canvas(
        Length::Fixed(switch_left - metrics.switch_width - SWITCH_TRAIL),
        0.0,
    )
    .view_static(|_painter, _info| {});
    grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(CHANNEL_ROW))
        .align(Align::Center)
        .padding(Edges {
            right: SWITCH_TRAIL,
            ..Edges::ZERO
        })
        .children([canvas, gap, centered(CHANNEL_ROW, switch)])
}
