use glam::Vec2;
use grana_measure::{COLOC_SCATTER, ColocPair, ColocSeries, ColocStatistics, ColocThresholds};
use grana_scene::{Job, Progress};
use grana_ui::{Align, Block, Color, Corners, Icon, Length, Rect, Role, TextStyle, TypeStyle, Ui};

use crate::panel::{card, card_title, empty_state};
use crate::state::State;
use crate::widget;
use grana_session::Command;

const PLOT_CELLS: usize = 72;
const PLOT_SIDE: f32 = 200.0;
const LEFT_WIDTH: f32 = 372.0;

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let Some(pair) = state.scene.coloc.pair() else {
        empty_state(
            ui,
            Icon::Grid,
            "Two channels required",
            "Colocalization compares the intensities of two channels across every frame in the region.",
        );
        return;
    };
    let running = crate::panel::progress(state.jobs, Job::Coloc);
    header(ui, state, commands, pair, running);
    if state.scene.coloc.series().is_none() {
        empty_state(
            ui,
            Icon::Grid,
            "No colocalization yet",
            if running.is_some() {
                "Streaming both channels over every frame of the region."
            } else {
                "Choose a channel pair, then compute correlation, overlap and Manders coefficients."
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
            },
        );
        scatter(ui, state);
    });
}

fn header(
    ui: &mut Ui,
    state: &mut State<'_>,
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
    let row = ui.allocate_full(32.0);
    let left = Rect::from_min_max(row.min, Vec2::new(row.max.x - 214.0, row.max.y));
    let right = Rect::from_min_size(
        Vec2::new(row.max.x - 206.0, row.min.y),
        Vec2::new(206.0, row.height()),
    );
    ui.place(left, |ui| {
        ui.row(
            Block::row()
                .gaps(8.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                widget::field_label(ui, "PAIR");
                let options = names
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != b)
                    .map(|(index, name)| (index, name.clone()))
                    .collect::<Vec<_>>();
                if let Some(value) =
                    grana_ui::popover::combo(ui, "coloc.a", 132.0, &options, Some(a))
                {
                    chosen_a = value;
                }
                widget::caption(ui, "×");
                let options = names
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != a)
                    .map(|(index, name)| (index, name.clone()))
                    .collect::<Vec<_>>();
                if let Some(value) =
                    grana_ui::popover::combo(ui, "coloc.b", 132.0, &options, Some(b))
                {
                    chosen_b = value;
                }
                if chosen_a != a || chosen_b != b {
                    commands.push(Command::SetColocPair(ColocPair::new(chosen_a, chosen_b)));
                }
                let busy = running.is_some() || state.io.loading.is_some();
                if running.is_none()
                    && widget::action_button(
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
            },
        );
    });
    ui.place(right, |ui| {
        ui.row(
            Block::row()
                .gaps(6.0)
                .align(Align::Center)
                .justify(Align::End)
                .width(Length::Fill)
                .height(Length::Fill),
            |ui| {
                if let Some(running) = running {
                    widget::caption(
                        ui,
                        &format!(
                            "frame {} · {}/{}",
                            running.time + 1,
                            running.done,
                            running.total
                        ),
                    );
                    if widget::icon_button(ui, "cancel", Icon::Close, 28.0, Role::Quiet, "Cancel") {
                        commands.push(Command::CancelColoc);
                    }
                }
                if state.scene.coloc.series().is_some()
                    && widget::icon_button(
                        ui,
                        "export",
                        Icon::Export,
                        28.0,
                        Role::Quiet,
                        "Export CSV",
                    )
                {
                    commands.push(Command::ExportColoc);
                }
            },
        );
    });
}

fn thresholds(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
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
                    if let Some(next) =
                        widget::slider(ui, "threshold", None, value, 0.0..=1.0, false)
                    {
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
        let otsu = state.scene.coloc.series().map(ColocSeries::otsu);
        let thresholds = state.scene.coloc.thresholds();
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if let Some(otsu) = otsu
                && widget::button(ui, "otsu", "Otsu", Role::Quiet)
            {
                commands.push(Command::SetColocThresholds(otsu));
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
    });
}

fn statistics(ui: &mut Ui, state: &State<'_>) {
    let Some(series) = state.scene.coloc.series_statistics() else {
        return;
    };
    let current = state.scene.coloc.statistics(state.scene.time());
    card(ui, 6.0, |ui| {
        card_title(ui, Icon::Table, "Statistics", None);
        let headers = ["", "Pearson", "Overlap", "M1", "M2", "voxels"];
        let widths = [52.0f32, 62.0, 60.0, 44.0, 44.0, 54.0];
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
    card(ui, 8.0, |ui| {
        card_title(ui, Icon::Grid, "Scatter", Some("all frames"));
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
            let brightness = 0.22 + 0.78 * weight;
            positions.push((
                Rect::from_min_size(
                    Vec2::new(column as f32 * cell, (PLOT_CELLS - row - 1) as f32 * cell),
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
