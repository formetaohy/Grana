use grana_display::{Stop, TransferFunction, linear_to_srgb};
use grana_session::Command;
use grana_ui::{Align, BuildContext, Color, Element, Icon, Length, Level, Role, TextStyle};

use crate::message::Message;

pub fn edit<'a>(
    ctx: &mut BuildContext<'_, Message>,
    salt: &str,
    channel: usize,
    transfer: &TransferFunction,
    tint: [f32; 3],
    window: grana_volume::Intensity,
    width: f32,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let caption_style = TextStyle::new(theme.font(Level::Monospace), palette.on_surface_variant);
    let lut = transfer.lut(96);
    let colors = lut
        .iter()
        .map(|[red, green, blue, opacity]| {
            Color::rgba(
                linear_to_srgb(*red),
                linear_to_srgb(*green),
                linear_to_srgb(*blue),
                *opacity,
            )
        })
        .collect::<Vec<_>>();
    let stops = transfer.stops().to_vec();
    let set = move |next: Vec<Stop>| {
        Message::Command(Command::SetTransfer {
            channel,
            transfer: TransferFunction::new(next),
        })
    };
    let mut children = Vec::new();
    let control = theme.metrics.control_height;
    children.push(grana_ui::widget::gradient_bar(&colors, 14.0));
    for (index, stop) in stops.iter().enumerate() {
        let text = format!("{:>6.0}", window.value(stop.position()));
        let caption = ctx.measure(&text, caption_style).x.ceil() + 2.0;
        let position_width = (width - caption - 116.0).max(40.0);
        let moved = {
            let base = stops.clone();
            move |value: f32| {
                let mut next = base.clone();
                next[index].set_position(value);
                set(next)
            }
        };
        let faded = {
            let base = stops.clone();
            move |value: f32| {
                let mut next = base.clone();
                next[index].set_opacity(value);
                set(next)
            }
        };
        let removed = {
            let mut next = stops.clone();
            if next.len() > 1 {
                next.remove(index);
            }
            set(next)
        };
        let anchor = grana_ui::Key::new(format!("{salt}.stop.{index}.color"));
        let recolor = {
            let base = stops.clone();
            move |color: [f32; 3]| {
                let mut next = base.clone();
                next[index].set_color(color);
                set(next)
            }
        };
        let swatch = grana_ui::widget::color_swatch(stop.color(), 18.0).key(anchor);
        let picker = grana_ui::widget::color_picker(stop.color()).on_color(recolor);
        let popup = grana_ui::popover::popup(anchor, 192.0, [picker]);
        children.push(
            grana_ui::row().gap(6.0).align(Align::Center).children([
                crate::inspector::centered(
                    control,
                    grana_ui::canvas(Length::Fixed(caption), 18.0).view(
                        text.clone(),
                        move |_, painter, info| {
                            painter.text_in(info.rect, &text, caption_style, Align::Center);
                        },
                    ),
                ),
                crate::inspector::centered(control, swatch),
                popup,
                grana_ui::widget::slider_sized(
                    &theme,
                    None,
                    stop.position(),
                    0.0..=1.0,
                    false,
                    Length::Fixed(position_width),
                )
                .named(&format!("{salt}.stop.{index}.position"))
                .on_slide(moved),
                grana_ui::widget::slider_sized(
                    &theme,
                    None,
                    stop.opacity(),
                    0.0..=1.0,
                    false,
                    Length::Fixed(54.0),
                )
                .named(&format!("{salt}.stop.{index}.opacity"))
                .on_slide(faded),
                crate::inspector::centered(
                    control,
                    grana_ui::widget::icon_button(
                        Icon::Minus,
                        22.0,
                        Role::Standard,
                        grana_i18n::inspector::remove_stop(language),
                    )
                    .named(&format!("{salt}.stop.{index}.remove"))
                    .press(removed),
                ),
            ]),
        );
    }
    children.push(
        crate::view::button_with(
            ctx,
            grana_i18n::inspector::add_stop(language),
            Role::SurfaceHigh,
            Length::Fill,
        )
        .press({
            let mut next = stops.clone();
            next.push(Stop::new(midpoint(&stops), tint, 0.5));
            set(next)
        }),
    );
    children
}

fn midpoint(stops: &[Stop]) -> f32 {
    let mut widest = 0.0;
    let mut midpoint = 0.5;
    for pair in stops.windows(2) {
        let span = pair[1].position() - pair[0].position();
        if span > widest {
            widest = span;
            midpoint = 0.5 * (pair[0].position() + pair[1].position());
        }
    }
    midpoint
}
