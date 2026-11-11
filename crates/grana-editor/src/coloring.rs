use grana_display::{Coloring, Extent, Palette};
use grana_scene::{ObjectColor, ObjectSetId};
use grana_session::Command;
use grana_ui::{Align, BuildContext, Color, Element, Length, Level, Role, TextStyle};

use crate::message::Message;

pub fn edit<'a>(
    ctx: &mut BuildContext<'_, Message>,
    salt: &str,
    set: ObjectSetId,
    coloring: &Coloring<ObjectColor>,
    measurements: &[(ObjectColor, String)],
    extent: Option<Extent>,
    width: f32,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
    let language = ctx.language();
    let base = coloring.clone();
    let mut children = Vec::new();
    let mut options = vec![(None, grana_i18n::inspector::uniform(language).to_string())];
    options.extend(
        measurements
            .iter()
            .map(|(metric, label)| (Some(*metric), label.clone())),
    );
    let current = coloring.metric();
    children.push(crate::inspector::form_combo(
        ctx,
        &format!("{salt}.metric"),
        grana_i18n::inspector::color_by(language),
        &options,
        Some(current),
        width - crate::panel::FORM_LABEL,
        {
            let base = base.clone();
            move |chosen: Option<ObjectColor>| {
                if chosen == current {
                    return Message::Noop;
                }
                let mut next = base.clone();
                next.set_metric(chosen);
                if chosen.is_some() {
                    next.set_manual(None);
                }
                apply(set, next)
            }
        },
    ));
    let Some(metric) = current else {
        return children;
    };
    let palettes = Palette::ALL
        .iter()
        .map(|palette| (*palette, palette.name().to_string()))
        .collect::<Vec<_>>();
    let combo_width = (width - crate::panel::FORM_LABEL - 64.0).max(88.0);
    let palette_combo = crate::inspector::combo(
        ctx,
        &format!("{salt}.palette"),
        Length::Fixed(combo_width),
        combo_width,
        &palettes,
        Some(coloring.palette()),
        {
            let base = base.clone();
            move |chosen: Palette| {
                let mut next = base.clone();
                next.set_palette(chosen);
                apply(set, next)
            }
        },
    );
    let reverse = grana_ui::widget::checkbox(
        &theme,
        grana_i18n::inspector::reverse(language),
        coloring.reversed(),
    )
    .named(&format!("{salt}.reverse"))
    .on_toggle({
        let base = base.clone();
        move |value| {
            let mut next = base.clone();
            next.set_reversed(value);
            apply(set, next)
        }
    });
    let palette_row = grana_ui::row()
        .gap(8.0)
        .align(Align::Center)
        .children([palette_combo, reverse]);
    children.push(crate::view::form_row(
        ctx,
        grana_i18n::inspector::palette(language),
        palette_row,
    ));
    let samples = 64;
    let colors = (0..samples)
        .map(|index| {
            let position = index as f32 / (samples - 1) as f32;
            let position = if coloring.reversed() {
                1.0 - position
            } else {
                position
            };
            let [red, green, blue] = coloring.palette().sample(position);
            Color::rgb(red, green, blue)
        })
        .collect::<Vec<_>>();
    children.push(grana_ui::widget::gradient_bar(&colors, 10.0));
    children.extend(range(ctx, salt, set, coloring, extent, metric, width));
    children
}

fn range<'a>(
    ctx: &mut BuildContext<'_, Message>,
    salt: &str,
    set: ObjectSetId,
    coloring: &Coloring<ObjectColor>,
    extent: Option<Extent>,
    metric: ObjectColor,
    width: f32,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let base = coloring.clone();
    let unit = metric.unit();
    let decimals = metric.decimals();
    let mut children = Vec::new();
    match (coloring.manual(), extent) {
        (Some((minimum, maximum)), _) => {
            let speed = ((maximum - minimum).abs() / 100.0).max(1.0e-6);
            let low = grana_ui::widget::drag_value(&theme, minimum, decimals as usize)
                .named(&format!("{salt}.range.low"))
                .on_drag_value(speed, {
                    let base = base.clone();
                    move |value| {
                        let mut next = base.clone();
                        next.set_manual(Some((value, maximum)));
                        apply(set, next)
                    }
                });
            let high = grana_ui::widget::drag_value(&theme, maximum, decimals as usize)
                .named(&format!("{salt}.range.high"))
                .on_drag_value(speed, {
                    let base = base.clone();
                    move |value| {
                        let mut next = base.clone();
                        next.set_manual(Some((value, minimum)));
                        apply(set, next)
                    }
                });
            let auto = crate::view::button(ctx, grana_i18n::inspector::auto(language), Role::Quiet)
                .press({
                    let mut next = base.clone();
                    next.set_manual(None);
                    apply(set, next)
                });
            let row = grana_ui::row()
                .gap(6.0)
                .align(Align::Center)
                .children([low, high, auto]);
            children.push(crate::view::form_row(
                ctx,
                grana_i18n::inspector::range(language),
                row,
            ));
        }
        (None, Some((minimum, maximum))) => {
            let text = grana_i18n::inspector::auto_range(
                language,
                &grana_figure::value(minimum, unit, decimals),
                &grana_figure::value(maximum, unit, decimals),
            );
            let style = TextStyle::new(theme.font(Level::Caption), palette.text_tertiary);
            let inset = (theme.metrics.control_height * 0.5).round() - 3.0;
            let fix_label = grana_i18n::inspector::fix_range(language);
            let button =
                ctx.measure(
                    fix_label,
                    TextStyle::new(theme.button_font(), palette.text_primary),
                )
                .x + inset * 2.0;
            let caption = (width - crate::panel::FORM_LABEL - 6.0 - button - 2.0).max(40.0);
            let label = grana_ui::canvas(Length::Fixed(caption), 18.0, move |painter, info| {
                painter.text_in(info.rect, &text, style, Align::Center);
            });
            let fix = crate::view::button(ctx, fix_label, Role::Quiet).press({
                let mut next = base.clone();
                next.set_manual(Some((minimum, maximum)));
                apply(set, next)
            });
            children.push(grana_ui::row().gap(6.0).align(Align::Center).children([
                crate::inspector::centered(theme.metrics.control_height, label),
                fix,
            ]));
        }
        (None, None) => children.push(crate::view::weak(
            ctx,
            grana_i18n::inspector::no_measured(language),
        )),
    }
    children
}

fn apply(set: ObjectSetId, coloring: Coloring<ObjectColor>) -> Message {
    Message::Command(Command::SetObjectColoring { set, coloring })
}
