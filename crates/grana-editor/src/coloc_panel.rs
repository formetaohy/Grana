use glam::Vec2;
use grana_measure::{
    COLOC_BLOCK_MAX, COLOC_PERMUTATIONS_MAX, ColocPair, ColocSeries, ColocSettings,
    ColocStatistics, ColocThresholds,
};
use grana_scene::{Job, ObjectSetId, Progress};
use grana_ui::{Align, Block, Color, Corners, Icon, Length, Rect, Role, TextStyle, TypeStyle, Ui};

use crate::panel::{card, card_title, empty_state};
use crate::state::State;
use crate::widget;
use grana_session::Command;

const PLOT_CELLS: usize = 160;
const PLOT_SIDE: f32 = 200.0;
const LEFT_WIDTH: f32 = 372.0;

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let Some(pair) = state.scene.coloc.pair() else {
        empty_state(
            ui,
            Icon::Grid,
            "Two channels required",
            "Compares two channels across every frame in the region and mask.",
        );
        return;
    };
    let running = crate::panel::progress(state.jobs, Job::Coloc);
    header(ui, state, commands, pair, running);
    if state.scene.coloc.result().is_none() {
        empty_state(
            ui,
            Icon::Grid,
            "No colocalization yet",
            if running.is_some() {
                "Streaming both channels over every frame of the current region and analysis mask."
            } else {
                "Choose a channel pair, then compute overlap, Manders coefficients and Costes."
            },
        );
        return;
    }
    ui.row(Block::row().gaps(14.0).align(Align::Start), |ui| {
        ui.column(
            Block::column().gaps(10.0).width(Length::Fixed(LEFT_WIDTH)),
            |ui| {
                thresholds(ui, state, commands);
                statistics(ui, state);
                method(ui, state, commands);
            },
        );
        scatter(ui, state);
    });
}

fn header(
    ui: &mut Ui,
    state: &State<'_>,
    commands: &mut Vec<Command>,
    pair: ColocPair,
    running: Option<Progress>,
) {
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
    ui.row(
        Block::row()
            .gaps(8.0)
            .align(Align::Center)
            .height(Length::Fixed(32.0)),
        |ui| {
            widget::field_label(ui, "Pair");
            let options = names
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != b)
                .map(|(index, name)| (index, name.clone()))
                .collect::<Vec<_>>();
            if let Some(value) = grana_ui::popover::combo(ui, "coloc.a", 118.0, &options, Some(a)) {
                chosen_a = value;
            }
            widget::caption(ui, "×");
            let options = names
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != a)
                .map(|(index, name)| (index, name.clone()))
                .collect::<Vec<_>>();
            if let Some(value) = grana_ui::popover::combo(ui, "coloc.b", 118.0, &options, Some(b)) {
                chosen_b = value;
            }
            if chosen_a != a || chosen_b != b {
                commands.push(Command::SetColocPair(ColocPair::new(chosen_a, chosen_b)));
            }
            let busy = running.is_some() || state.io.loading.is_some();
            match running {
                Some(running) => {
                    widget::caption(ui, &format!("{}/{} samples", running.done, running.total));
                    if widget::icon_button(ui, "cancel", Icon::Close, 28.0, Role::Quiet, "Cancel") {
                        commands.push(Command::CancelColoc);
                    }
                }
                None => {
                    if widget::action_button(
                        ui,
                        "compute",
                        Icon::Target,
                        "Compute",
                        Role::Prominent,
                    )
                    .clicked
                        && !busy
                    {
                        commands.push(Command::ComputeColoc);
                    }
                }
            }
            if state.scene.coloc.result().is_some()
                && widget::icon_button(ui, "export", Icon::Export, 28.0, Role::Quiet, "Export CSV")
            {
                commands.push(Command::ExportColoc);
            }
        },
    );
    results_row(ui, state, commands, pair);
}

fn results_row(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>, pair: ColocPair) {
    let results = state.scene.coloc.results();
    if results.is_empty() {
        return;
    }
    let names = state
        .scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    ui.row(
        Block::row()
            .gaps(6.0)
            .align(Align::Center)
            .height(Length::Fixed(28.0)),
        |ui| {
            for (index, result) in results.iter().enumerate() {
                let stored = result.pair();
                let label = format!("{} × {}", names[stored.a()], names[stored.b()]);
                let role = if stored == pair {
                    Role::Tinted
                } else {
                    Role::Quiet
                };
                if widget::button(ui, &format!("coloc.saved.{index}"), &label, role) {
                    commands.push(Command::SetColocPair(stored));
                }
            }
            if state.scene.coloc.result().is_some()
                && widget::icon_button(
                    ui,
                    "coloc.remove",
                    Icon::Trash,
                    28.0,
                    Role::Quiet,
                    "Remove this result",
                )
            {
                commands.push(Command::RemoveColocResult(pair));
            }
        },
    );
}

fn thresholds(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let Some(result) = state.scene.coloc.result() else {
        return;
    };
    let pair = result.pair();
    let thresholds = result.thresholds();
    let series = result.series();
    let tints = [
        state.scene.dataset.channels()[pair.a()].tint(),
        state.scene.dataset.channels()[pair.b()].tint(),
    ];
    let mut a = thresholds.a();
    let mut b = thresholds.b();
    let mut changed = false;
    card(ui, 6.0, |ui| {
        card_title(ui, Icon::Sliders, "Thresholds", None);
        for (index, tint) in tints.iter().enumerate() {
            let value = if index == 0 { a } else { b };
            let label = if index == 0 { "M1" } else { "M2" };
            let font = ui.theme().font(grana_ui::Level::Caption);
            let secondary = ui.theme().palette.text_secondary;
            let primary = ui.theme().palette.text_primary;
            let dot_tint = Color::from_rgb_f32(*tint);
            let row = ui.allocate_full(30.0);
            let swatch = Rect::from_center_size(
                Vec2::new(row.min.x + 5.0, row.center().y),
                Vec2::splat(9.0),
            );
            {
                let mut painter = ui.painter();
                painter.rect(swatch, Corners::capsule(), dot_tint);
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(row.min.x + 14.0, row.min.y),
                        Vec2::new(row.min.x + 42.0, row.max.y),
                    ),
                    label,
                    TextStyle::new(font, secondary),
                    Align::Center,
                );
            }
            ui.place(
                Rect::from_min_max(
                    Vec2::new(row.min.x + 44.0, row.min.y),
                    Vec2::new(row.max.x - 64.0, row.max.y),
                ),
                |ui| {
                    if let Some(next) = widget::slider(
                        ui,
                        &format!("threshold.{index}"),
                        None,
                        value,
                        0.0..=1.0,
                        false,
                    ) {
                        if index == 0 {
                            a = next;
                        } else {
                            b = next;
                        }
                        changed = true;
                    }
                },
            );
            ui.place(
                Rect::from_min_max(Vec2::new(row.max.x - 62.0, row.min.y), row.max),
                |ui| {
                    let style =
                        TextStyle::new(ui.theme().font(grana_ui::Level::Monospace), primary)
                            .aligned(Align::End);
                    ui.text_in(
                        Rect::from_min_size(
                            ui.cursor_position(),
                            Vec2::new(ui.available().x, row.height()),
                        ),
                        &format!("{value:.3}"),
                        style,
                        Align::Center,
                    );
                },
            );
        }
        if changed {
            commands.push(Command::SetColocThresholds(ColocThresholds::new(a, b)));
        }
        let otsu = series.otsu();
        let costes = series.costes();
        let channels = state.scene.dataset.channels();
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if widget::button(ui, "otsu", "Otsu", Role::Quiet) {
                commands.push(Command::SetColocThresholds(otsu));
            }
            if let Some(range) = costes
                && widget::button(ui, "costes", "Costes", Role::Quiet)
            {
                commands.push(Command::SetColocThresholds(range.high()));
            }
            widget::caption(
                ui,
                &format!(
                    "raw {:.0} · {:.0}",
                    channels[pair.a()].intensity().value(thresholds.a()),
                    channels[pair.b()].intensity().value(thresholds.b())
                ),
            );
        });
        if let Some(range) = costes {
            let low = range.low();
            let high = range.high();
            widget::caption(
                ui,
                &format!(
                    "costes M1 {:.3}–{:.3} · M2 {:.3}–{:.3}",
                    low.a(),
                    high.a(),
                    low.b(),
                    high.b()
                ),
            );
        }
    });
}

fn statistics(ui: &mut Ui, state: &State<'_>) {
    let Some(series) = state.scene.coloc.series_statistics() else {
        return;
    };
    let current = state.scene.coloc.statistics(state.scene.time());
    card(ui, 6.0, |ui| {
        card_title(ui, Icon::Table, "Statistics", None);
        let headers = ["", "Pearson", "Overlap", "M1", "M2", "voxels", "p"];
        let widths = [46.0f32, 54.0, 52.0, 40.0, 40.0, 48.0, 46.0];
        let head = ui.allocate(Vec2::new(widths.iter().sum(), 18.0));
        {
            let font = ui.theme().font(grana_ui::Level::Micro);
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
        metrics(ui, "Series", series, &widths);
        if let Some(current) = current {
            metrics(
                ui,
                &format!("Frame {}", state.scene.time() + 1),
                current,
                &widths,
            );
        }
    });
}

fn method(ui: &mut Ui, state: &State<'_>, commands: &mut Vec<Command>) {
    let Some(result) = state.scene.coloc.result() else {
        return;
    };
    let settings = state.scene.coloc.settings();
    let cost = result.cost();
    let level = result.level();
    let permutation_level = result.permutation_level();
    let stride = result.permutation_stride();
    let mask = result
        .mask()
        .and_then(ObjectSetId::from_raw)
        .and_then(|id| state.scene.objects.get(id))
        .map_or_else(|| "whole volume".to_string(), |set| set.name().to_string());
    let mut permutations = settings.permutations() as f32;
    let mut block = settings.block() as f32;
    let max_permutations = COLOC_PERMUTATIONS_MAX as f32;
    let max_block = COLOC_BLOCK_MAX as f32;
    card(ui, 5.0, |ui| {
        card_title(ui, Icon::Sparkles, "Method", None);
        widget::labelled(
            ui,
            "Resolution",
            &format!("level {level} · permutation {permutation_level} · stride {stride}"),
        );
        widget::labelled(ui, "Scan", &format!("{} voxels", cost.scan_voxels()));
        widget::labelled(
            ui,
            "Samples",
            &format!(
                "{} × {} frames · {} blocks",
                cost.permutation_voxels(),
                cost.permutation_frames(),
                cost.blocks()
            ),
        );
        widget::labelled(ui, "Mask", &mask);
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            widget::field_label(ui, "Shuffles");
            if let Some(value) = widget::drag_value(ui, "coloc.permutations", permutations, 0.5, 0)
            {
                permutations = value.round().clamp(1.0, max_permutations);
            }
            widget::field_label(ui, "PSF");
            if let Some(value) = widget::drag_value(ui, "coloc.block", block, 0.1, 0) {
                block = value.round().clamp(1.0, max_block);
            }
        });
        if permutations as u32 != settings.permutations() || block as u32 != settings.block() {
            commands.push(Command::SetColocSettings(ColocSettings::new(
                permutations as u32,
                block as u32,
            )));
        }
    });
}

fn metrics(ui: &mut Ui, label: &str, statistics: ColocStatistics, widths: &[f32; 7]) {
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
    values.push(statistics.significance().map_or_else(
        || "\u{2014}".to_string(),
        |significance| format!("{:.4}", significance.p()),
    ));
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
    let Some(result) = state.scene.coloc.result() else {
        return;
    };
    let series = result.series();
    let thresholds = result.thresholds();
    let channels = state.scene.dataset.channels();
    let tints = [channels[pair.a()].tint(), channels[pair.b()].tint()];
    let names = [channels[pair.a()].name(), channels[pair.b()].name()];
    let bins = series.bins();
    let subtitle = format!("all frames · {bins} bins");
    card(ui, 8.0, |ui| {
        card_title(ui, Icon::Grid, "Scatter", Some(&subtitle));
        let full = ui.available();
        let side = PLOT_SIDE.min(full.x - 4.0);
        let positions = scatter_positions(series, tints, side);
        let x = ui.cursor_position().x + (full.x - side) * 0.5;
        let plot = Rect::from_min_size(Vec2::new(x, ui.cursor_position().y), Vec2::splat(side));
        ui.allocate(Vec2::new(full.x, side));
        let background = ui.theme().look(Role::Field, grana_ui::State::Rest);
        let grid = Color::white_alpha(0.05);
        let line = Color::rgba(0.95, 0.96, 1.0, 0.55);
        let mut painter = ui.painter();
        painter.outlined(
            plot,
            Corners::all(8.0),
            background.fill,
            background.border,
            grana_ui::Shadow::NONE,
        );
        for index in 1..4 {
            let fraction = index as f32 / 4.0;
            let x = plot.min.x + plot.width() * fraction;
            let y = plot.min.y + plot.height() * fraction;
            painter.segment(
                Vec2::new(x, plot.min.y),
                Vec2::new(x, plot.max.y),
                0.5,
                grid,
            );
            painter.segment(
                Vec2::new(plot.min.x, y),
                Vec2::new(plot.max.x, y),
                0.5,
                grid,
            );
        }
        for (rect, color) in positions {
            painter.rect(
                rect.translate(plot.min),
                Corners::ZERO,
                Color::rgb(color[0], color[1], color[2]),
            );
        }
        let vertical = plot.min.x + thresholds.a() * plot.width();
        let horizontal = plot.max.y - thresholds.b() * plot.height();
        painter.segment(
            Vec2::new(vertical, plot.min.y),
            Vec2::new(vertical, plot.max.y),
            0.8,
            line,
        );
        painter.segment(
            Vec2::new(plot.min.x, horizontal),
            Vec2::new(plot.max.x, horizontal),
            0.8,
            line,
        );
        if let Some(range) = series.costes() {
            let low = range.low();
            let high = range.high();
            let low_x = plot.min.x + low.a() * plot.width();
            let high_x = plot.min.x + high.a() * plot.width();
            painter.segment(
                Vec2::new(low_x, plot.max.y - 6.0),
                Vec2::new(high_x, plot.max.y - 6.0),
                2.0,
                Color::rgba(1.0, 0.85, 0.45, 0.85),
            );
        }
        let font = TypeStyle::new(10.0, 400.0, 13.0);
        painter.text_at(
            Vec2::new(plot.min.x + 6.0, plot.min.y + 5.0),
            &format!("{} (y)", names[1]),
            TextStyle::new(font, Color::from_rgb_f32(tints[1])),
        );
        painter.text_at(
            Vec2::new(plot.max.x - 62.0, plot.max.y + 5.0),
            &format!("{} (x)", names[0]),
            TextStyle::new(font, Color::from_rgb_f32(tints[0])).aligned(Align::End),
        );
    });
}

fn scatter_positions(
    series: &ColocSeries,
    tints: [[f32; 3]; 2],
    side: f32,
) -> Vec<(Rect, [f32; 3])> {
    let bins = series.bins();
    let cells = bins.min(PLOT_CELLS);
    let block = bins.div_ceil(cells);
    let maximum = series.joint().iter().copied().max().unwrap_or(1).max(1);
    let ceiling = (maximum as f32 + 1.0).ln();
    let cell = side / cells as f32;
    let mut positions = Vec::new();
    for row in 0..cells {
        for column in 0..cells {
            let mut count = 0u64;
            for inner_row in 0..block {
                for inner_column in 0..block {
                    let bucket_a = column * block + inner_column;
                    let bucket_b = row * block + inner_row;
                    if bucket_a < bins && bucket_b < bins {
                        count = count.max(series.joint_at(bucket_a, bucket_b));
                    }
                }
            }
            if count == 0 {
                continue;
            }
            let weight = ((count as f32 + 1.0).ln() / ceiling).clamp(0.0, 1.0);
            let blend = (column + row) as f32 / (2.0 * (cells - 1).max(1) as f32);
            let tint =
                [0, 1, 2].map(|axis| tints[0][axis] + (tints[1][axis] - tints[0][axis]) * blend);
            let brightness = 0.22 + 0.78 * weight;
            positions.push((
                Rect::from_min_size(
                    Vec2::new(column as f32 * cell, (cells - row - 1) as f32 * cell),
                    Vec2::splat(cell + 0.4),
                ),
                [
                    tint[0] * brightness,
                    tint[1] * brightness,
                    tint[2] * brightness,
                ],
            ));
        }
    }
    positions
}
