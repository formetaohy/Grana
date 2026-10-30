use egui::Ui;
use grana_render::{Stop, TransferFunction};
use grana_volume::Intensity;

pub struct TransferEditor {
    stops: Vec<Stop>,
    dirty: bool,
}

impl TransferEditor {
    pub fn new() -> Self {
        Self {
            stops: TransferFunction::fluorescence().stops().to_vec(),
            dirty: true,
        }
    }

    pub fn transfer(&self) -> TransferFunction {
        TransferFunction::new(self.stops.clone())
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn ui(&mut self, ui: &mut Ui, intensity: Intensity) {
        ui.horizontal(|ui| {
            if ui.button("Fluorescence").clicked() {
                self.replace(TransferFunction::fluorescence());
            }
            if ui.button("Grayscale").clicked() {
                self.replace(TransferFunction::grayscale());
            }
        });
        self.preview(ui, intensity);

        let mut changed = false;
        let mut removed = None;
        for (index, stop) in self.stops.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let mut position = stop.position;
                if ui
                    .add(egui::Slider::new(&mut position, 0.0..=1.0).show_value(false))
                    .changed()
                {
                    stop.position = position;
                    changed = true;
                }
                ui.label(format!("{:>6.0}", intensity.value(stop.position)));
                let mut color = stop.color;
                if ui.color_edit_button_rgb(&mut color).changed() {
                    stop.color = color;
                    changed = true;
                }
                let mut opacity = stop.opacity;
                if ui
                    .add(egui::Slider::new(&mut opacity, 0.0..=1.0).show_value(false))
                    .changed()
                {
                    stop.opacity = opacity;
                    changed = true;
                }
                if ui.small_button("−").clicked() {
                    removed = Some(index);
                }
            });
        }

        if let Some(index) = removed
            && self.stops.len() > 1
        {
            self.stops.remove(index);
            changed = true;
        }
        if ui.button("Add stop").clicked() {
            let midpoint = self.midpoint();
            self.stops.push(Stop::new(midpoint, [0.9, 0.9, 0.9], 0.5));
            changed = true;
        }
        if changed {
            self.stops.sort_by(|left, right| {
                left.position
                    .partial_cmp(&right.position)
                    .expect("stop positions are finite")
            });
            self.dirty = true;
        }
    }

    fn replace(&mut self, transfer: TransferFunction) {
        self.stops = transfer.stops().to_vec();
        self.dirty = true;
    }

    fn midpoint(&self) -> f32 {
        let mut widest = 0.0;
        let mut midpoint = 0.5;
        for pair in self.stops.windows(2) {
            let span = pair[1].position - pair[0].position;
            if span > widest {
                widest = span;
                midpoint = 0.5 * (pair[0].position + pair[1].position);
            }
        }
        midpoint
    }

    fn preview(&self, ui: &mut Ui, intensity: Intensity) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(18));

        let lut = self.transfer().lut(128);
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

        let marker = rect.left() + rect.width() * intensity.unit(intensity.max());
        painter.line_segment(
            [
                egui::pos2(marker, rect.top()),
                egui::pos2(marker, rect.bottom()),
            ],
            egui::Stroke::new(1.0, egui::Color32::from_gray(90)),
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
