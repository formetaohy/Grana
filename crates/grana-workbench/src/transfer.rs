use grana_render::{Stop, TransferFunction};
use grana_ui::{Block, Color, Length, Role, Ui};
use grana_volume::Intensity;

use crate::widget;

pub fn edit(
    ui: &mut Ui,
    salt: &str,
    transfer: &mut TransferFunction,
    tint: [f32; 3],
    intensity: Intensity,
) {
    ui.push_id(salt);
    let mut stops = transfer.stops().to_vec();
    let mut changed = false;
    ui.row(
        Block::row().gaps(6.0).align(grana_ui::Align::Center),
        |ui| {
            widget::field_label(ui, "transfer");
            if widget::button(ui, "tinted", "Tinted", Role::Quiet) {
                stops = TransferFunction::tinted(tint).stops().to_vec();
                changed = true;
            }
            if widget::button(ui, "gray", "Grayscale", Role::Quiet) {
                stops = TransferFunction::grayscale().stops().to_vec();
                changed = true;
            }
        },
    );
    let lut = TransferFunction::new(stops.clone()).lut(96);
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
    widget::gradient(ui, &colors, 22.0);
    let mut removed = None;
    for (index, stop) in stops.iter_mut().enumerate() {
        ui.push_id(&format!("stop.{index}"));
        let mut position = stop.position();
        ui.row(
            Block::row().gaps(6.0).align(grana_ui::Align::Center),
            |ui| {
                if let Some(value) = widget::slider_sized(
                    ui,
                    "position",
                    None,
                    position,
                    0.0..=1.0,
                    false,
                    Length::Fixed(72.0),
                ) {
                    position = value;
                    stop.set_position(value);
                    changed = true;
                }
                let text = format!("{:>6.0}", intensity.value(stop.position()));
                widget::caption(ui, &text);
                let base = stop.color();
                if let Some(color) = grana_ui::popover::color_field(ui, "color", base, 18.0) {
                    stop.set_color(color);
                    changed = true;
                }
                if let Some(value) = widget::slider_sized(
                    ui,
                    "opacity",
                    None,
                    stop.opacity(),
                    0.0..=1.0,
                    false,
                    Length::Fill,
                ) {
                    stop.set_opacity(value);
                    changed = true;
                }
                if widget::icon_button(
                    ui,
                    "remove",
                    grana_ui::Icon::Minus,
                    18.0,
                    Role::Quiet,
                    "Remove stop",
                ) {
                    removed = Some(index);
                }
            },
        );
        ui.pop_id();
    }
    if let Some(index) = removed
        && stops.len() > 1
    {
        stops.remove(index);
        changed = true;
    }
    ui.row(
        Block::row().gaps(6.0).align(grana_ui::Align::Center),
        |ui| {
            if widget::button(ui, "add", "Add stop", Role::Quiet) {
                stops.push(Stop::new(midpoint(&stops), tint, 0.5));
                changed = true;
            }
        },
    );
    if changed {
        *transfer = TransferFunction::new(stops);
    }
    ui.pop_id();
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

fn linear_to_srgb(channel: f32) -> f32 {
    if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}
