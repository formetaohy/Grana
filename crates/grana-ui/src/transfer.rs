use egui::Ui;
use grana_render::{Stop, TransferFunction};
use grana_volume::Intensity;

pub fn edit(ui: &mut Ui, transfer: &mut TransferFunction, tint: [f32; 3], intensity: Intensity) {
    let mut stops = transfer.stops().to_vec();
    ui.horizontal(|ui| {
        if ui.button("Tinted").clicked() {
            stops = TransferFunction::tinted(tint).stops().to_vec();
        }
        if ui.button("Grayscale").clicked() {
            stops = TransferFunction::grayscale().stops().to_vec();
        }
    });
    preview(ui, &stops);

    let mut changed = false;
    let mut removed = None;
    for (index, stop) in stops.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let mut position = stop.position();
            if ui
                .add(egui::Slider::new(&mut position, 0.0..=1.0).show_value(false))
                .changed()
            {
                stop.set_position(position);
                changed = true;
            }
            ui.label(format!("{:>6.0}", intensity.value(stop.position())));
            let mut color = stop.color();
            if ui.color_edit_button_rgb(&mut color).changed() {
                stop.set_color(color);
                changed = true;
            }
            let mut opacity = stop.opacity();
            if ui
                .add(egui::Slider::new(&mut opacity, 0.0..=1.0).show_value(false))
                .changed()
            {
                stop.set_opacity(opacity);
                changed = true;
            }
            if ui.small_button("−").clicked() {
                removed = Some(index);
            }
        });
    }

    if let Some(index) = removed
        && stops.len() > 1
    {
        stops.remove(index);
        changed = true;
    }
    if ui.button("Add stop").clicked() {
        stops.push(Stop::new(midpoint(&stops), tint, 0.5));
        changed = true;
    }
    if changed {
        *transfer = TransferFunction::new(stops);
    }
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

fn preview(ui: &mut Ui, stops: &[Stop]) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(18));
    let transfer = TransferFunction::new(stops.to_vec());
    let lut = transfer.lut(160);
    let segments = lut.len();
    for (index, [red, green, blue, opacity]) in lut.into_iter().enumerate() {
        let start = rect.left() + rect.width() * index as f32 / segments as f32;
        let end = rect.left() + rect.width() * (index + 1) as f32 / segments as f32;
        let color = egui::Color32::from_rgba_unmultiplied(
            to_byte(linear_to_srgb(red)),
            to_byte(linear_to_srgb(green)),
            to_byte(linear_to_srgb(blue)),
            to_byte(opacity),
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(start, rect.top()),
                egui::pos2(end + 0.5, rect.bottom()),
            ),
            0.0,
            color,
        );
    }
}

fn linear_to_srgb(channel: f32) -> f32 {
    if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

fn to_byte(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}
