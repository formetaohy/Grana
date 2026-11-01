use glam::Vec2;
use grana_object::{COLOC_SCATTER, ColocPair, ColocSeries, ColocStatistics, ColocThresholds};
use grana_scene::{Job, Progress};
use grana_ui::{Align, Block, Color, Corners, Length, Rect, Role, TextStyle, Ui};

use crate::action::Action;
use crate::panel::progress;
use crate::state::State;
use crate::widget;

const PLOT_MAX: f32 = 224.0;
const PLOT_MIN: f32 = 112.0;
const PLOT_CELLS: usize = 72;

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    widget::caption(
        ui,
        "intensity correlation and threshold overlap for a channel pair",
    );
    let running = progress(state.analysis, Job::Coloc);
    header(ui, state, actions, running);
    if state.scene.coloc.pair().is_none() {
        return;
    }
    if state.scene.coloc.series().is_none() {
        let text = if running.is_some() {
            "streaming both channels over every frame of the region"
        } else {
            "compute to stream both channels over every frame of the region"
        };
        widget::weak(ui, text);
        return;
    }
    ui.row(Block::row().gaps(24.0).align(Align::Start), |ui| {
        ui.column(
            Block::column().gaps(10.0).width(Length::Fixed(360.0)),
            |ui| {
                thresholds(ui, state);
                statistics(ui, state);
            },
        );
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
        widget::weak(ui, "the dataset needs two channels to colocalize");
        return;
    };
    let names = state
        .scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let a = pair.a();
    let b = pair.b();
    let mut chosen_a = a;
    let mut chosen_b = b;
    ui.row(Block::row().gaps(10.0).align(Align::Center), |ui| {
        let options = names
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != b)
            .map(|(index, name)| (index, name.clone()))
            .collect::<Vec<_>>();
        if let Some(value) = grana_ui::popover::combo(ui, "coloc.a", 140.0, &options, Some(a)) {
            chosen_a = value;
        }
        widget::caption(ui, "×");
        let options = names
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != a)
            .map(|(index, name)| (index, name.clone()))
            .collect::<Vec<_>>();
        if let Some(value) = grana_ui::popover::combo(ui, "coloc.b", 140.0, &options, Some(b)) {
            chosen_b = value;
        }
        if chosen_a != a || chosen_b != b {
            state
                .scene
                .coloc
                .set_pair(ColocPair::new(chosen_a, chosen_b));
        }
        let busy = running.is_some() || state.loading.is_some();
        if !busy && widget::button(ui, "compute", "Compute", Role::Prominent) {
            actions.push(Action::ComputeColoc);
        }
        if running.is_some() {
            if widget::button(ui, "cancel", "Cancel", Role::Quiet) {
                actions.push(Action::CancelColoc);
            }
            widget::spinner(ui, 14.0);
        }
        if state.scene.coloc.series().is_some()
            && widget::button(ui, "export", "Export CSV", Role::Control)
        {
            actions.push(Action::ExportColoc);
        }
    });
    ui.row(Block::row().gaps(10.0).align(Align::Center), |ui| {
        if let Some(running) = running {
            widget::caption(
                ui,
                &format!(
                    "frame {} · {}/{} tiles",
                    running.time + 1,
                    running.done,
                    running.total
                ),
            );
        }
        if state
            .scene
            .coloc
            .region()
            .is_some_and(|region| region != state.scene.region)
        {
            let style = TextStyle::new(
                ui.theme().font(grana_ui::Level::Caption),
                Color::rgb(1.0, 0.69, 0.45),
            );
            ui.text_style("the region changed · compute again", style);
        }
        if state.loading.is_some() {
            widget::caption(ui, "the dataset is loading");
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
    for (index, tint) in tints.iter().enumerate() {
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            crate::panel::swatch(ui, *tint);
            let value = if index == 0 { a } else { b };
            let label = if index == 0 {
                "M2 threshold"
            } else {
                "M1 threshold"
            };
            if let Some(next) = widget::slider_readout(
                ui,
                &format!("threshold.{index}"),
                Some(label),
                value,
                0.0..=1.0,
                false,
                &format!("{value:.3}"),
            ) {
                if index == 0 {
                    a = next;
                } else {
                    b = next;
                }
                changed = true;
            }
        });
    }
    if changed {
        state.scene.coloc.set_thresholds(ColocThresholds::new(a, b));
    }
    let otsu = state.scene.coloc.series().map(ColocSeries::otsu);
    let thresholds = state.scene.coloc.thresholds();
    ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
        if widget::button(ui, "otsu", "Otsu", Role::Quiet)
            && let Some(otsu) = otsu
        {
            state.scene.coloc.set_thresholds(otsu);
        }
        widget::caption(
            ui,
            &format!(
                "raw {:.0} · {:.0}",
                state.scene.dataset.channels()[pair.a()]
                    .intensity()
                    .value(thresholds.a()),
                state.scene.dataset.channels()[pair.b()]
                    .intensity()
                    .value(thresholds.b())
            ),
        );
    });
}

fn statistics(ui: &mut Ui, state: &State<'_>) {
    let Some(series) = state.scene.coloc.series_statistics() else {
        return;
    };
    let current = state.scene.coloc.statistics(state.scene.time());
    let headers = ["", "Pearson", "Overlap", "M1", "M2", "voxels"];
    let widths = [64.0f32, 66.0, 62.0, 48.0, 48.0, 58.0];
    let head = ui.allocate(Vec2::new(widths.iter().sum(), 18.0));
    {
        let font = ui.theme().font(grana_ui::Level::Caption);
        let color = ui.theme().palette.text_tertiary;
        let mut painter = ui.painter();
        let mut left = head.min.x;
        for (label, width) in headers.iter().zip(widths.iter()) {
            painter.text_in(
                Rect::from_min_size(
                    Vec2::new(left, head.min.y),
                    Vec2::new(*width, head.height()),
                ),
                label,
                TextStyle::new(font, color).aligned(if label.is_empty() {
                    Align::Start
                } else {
                    Align::End
                }),
                Align::Center,
            );
            left += width;
        }
    }
    metrics(ui, "series", series, &widths);
    if let Some(current) = current {
        metrics(
            ui,
            &format!("frame {}", state.scene.time() + 1),
            current,
            &widths,
        );
    }
}

fn metrics(ui: &mut Ui, label: &str, statistics: ColocStatistics, widths: &[f32; 6]) {
    let row = ui.allocate(Vec2::new(widths.iter().sum(), 20.0));
    let font = ui.theme().font(grana_ui::Level::Monospace);
    let color = ui.theme().palette.text_secondary;
    let caption_font = ui.theme().font(grana_ui::Level::Caption);
    let caption_color = ui.theme().palette.text_tertiary;
    let mut values = vec![number(statistics.pearson()), number(statistics.overlap())];
    let manders = statistics.manders();
    values.push(number(manders[0]));
    values.push(number(manders[1]));
    values.push(statistics.voxels().to_string());
    let mut painter = ui.painter();
    painter.text_in(
        Rect::from_min_size(row.min, Vec2::new(widths[0], row.height())),
        label,
        TextStyle::new(caption_font, caption_color),
        Align::Center,
    );
    let mut left = row.min.x + widths[0];
    for (index, value) in values.iter().enumerate() {
        let width = widths[index + 1];
        painter.text_in(
            Rect::from_min_size(Vec2::new(left, row.min.y), Vec2::new(width, row.height())),
            value,
            TextStyle::new(font, color).aligned(Align::End),
            Align::Center,
        );
        left += width;
    }
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(|| "\u{2014}".to_string(), |value| format!("{value:.3}"))
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
    let available = ui.available();
    let side = (available.y - 26.0)
        .clamp(PLOT_MIN, PLOT_MAX)
        .min(available.x);
    let rect = ui.allocate(Vec2::new(side, side + 24.0));
    let plot = Rect::from_min_size(rect.min, Vec2::splat(side));
    let maximum = series
        .scatter()
        .iter()
        .skip(1)
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    let ceiling = (maximum as f32 + 1.0).ln();
    let block = COLOC_SCATTER.div_ceil(PLOT_CELLS);
    let cell = side / PLOT_CELLS as f32;
    let mut positions = Vec::new();
    for row in 0..PLOT_CELLS {
        for column in 0..PLOT_CELLS {
            let mut count = 0u64;
            for inner_row in 0..block {
                for inner_column in 0..block {
                    let bucket_a = column * block + inner_column;
                    let bucket_b = row * block + inner_row;
                    if bucket_a < COLOC_SCATTER && bucket_b < COLOC_SCATTER {
                        count = count.max(series.scatter_at(bucket_a, bucket_b));
                    }
                }
            }
            if count == 0 {
                continue;
            }
            let weight = ((count as f32 + 1.0).ln() / ceiling).clamp(0.0, 1.0);
            let blend = (column + row) as f32 / (2.0 * (PLOT_CELLS - 1) as f32);
            let tint =
                [0, 1, 2].map(|axis| tints[0][axis] + (tints[1][axis] - tints[0][axis]) * blend);
            let brightness = 0.10 + 0.90 * weight;
            positions.push((
                Rect::from_min_size(
                    Vec2::new(
                        plot.min.x + column as f32 * cell,
                        plot.max.y - (row as f32 + 1.0) * cell,
                    ),
                    Vec2::splat(cell + 0.4),
                ),
                Color::rgb(
                    tint[0] * brightness,
                    tint[1] * brightness,
                    tint[2] * brightness,
                ),
            ));
        }
    }
    let font = ui.theme().font(grana_ui::Level::Micro);
    let quiet = ui.theme().palette.text_tertiary;
    let line = Color::rgba(0.93, 0.93, 0.97, 0.5);
    let vertical = plot.min.x + thresholds.a() * plot.width();
    let horizontal = plot.max.y - thresholds.b() * plot.height();
    let mut painter = ui.painter();
    painter.rect(plot, Corners::all(8.0), Color::rgb(0.043, 0.055, 0.078));
    for (rect, color) in positions {
        painter.rect(rect, Corners::ZERO, color);
    }
    painter.segment(
        Vec2::new(vertical, plot.min.y),
        Vec2::new(vertical, plot.max.y),
        1.0,
        line,
    );
    painter.segment(
        Vec2::new(plot.min.x, horizontal),
        Vec2::new(plot.max.x, horizontal),
        1.0,
        line,
    );
    painter.text_at(
        Vec2::new(plot.min.x + 5.0, plot.min.y + 5.0),
        &format!("{} (y)", names[1]),
        TextStyle::new(font, Color::from_rgb_f32(tints[1])),
    );
    painter.text_at(
        Vec2::new(plot.max.x - 76.0, plot.min.y + 5.0),
        "all frames",
        TextStyle::new(font, quiet),
    );
    painter.text_at(
        Vec2::new(plot.max.x - 96.0, plot.max.y + 6.0),
        &format!("{} (x)", names[0]),
        TextStyle::new(font, Color::from_rgb_f32(tints[0])),
    );
}
