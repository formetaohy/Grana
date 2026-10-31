use egui::{Align2, Color32, FontId, Rect, RichText, Stroke, Ui, pos2, vec2};
use grana_object::{COLOC_SCATTER, ColocPair, ColocSeries, ColocStatistics, ColocThresholds};
use grana_scene::{Job, Progress};

use crate::action::Action;
use crate::panel::{color_swatch, progress};
use crate::state::State;

const PLOT_MAX: f32 = 208.0;
const PLOT_MIN: f32 = 96.0;
const WARNING: Color32 = Color32::from_rgb(255, 176, 116);

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let running = progress(state.analysis, Job::Coloc);
    header(ui, state, actions, running);
    if state.scene.coloc.pair().is_none() {
        return;
    }
    if state.scene.coloc.series().is_none() {
        ui.label(
            RichText::new(if running.is_some() {
                "streaming both channels over every frame of the region"
            } else {
                "compute to stream both channels over every frame of the region"
            })
            .small()
            .weak(),
        );
        return;
    }
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            thresholds(ui, state);
            statistics(ui, state);
        });
        ui.add_space(16.0);
        scatter(ui, state);
    });
}

fn header(
    ui: &mut Ui,
    state: &mut State<'_>,
    actions: &mut Vec<Action>,
    running: Option<Progress>,
) {
    let Some(pair) = state.scene.coloc.pair() else {
        ui.label(
            RichText::new("the dataset needs two channels to colocalize")
                .small()
                .weak(),
        );
        return;
    };
    let names = state
        .scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let mut a = pair.a();
    let mut b = pair.b();
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("grana.coloc.a")
            .selected_text(&names[a])
            .show_ui(ui, |ui| {
                for (index, name) in names.iter().enumerate() {
                    if index != b {
                        ui.selectable_value(&mut a, index, name);
                    }
                }
            });
        egui::ComboBox::from_id_salt("grana.coloc.b")
            .selected_text(&names[b])
            .show_ui(ui, |ui| {
                for (index, name) in names.iter().enumerate() {
                    if index != a {
                        ui.selectable_value(&mut b, index, name);
                    }
                }
            });
        if a != pair.a() || b != pair.b() {
            state.scene.coloc.set_pair(ColocPair::new(a, b));
        }
        ui.separator();
        let busy = running.is_some() || state.loading.is_some();
        if ui
            .add_enabled(!busy, egui::Button::new("Compute"))
            .clicked()
        {
            actions.push(Action::ComputeColoc);
        }
        if running.is_some() {
            if ui.button("Cancel").clicked() {
                actions.push(Action::CancelColoc);
            }
            ui.spinner();
        }
        if state.scene.coloc.series().is_some() && ui.button("Export CSV").clicked() {
            actions.push(Action::ExportColoc);
        }
        if let Some(running) = running {
            ui.label(
                RichText::new(format!(
                    "frame {} · {}/{} tiles",
                    running.time + 1,
                    running.done,
                    running.total
                ))
                .small()
                .weak(),
            );
        }
        if state
            .scene
            .coloc
            .region()
            .is_some_and(|region| region != state.scene.region)
        {
            ui.label(
                RichText::new("the region changed · compute again")
                    .small()
                    .color(WARNING),
            );
        }
        if state.loading.is_some() {
            ui.label(RichText::new("the dataset is loading").small().weak());
        }
    });
}

fn thresholds(ui: &mut Ui, state: &mut State<'_>) {
    let Some(pair) = state.scene.coloc.pair() else {
        return;
    };
    let thresholds = state.scene.coloc.thresholds();
    let tints = [
        state.scene.dataset.channels()[pair.a()].tint(),
        state.scene.dataset.channels()[pair.b()].tint(),
    ];
    let mut a = thresholds.a();
    let mut b = thresholds.b();
    let mut changed = false;
    ui.horizontal(|ui| {
        color_swatch(ui, tints[0]);
        changed |= ui
            .add(
                egui::Slider::new(&mut a, 0.0..=1.0)
                    .text("M2 threshold")
                    .max_decimals(3),
            )
            .changed();
    });
    ui.horizontal(|ui| {
        color_swatch(ui, tints[1]);
        changed |= ui
            .add(
                egui::Slider::new(&mut b, 0.0..=1.0)
                    .text("M1 threshold")
                    .max_decimals(3),
            )
            .changed();
    });
    if changed {
        state.scene.coloc.set_thresholds(ColocThresholds::new(a, b));
    }
    let otsu = state.scene.coloc.series().map(ColocSeries::otsu);
    let thresholds = state.scene.coloc.thresholds();
    ui.horizontal(|ui| {
        if ui.small_button("Otsu").clicked()
            && let Some(otsu) = otsu
        {
            state.scene.coloc.set_thresholds(otsu);
        }
        ui.label(
            RichText::new(format!(
                "raw {:.0} · {:.0}",
                state.scene.dataset.channels()[pair.a()]
                    .intensity()
                    .value(thresholds.a()),
                state.scene.dataset.channels()[pair.b()]
                    .intensity()
                    .value(thresholds.b())
            ))
            .small()
            .weak(),
        );
    });
}

fn statistics(ui: &mut Ui, state: &State<'_>) {
    let Some(series) = state.scene.coloc.series_statistics() else {
        return;
    };
    let current = state.scene.coloc.statistics(state.scene.time());
    ui.add_space(4.0);
    egui::Grid::new("grana.coloc.statistics")
        .num_columns(6)
        .spacing([16.0, 3.0])
        .show(ui, |ui| {
            for label in ["", "Pearson", "Overlap", "M1", "M2", "voxels"] {
                ui.label(RichText::new(label).small().weak());
            }
            ui.end_row();
            metrics(ui, "series", series);
            if let Some(current) = current {
                metrics(ui, &format!("frame {}", state.scene.time() + 1), current);
            }
        });
}

fn metrics(ui: &mut Ui, label: &str, statistics: ColocStatistics) {
    ui.label(RichText::new(label).small().weak());
    ui.monospace(number(statistics.pearson()));
    ui.monospace(number(statistics.overlap()));
    let manders = statistics.manders();
    ui.monospace(number(manders[0]));
    ui.monospace(number(manders[1]));
    ui.monospace(statistics.voxels().to_string());
    ui.end_row();
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_string(), |value| format!("{value:.3}"))
}

fn scatter(ui: &mut Ui, state: &State<'_>) {
    let pair = state.scene.coloc.pair().expect("the panel names a pair");
    let Some(series) = state.scene.coloc.series() else {
        return;
    };
    let thresholds = state.scene.coloc.thresholds();
    let channels = state.scene.dataset.channels();
    let tints = [channels[pair.a()].tint(), channels[pair.b()].tint()];
    let names = [channels[pair.a()].name(), channels[pair.b()].name()];
    let side = (ui.available_height() - 18.0).clamp(PLOT_MIN, PLOT_MAX);
    let (response, painter) = ui.allocate_painter(vec2(side, side), egui::Sense::hover());
    let rect = response.rect;
    painter.rect_filled(rect, 4.0, Color32::from_rgb(11, 14, 20));
    let maximum = series
        .scatter()
        .iter()
        .skip(1)
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    let ceiling = (maximum as f32 + 1.0).ln();
    let cell = rect.width() / COLOC_SCATTER as f32;
    let mut mesh = egui::Mesh::default();
    for bucket_b in 0..COLOC_SCATTER {
        for bucket_a in 0..COLOC_SCATTER {
            let count = series.scatter_at(bucket_a, bucket_b);
            if count == 0 {
                continue;
            }
            let weight = ((count as f32 + 1.0).ln() / ceiling).clamp(0.0, 1.0);
            let blend = (bucket_a + bucket_b) as f32 / (2.0 * (COLOC_SCATTER - 1) as f32);
            let tint =
                [0, 1, 2].map(|axis| tints[0][axis] + (tints[1][axis] - tints[0][axis]) * blend);
            let brightness = 0.08 + 0.92 * weight;
            let color = Color32::from_rgb(
                (tint[0] * brightness * 255.0) as u8,
                (tint[1] * brightness * 255.0) as u8,
                (tint[2] * brightness * 255.0) as u8,
            );
            let corner = pos2(
                rect.left() + bucket_a as f32 * cell,
                rect.bottom() - (bucket_b as f32 + 1.0) * cell,
            );
            mesh.add_colored_rect(Rect::from_min_size(corner, vec2(cell, cell)), color);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(236, 236, 246, 130));
    let vertical = rect.left() + thresholds.a() * rect.width();
    painter.line_segment(
        [pos2(vertical, rect.top()), pos2(vertical, rect.bottom())],
        stroke,
    );
    let horizontal = rect.bottom() - thresholds.b() * rect.height();
    painter.line_segment(
        [
            pos2(rect.left(), horizontal),
            pos2(rect.right(), horizontal),
        ],
        stroke,
    );
    painter.text(
        pos2(rect.right(), rect.bottom() + 3.0),
        Align2::RIGHT_TOP,
        format!("{} (x)", names[0]),
        FontId::proportional(10.0),
        tint(tints[0]),
    );
    painter.text(
        pos2(rect.left() + 4.0, rect.top() + 4.0),
        Align2::LEFT_TOP,
        format!("{} (y)", names[1]),
        FontId::proportional(10.0),
        tint(tints[1]),
    );
    painter.text(
        pos2(rect.right() - 4.0, rect.top() + 4.0),
        Align2::RIGHT_TOP,
        "all frames",
        FontId::proportional(10.0),
        Color32::from_gray(120),
    );
}

fn tint(color: [f32; 3]) -> Color32 {
    Color32::from_rgb(
        (color[0] * 255.0) as u8,
        (color[1] * 255.0) as u8,
        (color[2] * 255.0) as u8,
    )
}
