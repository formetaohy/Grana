use grana_display::{Stop, TransferFunction, linear_to_srgb};
use grana_ui::{Align, Block, Color, Icon, Length, Role, Ui};
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
    widget::gradient(ui, &colors, 14.0);
    let mut removed = None;
    for (index, stop) in stops.iter_mut().enumerate() {
        ui.push_id(&format!("stop.{index}"));
        let mut position = stop.position();
        ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
            let text = format!("{:>6.0}", intensity.value(stop.position()));
            widget::caption(ui, &text);
            if let Some(color) = grana_ui::popover::color_field(ui, "color", stop.color(), 16.0) {
                stop.set_color(color);
                changed = true;
            }
            if let Some(value) = widget::slider_sized(
                ui,
                "position",
                None,
                position,
                0.0..=1.0,
                false,
                Length::Fill,
            ) {
                position = value;
                stop.set_position(value);
                changed = true;
            }
            if let Some(value) = widget::slider_sized(
                ui,
                "opacity",
                None,
                stop.opacity(),
                0.0..=1.0,
                false,
                Length::Fixed(52.0),
            ) {
                stop.set_opacity(value);
                changed = true;
            }
            if widget::icon_button(ui, "remove", Icon::Minus, 18.0, Role::Quiet, "Remove stop") {
                removed = Some(index);
            }
        });
        ui.pop_id();
    }
    if let Some(index) = removed
        && stops.len() > 1
    {
        stops.remove(index);
        changed = true;
    }
    if widget::button_with(ui, "add", "Add Stop", Role::Control, Length::Fill).clicked {
        stops.push(Stop::new(midpoint(&stops), tint, 0.5));
        changed = true;
    }
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
