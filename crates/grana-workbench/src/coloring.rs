use grana_measure::Unit;
use grana_render::Palette;
use grana_scene::Coloring;
use grana_ui::{Color, Length, Ui};

use crate::widget;

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
    ui.row(grana_ui::Block::row().gaps(8.0), |ui| {
        widget::field_label(ui, "color by");
        let mut options = vec![(None, "uniform".to_string())];
        options.extend(
            measurements
                .iter()
                .map(|(metric, label)| (Some(*metric), label.clone())),
        );
        let current = coloring.metric();
        if let Some(chosen) = grana_ui::popover::combo(ui, "metric", 180.0, &options, Some(current))
        {
            let target = chosen;
            if target != current && coloring.set_metric(target) {
                if target.is_some() {
                    coloring.set_manual(None);
                }
                changed = true;
            }
        }
    });
    let Some(metric) = coloring.metric() else {
        ui.pop_id();
        return changed;
    };
    let (unit, decimals) = describe(metric);
    ui.row(grana_ui::Block::row().gaps(8.0), |ui| {
        let palettes = Palette::ALL
            .iter()
            .map(|palette| (*palette, palette.name().to_string()))
            .collect::<Vec<_>>();
        if let Some(chosen) =
            grana_ui::popover::combo(ui, "palette", 132.0, &palettes, Some(coloring.palette()))
            && chosen != coloring.palette()
        {
            coloring.set_palette(chosen);
            changed = true;
        }
        if let Some(reversed) = widget::checkbox(ui, "reverse", "reverse", coloring.reversed()) {
            coloring.set_reversed(reversed);
            changed = true;
        }
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
            ui.row(grana_ui::Block::row().gaps(6.0), |ui| {
                if let Some(value) = widget::drag_value(ui, "low", low, speed, decimals as usize) {
                    low = value;
                }
                if let Some(value) = widget::drag_value(ui, "high", high, speed, decimals as usize)
                {
                    high = value;
                }
                widget::field_label(ui, unit.suffix());
                if widget::button(ui, "auto", "Auto", grana_ui::Role::Quiet) {
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
            let text = format!(
                "auto {} – {}",
                value(minimum, unit, decimals),
                value(maximum, unit, decimals)
            );
            ui.row(grana_ui::Block::row().gaps(6.0), |ui| {
                widget::caption(ui, &text);
                if widget::button(ui, "fix", "Fix range", grana_ui::Role::Quiet) {
                    coloring.set_manual(Some((minimum, maximum)));
                    changed = true;
                }
            });
        }
        (None, None) => widget::weak(ui, "no measured values to map yet"),
    }
    let _ = Length::Auto;
    changed
}

pub fn value(value: f32, unit: Unit, decimals: u8) -> String {
    let suffix = unit.suffix();
    if suffix.is_empty() {
        format!("{:.*}", decimals as usize, value)
    } else {
        format!("{:.*} {}", decimals as usize, value, suffix)
    }
}
