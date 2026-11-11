use super::*;

const CHANNEL_ROW: f32 = 34.0;

const SWITCH_TRAIL: f32 = 4.0;

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
    let detail = grana_i18n::inspector::visible_channels(language, enabled, count);
    let title = view::card_title(
        ctx,
        Icon::Layers,
        grana_i18n::inspector::channels(language),
        Some(&detail),
    );
    let mut children = Vec::new();
    children.push(
        grana_ui::column()
            .gap(0.0)
            .children([title, grana_ui::spacer(2.0)]),
    );
    for index in 0..count {
        children.push(channel_row(ctx, state, index));
    }
    view::card(ctx, 3.0, children)
}

pub(super) fn channel_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
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
        theme.font(Level::Body).medium()
    } else {
        theme.font(Level::Body)
    };
    let mono = theme.font(Level::Monospace);
    let switch_left = metrics.switch_width + 10.0;
    let width = card_width() - switch_left;
    let canvas = grana_ui::canvas(Length::Fixed(width), CHANNEL_ROW, move |painter, info| {
        let rect = info.rect;
        let full = Rect::from_min_max(
            rect.min,
            Vec2::new(rect.max.x + switch_left + SWITCH_TRAIL, rect.max.y),
        );
        if active {
            painter.outlined(
                full,
                Corners::all(9.0),
                Fill::Solid(palette.accent.with_alpha(0.16)),
                Border {
                    width: 1.0,
                    color: palette.accent.with_alpha(0.34),
                },
                Shadow::NONE,
            );
        } else if info.hover > 0.01 {
            painter.rect(
                full,
                Corners::all(9.0),
                palette.material_hover.amplify(0.50 * info.hover),
            );
        }
        let dot = Rect::from_center_size(
            Vec2::new(rect.min.x + 12.0, rect.center().y),
            Vec2::splat(8.5),
        );
        if active {
            painter.ring(
                dot.center(),
                6.8,
                1.2,
                0.0,
                std::f32::consts::TAU,
                tint.with_alpha(if enabled { 0.48 } else { 0.20 }),
            );
        }
        painter.rect(
            dot,
            Corners::capsule(),
            tint.with_alpha(if enabled { 1.0 } else { 0.28 }),
        );
        let label_right = if enabled && opacity < 0.995 {
            let badge = Rect::from_min_max(
                Vec2::new(rect.max.x + switch_left - 42.0, rect.min.y),
                Vec2::new(rect.max.x + switch_left - 8.0, rect.max.y),
            );
            painter.text_in(
                badge,
                &format!("{:.0}%", opacity * 100.0),
                TextStyle::new(mono, palette.text_tertiary).aligned(Align::End),
                Align::Center,
            );
            badge.min.x - 4.0
        } else {
            rect.max.x + switch_left - 8.0
        };
        let color = if !enabled {
            palette.text_quaternary
        } else if active {
            palette.text_primary
        } else {
            palette.text_secondary
        };
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 25.0, rect.min.y),
                Vec2::new(label_right, rect.max.y),
            ),
            &name,
            TextStyle::new(font, color).truncating(),
            Align::Center,
        );
    })
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
        |_painter, _info| {},
    );
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
