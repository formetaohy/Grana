use grana_display::Palette;
use grana_measure::Unit;
use grana_scene::Coloring;
use grana_ui::{Align, Block, Color, Role, Ui};

use crate::panel::{form_combo, form_row};
use crate::widget;
use grana_figure::value;

pub fn edit<M: Copy + PartialEq>(
    ui: &mut Ui,
    salt: &str,
    coloring: &mut Coloring<M>,
    measurements: &[(M, String)],
    extent: Option<(f32, f32)>,
    describe: impl Fn(M) -> (Unit, u8),
) -> bool {
    let mut changed = false;
    ui.push_id(salt);
    let mut options = vec![(None, "Uniform".to_string())];
    options.extend(
        measurements
            .iter()
            .map(|(metric, label)| (Some(*metric), label.clone())),
    );
    let current = coloring.metric();
    if let Some(chosen) = form_combo(ui, "metric", "Color by", &options, Some(current))
        && chosen != current
        && coloring.set_metric(chosen)
    {
        if chosen.is_some() {
            coloring.set_manual(None);
        }
        changed = true;
    }
    let Some(metric) = coloring.metric() else {
        ui.pop_id();
        return changed;
    };
    let (unit, decimals) = describe(metric);
    form_row(ui, "Palette", |ui| {
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            let palettes = Palette::ALL
                .iter()
                .map(|palette| (*palette, palette.name().to_string()))
                .collect::<Vec<_>>();
            let combo_w = (ui.available().x - 64.0).max(88.0);
            if let Some(chosen) = grana_ui::popover::combo(
                ui,
                "palette",
                combo_w,
                &palettes,
                Some(coloring.palette()),
            ) && chosen != coloring.palette()
            {
                coloring.set_palette(chosen);
                changed = true;
            }
            if let Some(reversed) = widget::checkbox(ui, "reverse", "Rev", coloring.reversed()) {
                coloring.set_reversed(reversed);
                changed = true;
            }
        });
    });
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
    widget::gradient(ui, &colors, 10.0);
    changed |= range(ui, coloring, extent, unit, decimals);
    ui.pop_id();
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
            let speed = ((maximum - minimum).abs() / 100.0).max(1.0e-6);
            let mut low = minimum;
            let mut high = maximum;
            form_row(ui, "Range", |ui| {
                ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                    if let Some(value) =
                        widget::drag_value(ui, "low", low, speed, decimals as usize)
                    {
                        low = value;
                    }
                    if let Some(value) =
                        widget::drag_value(ui, "high", high, speed, decimals as usize)
                    {
                        high = value;
                    }
                    if widget::button(ui, "auto", "Auto", Role::Quiet) {
                        coloring.set_manual(None);
                        changed = true;
                    }
                });
            });
            if low != minimum || high != maximum {
                coloring.set_manual(Some((low, high)));
                changed = true;
            }
        }
        (None, Some((minimum, maximum))) => {
            let text = format!(
                "Auto: {} – {}",
                value(minimum, unit, decimals),
                value(maximum, unit, decimals)
            );
            ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
                widget::caption(ui, &text);
                ui.space((ui.available().x - 76.0).max(0.0));
                if widget::button(ui, "fix", "Fix Range", Role::Quiet) {
                    coloring.set_manual(Some((minimum, maximum)));
                    changed = true;
                }
            });
        }
        (None, None) => widget::weak(ui, "No measured values to map yet."),
    }
    changed
}
