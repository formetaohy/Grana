use egui::{Color32, RichText, Ui};
use grana_measure::Unit;
use grana_render::Palette;
use grana_scene::Coloring;

pub fn edit<M: Copy + PartialEq>(
    ui: &mut Ui,
    id: egui::Id,
    coloring: &mut Coloring<M>,
    measurements: &[(M, String)],
    extent: Option<(f32, f32)>,
    describe: impl Fn(M) -> (Unit, u8),
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Color by").small().weak());
        let current = coloring.metric();
        let selected = current
            .and_then(|metric| {
                measurements
                    .iter()
                    .find(|(candidate, _)| *candidate == metric)
                    .map(|(_, label)| label.clone())
            })
            .unwrap_or_else(|| "uniform".to_string());
        egui::ComboBox::from_id_salt(id.with("measurement"))
            .selected_text(selected)
            .width(150.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "uniform").clicked()
                    && coloring.set_metric(None)
                {
                    changed = true;
                }
                for (metric, label) in measurements {
                    if ui
                        .selectable_label(current == Some(*metric), label)
                        .clicked()
                        && coloring.set_metric(Some(*metric))
                    {
                        coloring.set_manual(None);
                        changed = true;
                    }
                }
            });
    });
    let Some(metric) = coloring.metric() else {
        return changed;
    };
    let (unit, decimals) = describe(metric);
    ui.horizontal(|ui| {
        let mut palette = coloring.palette();
        egui::ComboBox::from_id_salt(id.with("palette"))
            .selected_text(palette.name())
            .width(112.0)
            .show_ui(ui, |ui| {
                for candidate in Palette::ALL {
                    ui.selectable_value(&mut palette, candidate, candidate.name());
                }
            });
        if palette != coloring.palette() {
            coloring.set_palette(palette);
            changed = true;
        }
        let mut reversed = coloring.reversed();
        if ui.checkbox(&mut reversed, "reverse").changed() {
            coloring.set_reversed(reversed);
            changed = true;
        }
    });
    preview(ui, coloring.palette(), coloring.reversed());
    changed |= range(ui, coloring, extent, unit, decimals);
    changed
}

fn range<M: Copy + PartialEq>(
    ui: &mut Ui,
    coloring: &mut Coloring<M>,
    extent: Option<(f32, f32)>,
    unit: Unit,
    decimals: u8,
) -> bool {
    let mut changed = false;
    match (coloring.manual(), extent) {
        (Some((minimum, maximum)), _) => {
            let mut low = minimum;
            let mut high = maximum;
            let speed = ((maximum - minimum).abs() / 100.0).max(1.0e-6);
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut low)
                        .speed(speed)
                        .max_decimals(decimals as usize),
                );
                ui.add(
                    egui::DragValue::new(&mut high)
                        .speed(speed)
                        .max_decimals(decimals as usize),
                );
                ui.label(RichText::new(unit.suffix()).small().weak());
                if ui.small_button("Auto").clicked() {
                    coloring.set_manual(None);
                    changed = true;
                }
            });
            if low != minimum || high != maximum {
                coloring.set_manual(Some((low, high)));
                changed = true;
            }
        }
        (None, Some((minimum, maximum))) => {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "auto {} – {}",
                        value(minimum, unit, decimals),
                        value(maximum, unit, decimals)
                    ))
                    .small()
                    .weak(),
                );
                if ui.small_button("Fix range").clicked() {
                    coloring.set_manual(Some((minimum, maximum)));
                    changed = true;
                }
            });
        }
        (None, None) => {
            ui.label(
                RichText::new("no measured values to map yet")
                    .small()
                    .weak(),
            );
        }
    }
    changed
}

fn preview(ui: &mut Ui, palette: Palette, reversed: bool) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), egui::Sense::hover());
    let samples = 128;
    let step = rect.width() / samples as f32;
    for index in 0..samples {
        let position = index as f32 / (samples - 1) as f32;
        let position = if reversed { 1.0 - position } else { position };
        let [red, green, blue] = palette.sample(position);
        let color = Color32::from_rgb(to_byte(red), to_byte(green), to_byte(blue));
        let left = rect.left() + step * index as f32;
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(left, rect.top()),
                egui::pos2(left + step + 0.5, rect.bottom()),
            ),
            0.0,
            color,
        );
    }
}

pub fn swatch(ui: &mut Ui, palette: Palette, reversed: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 12.0), egui::Sense::hover());
    let samples = 16;
    let step = rect.width() / samples as f32;
    for index in 0..samples {
        let position = index as f32 / (samples - 1) as f32;
        let position = if reversed { 1.0 - position } else { position };
        let [red, green, blue] = palette.sample(position);
        let left = rect.left() + step * index as f32;
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(left, rect.top()),
                egui::pos2(left + step + 0.5, rect.bottom()),
            ),
            1.0,
            Color32::from_rgb(to_byte(red), to_byte(green), to_byte(blue)),
        );
    }
}

pub fn value(value: f32, unit: Unit, decimals: u8) -> String {
    let suffix = unit.suffix();
    if suffix.is_empty() {
        format!("{:.*}", decimals as usize, value)
    } else {
        format!("{:.*} {}", decimals as usize, value, suffix)
    }
}

fn to_byte(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}
