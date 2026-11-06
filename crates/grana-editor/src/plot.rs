use glam::Vec2;
use grana_measure::{
    Histogram, Lasso, MeasureView, Observation, Plot, SCATTER_BINS, Scatter, Table,
};
use grana_scene::{Item, ObjectSetId, Scene};
use grana_session::Command;
use grana_ui::{
    Align, Border, Button, Color, Corners, Cursor, Fill, Painter, Rect, Shadow, TextStyle,
    TypeStyle, Ui,
};

use crate::measure_panel;
use crate::state::{PlotDrag, PlotGate, State};

const LEFT: f32 = 44.0;
const RIGHT: f32 = 14.0;
const TOP: f32 = 28.0;
const TICK: f32 = 18.0;
const SUMMARY: f32 = 22.0;
const POINT_LIMIT: u64 = 4096;
const BRUSH_SLOP: f32 = 4.0;
const LASSO_STEP: f32 = 1.5;
const HOVER_REACH: f32 = 11.0;
const SCATTER_HINT: &str = "Drag: Lasso · Shift-Drag: Box · Click: Locate";
const HISTOGRAM_HINT: &str = "Drag: Gate · Click: Locate";

enum Readout {
    None,
    Bin {
        low: f32,
        high: f32,
        total: u64,
        gated: u64,
    },
    Cell {
        x: f32,
        y: f32,
        observation: Option<Observation>,
    },
}

pub(crate) fn view(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let measure = state.measure;
    let (Some(table), Some(plot)) = (measure.table(), measure.plot()) else {
        return;
    };
    let card = Rect::from_min_size(ui.cursor_position(), ui.available());
    if card.width() < 140.0 || card.height() < 80.0 {
        return;
    }
    crate::panel::card_surface(ui, card);
    let inner = Rect::from_min_max(
        card.min + Vec2::new(LEFT, TOP),
        Vec2::new(card.max.x - RIGHT, card.max.y - SUMMARY - TICK),
    );
    let current = plot.view();
    if !ui.input().button(Button::Primary).down
        && state
            .workspace
            .drag
            .view
            .is_some_and(|view| view != current)
    {
        state.workspace.drag = PlotDrag::default();
    }
    let readout = match plot {
        Plot::Histogram(histogram) => histogram_view(ui, state, commands, histogram, inner, table),
        Plot::Scatter(scatter) => scatter_view(ui, state, commands, scatter, inner, table),
    };
    titles(ui, plot, table, card);
    summary(ui, plot, table, readout, card);
}

fn titles(ui: &mut Ui, plot: &Plot, table: &Table, card: Rect) {
    let columns = table.columns();
    let (y, x) = match plot {
        Plot::Histogram(histogram) => (
            "Y: Objects".to_string(),
            format!(
                "X: {}",
                crate::panel::column_label(&columns[histogram.column()])
            ),
        ),
        Plot::Scatter(scatter) => (
            format!("Y: {}", crate::panel::column_label(&columns[scatter.y()])),
            format!("X: {}", crate::panel::column_label(&columns[scatter.x()])),
        ),
    };
    let style = TextStyle::new(
        TypeStyle::new(10.5, 500.0, 14.0),
        ui.theme().palette.text_tertiary,
    );
    let strip = Rect::from_min_max(
        card.min + Vec2::new(12.0, 4.0),
        Vec2::new(card.max.x - 12.0, card.min.y + TOP),
    );
    let mut painter = ui.painter();
    painter.text_in(strip, &y, style, Align::Center);
    painter.text_in(strip, &x, style.aligned(Align::End), Align::Center);
}

fn summary(ui: &mut Ui, plot: &Plot, table: &Table, readout: Readout, card: Rect) {
    let total = table.rows() as u64;
    let text = match (plot, readout) {
        (
            Plot::Histogram(histogram),
            Readout::Bin {
                low,
                high,
                total: bin_total,
                gated,
            },
        ) => {
            let column = &table.columns()[histogram.column()];
            let places = column.decimals() as usize;
            let unit = column.unit().suffix();
            format!("{low:.places$} – {high:.places$} {unit} · {gated} of {bin_total}")
        }
        (Plot::Scatter(scatter), Readout::Cell { x, y, observation }) => {
            let first = &table.columns()[scatter.x()];
            let second = &table.columns()[scatter.y()];
            let x_places = first.decimals() as usize;
            let y_places = second.decimals() as usize;
            let mut text = format!(
                "{} {x:.x_places$} {} · {} {y:.y_places$} {}",
                first.label(),
                first.unit().suffix(),
                second.label(),
                second.unit().suffix()
            );
            if let Some(observation) = observation {
                text.push_str(&located(observation));
            }
            text
        }
        (Plot::Histogram(histogram), _) => {
            let statistics = histogram.summary();
            let column = &table.columns()[histogram.column()];
            let places = column.decimals() as usize;
            let unit = column.unit().suffix();
            if statistics.count() == 0 {
                format!("{total} objects · Ungated · {HISTOGRAM_HINT}")
            } else {
                format!(
                    "{} of {total} gated · Mean {:.places$} {unit} · σ {:.places$} · {HISTOGRAM_HINT}",
                    statistics.count(),
                    statistics.mean(),
                    statistics.deviation()
                )
            }
        }
        (Plot::Scatter(scatter), _) => {
            let correlation = scatter
                .correlation()
                .map_or_else(|| "r —".to_string(), |value| format!("r = {value:.2}"));
            if scatter.x_summary().count() == 0 {
                format!("{total} objects · Ungated · {SCATTER_HINT}")
            } else {
                format!(
                    "{} of {total} gated · {correlation} · {SCATTER_HINT}",
                    scatter.x_summary().count()
                )
            }
        }
    };
    let style = TextStyle::new(
        TypeStyle::new(10.5, 400.0, 14.0),
        ui.theme().palette.text_tertiary,
    )
    .truncating();
    let rect = Rect::from_min_max(
        Vec2::new(card.min.x + 12.0, card.max.y - SUMMARY),
        card.max - Vec2::new(12.0, 2.0),
    );
    let mut painter = ui.painter();
    painter.text_in(rect, &text, style, Align::Center);
}

fn located(observation: Observation) -> String {
    match (
        observation.time(),
        observation.object(),
        observation.track(),
    ) {
        (Some(time), Some(object), _) => format!(" · Frame {} · Object {}", time + 1, object + 1),
        (_, _, Some(track)) => format!(" · Track {}", track + 1),
        (Some(time), _, _) => format!(" · Frame {}", time + 1),
        _ => String::new(),
    }
}

fn histogram_view(
    ui: &mut Ui,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
    histogram: &Histogram,
    inner: Rect,
    table: &Table,
) -> Readout {
    if inner.is_empty() {
        return Readout::None;
    }
    let measure = state.measure;
    let key = ui.id("plot.histogram");
    let response = ui.interact(key, inner);
    if response.hovered {
        ui.set_cursor(Cursor::Crosshair);
    }
    let pointer = ui
        .input()
        .position()
        .clamp(inner.min, inner.max - Vec2::splat(1.0));
    let released = ui.input().button(Button::Primary).released;
    let clicked = response.clicked;
    let began = response.active && ui.input().button(Button::Primary).pressed;
    let mut drag = std::mem::take(&mut state.workspace.drag);
    if began && ui.pointer_in(inner).is_some() {
        drag.view = Some(MeasureView::Histogram);
        drag.gate = Some(PlotGate::Rectangle);
        drag.origin = Some(pointer);
        drag.moved = false;
        drag.vertices.clear();
    }
    let mut brush = None;
    if let Some(origin) = drag
        .origin
        .filter(|_| drag.view == Some(MeasureView::Histogram))
    {
        if pointer.distance(origin) > BRUSH_SLOP {
            drag.moved = true;
        }
        let from = value_at(inner, histogram.low(), histogram.high(), origin.x);
        let to = value_at(inner, histogram.low(), histogram.high(), pointer.x);
        if drag.moved {
            brush = Some((from.min(to), from.max(to)));
        }
        if released {
            if drag.moved {
                let width =
                    (to - from).abs() / (histogram.high() - histogram.low()) * inner.width();
                if width >= BRUSH_SLOP {
                    commands.push(Command::SetGate {
                        column: histogram.column(),
                        minimum: from.min(to),
                        maximum: from.max(to),
                    });
                }
            }
            drag.origin = None;
            drag.moved = false;
        }
    }
    state.workspace.drag = drag;
    if clicked
        && let Some(row) = table.nearest(
            measure.rows(),
            &[histogram.column()],
            &[value_at(
                inner,
                histogram.low(),
                histogram.high(),
                pointer.x,
            )],
            &[inner.width()],
        )
        && let Some(set) = measure.set()
    {
        measure_panel::locate(commands, set, table.observation(row));
    }
    let hovered = if brush.is_some() {
        None
    } else {
        ui.pointer_in(inner)
    };
    let bins = histogram.bins();
    let peak = histogram.peak().max(1);
    let width = inner.width() / bins as f32;
    let gap = if width > 4.0 { 1.0 } else { 0.0 };
    let accent = ui.theme().palette.accent;
    let font = ui.theme().font(grana_ui::Level::Monospace);
    let tick_color = ui.theme().palette.text_tertiary;
    let empty = TextStyle::new(
        ui.theme().font(grana_ui::Level::Caption),
        ui.theme().palette.text_tertiary,
    )
    .aligned(Align::Center);
    let mut readout = Readout::None;
    let mut painter = ui.painter();
    axis_grid(
        &mut painter,
        inner,
        (histogram.low(), histogram.high()),
        (0.0, peak as f32),
        font,
        tick_color,
    );
    if let Some(filter) = measure
        .filters()
        .iter()
        .find(|filter| filter.column() == histogram.column())
    {
        let low = pixel_x(inner, histogram.low(), histogram.high(), filter.minimum());
        let high = pixel_x(inner, histogram.low(), histogram.high(), filter.maximum());
        gate(&mut painter, inner, low, high, accent);
    }
    for bin in 0..bins {
        let total = histogram.total(bin);
        if total == 0 {
            continue;
        }
        let x = inner.min.x + width * bin as f32;
        let bar = (width - gap).max(0.5);
        let height = total as f32 / peak as f32 * inner.height();
        painter.rect(
            Rect::from_min_max(
                Vec2::new(x, inner.max.y - height),
                Vec2::new(x + bar, inner.max.y),
            ),
            Corners::top(1.5),
            Color::white_alpha(0.14),
        );
        let gated = histogram.gated(bin);
        if gated > 0 {
            let height = gated as f32 / peak as f32 * inner.height();
            painter.rect(
                Rect::from_min_max(
                    Vec2::new(x, inner.max.y - height),
                    Vec2::new(x + bar, inner.max.y),
                ),
                Corners::top(1.5),
                accent.with_alpha(0.85),
            );
        }
    }
    if let Some((low, high)) = brush {
        let low = pixel_x(inner, histogram.low(), histogram.high(), low);
        let high = pixel_x(inner, histogram.low(), histogram.high(), high);
        gate(&mut painter, inner, low, high, accent);
        painter.rect(
            Rect::from_min_max(Vec2::new(low, inner.min.y), Vec2::new(high, inner.max.y)),
            Corners::ZERO,
            accent.with_alpha(0.06),
        );
    }
    if let Some(row) = selected_row(table, measure.rows(), state.scene, measure.set()) {
        let x = pixel_x(
            inner,
            histogram.low(),
            histogram.high(),
            table.value(row, histogram.column()),
        );
        painter.segment(
            Vec2::new(x, inner.min.y),
            Vec2::new(x, inner.max.y),
            1.4,
            accent,
        );
        painter.rect(
            Rect::from_center_size(Vec2::new(x, inner.max.y - 4.0), Vec2::splat(5.0)),
            Corners::capsule(),
            accent,
        );
    }
    if let Some(pointer) = hovered {
        painter.segment(
            Vec2::new(pointer.x, inner.min.y),
            Vec2::new(pointer.x, inner.max.y),
            0.6,
            Color::white_alpha(0.12),
        );
        let position = (pointer.x - inner.min.x) / inner.width();
        let bin = ((position * bins as f32) as usize).min(bins - 1);
        let (low, high) = histogram.bin_range(bin);
        readout = Readout::Bin {
            low,
            high,
            total: histogram.total(bin),
            gated: histogram.gated(bin),
        };
    }
    if histogram.peak() == 0 {
        painter.text_in(inner, "No objects to plot", empty, Align::Center);
    }
    readout
}

fn scatter_view(
    ui: &mut Ui,
    state: &mut State<'_>,
    commands: &mut Vec<Command>,
    scatter: &Scatter,
    inner: Rect,
    table: &Table,
) -> Readout {
    if inner.is_empty() {
        return Readout::None;
    }
    let measure = state.measure;
    let key = ui.id("plot.scatter");
    let response = ui.interact(key, inner);
    if response.hovered {
        ui.set_cursor(Cursor::Crosshair);
    }
    let pointer = ui
        .input()
        .position()
        .clamp(inner.min, inner.max - Vec2::splat(1.0));
    let released = ui.input().button(Button::Primary).released;
    let clicked = response.clicked;
    let began = response.active && ui.input().button(Button::Primary).pressed;
    let shift = ui.input().modifiers().shift;
    let mut drag = std::mem::take(&mut state.workspace.drag);
    if began && ui.pointer_in(inner).is_some() {
        drag.view = Some(MeasureView::Scatter);
        drag.gate = Some(if shift {
            PlotGate::Rectangle
        } else {
            PlotGate::Lasso
        });
        drag.origin = Some(pointer);
        drag.moved = false;
        drag.vertices.clear();
    }
    let mut brush = None;
    if let Some(origin) = drag
        .origin
        .filter(|_| drag.view == Some(MeasureView::Scatter))
    {
        if pointer.distance(origin) > BRUSH_SLOP {
            drag.moved = true;
        }
        let from_x = value_at(inner, scatter.x_low(), scatter.x_high(), origin.x);
        let from_y = value_at_y(inner, scatter.y_low(), scatter.y_high(), origin.y);
        let to_x = value_at(inner, scatter.x_low(), scatter.x_high(), pointer.x);
        let to_y = value_at_y(inner, scatter.y_low(), scatter.y_high(), pointer.y);
        let lassoing = drag.gate == Some(PlotGate::Lasso);
        if drag.moved && lassoing {
            push_vertex(&mut drag.vertices, Vec2::new(to_x, to_y));
        } else if drag.moved {
            brush = Some((
                from_x.min(to_x),
                from_x.max(to_x),
                from_y.min(to_y),
                from_y.max(to_y),
            ));
        }
        if released {
            if drag.moved {
                if lassoing {
                    push_vertex(&mut drag.vertices, Vec2::new(to_x, to_y));
                    let points = drag
                        .vertices
                        .iter()
                        .map(|vertex| [vertex.x, vertex.y])
                        .collect::<Vec<_>>();
                    if let Some(lasso) = Lasso::new(scatter.x(), scatter.y(), points) {
                        commands.push(Command::SetLasso(Some(lasso)));
                    }
                } else if let Some((low_x, high_x, low_y, high_y)) = brush {
                    let width =
                        (high_x - low_x) / (scatter.x_high() - scatter.x_low()) * inner.width();
                    let height =
                        (high_y - low_y) / (scatter.y_high() - scatter.y_low()) * inner.height();
                    if width >= BRUSH_SLOP && height >= BRUSH_SLOP {
                        commands.push(Command::SetGate {
                            column: scatter.x(),
                            minimum: low_x,
                            maximum: high_x,
                        });
                        commands.push(Command::SetGate {
                            column: scatter.y(),
                            minimum: low_y,
                            maximum: high_y,
                        });
                    }
                }
            }
            drag.origin = None;
            drag.moved = false;
        }
    }
    let draft = (drag.moved && drag.gate == Some(PlotGate::Lasso)).then(|| {
        (
            drag.vertices.clone(),
            Vec2::new(
                value_at(inner, scatter.x_low(), scatter.x_high(), pointer.x),
                value_at_y(inner, scatter.y_low(), scatter.y_high(), pointer.y),
            ),
        )
    });
    state.workspace.drag = drag;
    if clicked
        && let Some(row) = table.nearest(
            measure.rows(),
            &[scatter.x(), scatter.y()],
            &[
                value_at(inner, scatter.x_low(), scatter.x_high(), pointer.x),
                value_at_y(inner, scatter.y_low(), scatter.y_high(), pointer.y),
            ],
            &[inner.width(), inner.height()],
        )
        && let Some(set) = measure.set()
    {
        measure_panel::locate(commands, set, table.observation(row));
    }
    let hovered = if brush.is_some() || draft.is_some() {
        None
    } else {
        ui.pointer_in(inner)
    };
    let accent = ui.theme().palette.accent;
    let font = ui.theme().font(grana_ui::Level::Monospace);
    let tick_color = ui.theme().palette.text_tertiary;
    let scale = (1.0 + scatter.peak().max(1) as f32).ln();
    let cell_width = inner.width() / SCATTER_BINS as f32;
    let cell_height = inner.height() / SCATTER_BINS as f32;
    let points = measure.rows().len() as u64 <= POINT_LIMIT;
    let lasso = measure
        .lasso()
        .filter(|lasso| lasso.x() == scatter.x() && lasso.y() == scatter.y());
    let lassoed = lasso.is_some();
    let empty = TextStyle::new(
        ui.theme().font(grana_ui::Level::Caption),
        ui.theme().palette.text_tertiary,
    )
    .aligned(Align::Center);
    let mut readout = Readout::None;
    let mut painter = ui.painter();
    axis_grid(
        &mut painter,
        inner,
        (scatter.x_low(), scatter.x_high()),
        (scatter.y_low(), scatter.y_high()),
        font,
        tick_color,
    );
    let x_gate = measure
        .filters()
        .iter()
        .find(|filter| filter.column() == scatter.x());
    let y_gate = measure
        .filters()
        .iter()
        .find(|filter| filter.column() == scatter.y());
    if let (Some(x_gate), Some(y_gate)) = (x_gate, y_gate) {
        let rect = Rect::from_min_max(
            Vec2::new(
                pixel_x(inner, scatter.x_low(), scatter.x_high(), x_gate.minimum()),
                pixel_y(inner, scatter.y_low(), scatter.y_high(), y_gate.maximum()),
            ),
            Vec2::new(
                pixel_x(inner, scatter.x_low(), scatter.x_high(), x_gate.maximum()),
                pixel_y(inner, scatter.y_low(), scatter.y_high(), y_gate.minimum()),
            ),
        );
        painter.outlined(
            rect,
            Corners::ZERO,
            Fill::Solid(accent.with_alpha(0.05)),
            Border {
                width: 0.75,
                color: accent.with_alpha(0.32),
            },
            Shadow::NONE,
        );
    } else if let Some(x_gate) = x_gate {
        let low = pixel_x(inner, scatter.x_low(), scatter.x_high(), x_gate.minimum());
        let high = pixel_x(inner, scatter.x_low(), scatter.x_high(), x_gate.maximum());
        painter.rect(
            Rect::from_min_max(Vec2::new(low, inner.min.y), Vec2::new(high, inner.max.y)),
            Corners::ZERO,
            accent.with_alpha(0.05),
        );
    } else if let Some(y_gate) = y_gate {
        let low = pixel_y(inner, scatter.y_low(), scatter.y_high(), y_gate.maximum());
        let high = pixel_y(inner, scatter.y_low(), scatter.y_high(), y_gate.minimum());
        painter.rect(
            Rect::from_min_max(Vec2::new(inner.min.x, low), Vec2::new(inner.max.x, high)),
            Corners::ZERO,
            accent.with_alpha(0.05),
        );
    }
    let dim = |alpha: f32| Color::white_alpha(if lassoed { alpha * 0.5 } else { alpha });
    if points {
        for row in 0..SCATTER_BINS {
            for column in 0..SCATTER_BINS {
                if scatter.total_at(column, row) == 0 {
                    continue;
                }
                let centre = cell_centre(inner, column, row, cell_width, cell_height);
                let dot = cell_width.min(cell_height).clamp(1.8, 3.0);
                painter.rect(
                    Rect::from_center_size(centre, Vec2::splat(dot)),
                    Corners::capsule(),
                    dim(0.5),
                );
            }
        }
        for row in 0..SCATTER_BINS {
            for column in 0..SCATTER_BINS {
                if scatter.gated_at(column, row) == 0 {
                    continue;
                }
                let centre = cell_centre(inner, column, row, cell_width, cell_height);
                let dot = cell_width.min(cell_height).clamp(2.2, 3.4);
                painter.rect(
                    Rect::from_center_size(centre, Vec2::splat(dot)),
                    Corners::capsule(),
                    accent.with_alpha(0.9),
                );
            }
        }
    } else {
        for row in 0..SCATTER_BINS {
            for column in 0..SCATTER_BINS {
                let total = scatter.total_at(column, row);
                if total == 0 {
                    continue;
                }
                let centre = cell_centre(inner, column, row, cell_width, cell_height);
                let weight = ((1.0 + total as f32).ln() / scale).clamp(0.0, 1.0);
                painter.rect(
                    Rect::from_center_size(centre, Vec2::new(cell_width + 0.5, cell_height + 0.5)),
                    Corners::ZERO,
                    dim(0.04 + 0.34 * weight),
                );
                let gated = scatter.gated_at(column, row);
                if gated > 0 {
                    let weight = ((1.0 + gated as f32).ln() / scale).clamp(0.0, 1.0);
                    painter.rect(
                        Rect::from_center_size(
                            centre,
                            Vec2::new(cell_width * 0.7, cell_height * 0.7),
                        ),
                        Corners::ZERO,
                        accent.with_alpha(0.22 + 0.68 * weight),
                    );
                }
            }
        }
    }
    if let Some(lasso) = lasso {
        lasso_outline(
            &mut painter,
            inner,
            (scatter.x_low(), scatter.x_high()),
            (scatter.y_low(), scatter.y_high()),
            lasso.points(),
            accent,
        );
    }
    if let Some((vertices, last)) = draft {
        let mut points = vertices;
        points.push(last);
        lasso_outline(
            &mut painter,
            inner,
            (scatter.x_low(), scatter.x_high()),
            (scatter.y_low(), scatter.y_high()),
            &points.iter().map(|v| [v.x, v.y]).collect::<Vec<_>>(),
            accent.with_alpha(0.8),
        );
    }
    if let Some((low_x, high_x, low_y, high_y)) = brush {
        let rect = Rect::from_min_max(
            Vec2::new(
                pixel_x(inner, scatter.x_low(), scatter.x_high(), low_x),
                pixel_y(inner, scatter.y_low(), scatter.y_high(), high_y),
            ),
            Vec2::new(
                pixel_x(inner, scatter.x_low(), scatter.x_high(), high_x),
                pixel_y(inner, scatter.y_low(), scatter.y_high(), low_y),
            ),
        );
        painter.outlined(
            rect,
            Corners::ZERO,
            Fill::Solid(accent.with_alpha(0.10)),
            Border {
                width: 0.75,
                color: accent.with_alpha(0.55),
            },
            Shadow::NONE,
        );
    }
    if let Some(row) = selected_row(table, measure.rows(), state.scene, measure.set()) {
        let centre = Vec2::new(
            pixel_x(
                inner,
                scatter.x_low(),
                scatter.x_high(),
                table.value(row, scatter.x()),
            ),
            pixel_y(
                inner,
                scatter.y_low(),
                scatter.y_high(),
                table.value(row, scatter.y()),
            ),
        );
        painter.outlined(
            Rect::from_center_size(centre, Vec2::splat(9.0)),
            Corners::capsule(),
            Fill::Solid(Color::white_alpha(0.12)),
            Border {
                width: 1.4,
                color: accent,
            },
            Shadow::NONE,
        );
    }
    if let Some(pointer) = hovered {
        painter.segment(
            Vec2::new(pointer.x, inner.min.y),
            Vec2::new(pointer.x, inner.max.y),
            0.6,
            Color::white_alpha(0.10),
        );
        painter.segment(
            Vec2::new(inner.min.x, pointer.y),
            Vec2::new(inner.max.x, pointer.y),
            0.6,
            Color::white_alpha(0.10),
        );
        let x = value_at(inner, scatter.x_low(), scatter.x_high(), pointer.x);
        let y = value_at_y(inner, scatter.y_low(), scatter.y_high(), pointer.y);
        let observation = table
            .nearest(
                measure.rows(),
                &[scatter.x(), scatter.y()],
                &[x, y],
                &[inner.width(), inner.height()],
            )
            .filter(|row| {
                let centre = Vec2::new(
                    pixel_x(
                        inner,
                        scatter.x_low(),
                        scatter.x_high(),
                        table.value(*row, scatter.x()),
                    ),
                    pixel_y(
                        inner,
                        scatter.y_low(),
                        scatter.y_high(),
                        table.value(*row, scatter.y()),
                    ),
                );
                centre.distance(pointer) <= HOVER_REACH
            })
            .map(|row| table.observation(row));
        readout = Readout::Cell { x, y, observation };
    }
    if scatter.peak() == 0 {
        painter.text_in(inner, "No objects to plot", empty, Align::Center);
    }
    readout
}

fn push_vertex(vertices: &mut Vec<Vec2>, vertex: Vec2) {
    if vertices
        .last()
        .is_none_or(|last| last.distance(vertex) > LASSO_STEP)
    {
        vertices.push(vertex);
    }
}

fn lasso_outline(
    painter: &mut Painter<'_>,
    inner: Rect,
    x: (f32, f32),
    y: (f32, f32),
    points: &[[f32; 2]],
    accent: Color,
) {
    if points.len() < 2 {
        return;
    }
    let mut previous = pixel_of(inner, x, y, points[points.len() - 1]);
    for point in points {
        let current = pixel_of(inner, x, y, *point);
        painter.segment(previous, current, 4.0, accent.with_alpha(0.10));
        painter.segment(previous, current, 1.0, accent.with_alpha(0.75));
        previous = current;
    }
}

fn pixel_of(inner: Rect, x: (f32, f32), y: (f32, f32), point: [f32; 2]) -> Vec2 {
    Vec2::new(
        pixel_x(inner, x.0, x.1, point[0]),
        pixel_y(inner, y.0, y.1, point[1]),
    )
}

fn selected_row(
    table: &Table,
    rows: &[usize],
    scene: &Scene,
    set: Option<ObjectSetId>,
) -> Option<usize> {
    let selection = scene.selection()?;
    if Some(selection.set) != set {
        return None;
    }
    rows.iter().copied().find(|row| {
        let observation = table.observation(*row);
        match (
            observation.time(),
            observation.object(),
            observation.track(),
        ) {
            (Some(time), Some(object), _) => {
                scene.time() == time && selection.item == Item::Object(object)
            }
            (_, _, Some(track)) => selection.item == Item::Track(track),
            _ => false,
        }
    })
}

fn cell_centre(inner: Rect, column: usize, row: usize, width: f32, height: f32) -> Vec2 {
    Vec2::new(
        inner.min.x + width * (column as f32 + 0.5),
        inner.max.y - height * (row as f32 + 0.5),
    )
}

fn gate(painter: &mut Painter<'_>, inner: Rect, low: f32, high: f32, accent: Color) {
    painter.rect(
        Rect::from_min_max(Vec2::new(low, inner.min.y), Vec2::new(high, inner.max.y)),
        Corners::ZERO,
        accent.with_alpha(0.07),
    );
    for x in [low, high] {
        painter.segment(
            Vec2::new(x, inner.min.y),
            Vec2::new(x, inner.max.y),
            0.8,
            accent.with_alpha(0.35),
        );
    }
}

fn axis_grid(
    painter: &mut Painter<'_>,
    inner: Rect,
    x: (f32, f32),
    y: (f32, f32),
    font: TypeStyle,
    color: Color,
) {
    let style = TextStyle::new(font, color);
    let x_step = nice_step(x.1 - x.0, 5);
    let x_first = (x.0 / x_step - 1.0e-5).ceil();
    for offset in 0..16 {
        let value = (x_first + offset as f32) * x_step;
        if value > x.1 + x_step * 1.0e-4 {
            break;
        }
        let pixel = pixel_x(inner, x.0, x.1, value);
        painter.segment(
            Vec2::new(pixel, inner.min.y),
            Vec2::new(pixel, inner.max.y),
            0.5,
            Color::white_alpha(0.045),
        );
        let half = 36.0;
        let low = inner.min.x + half;
        let high = (inner.max.x - half).max(low);
        painter.text_in(
            Rect::from_center_size(
                Vec2::new(pixel.clamp(low, high), inner.max.y + TICK * 0.5),
                Vec2::new(72.0, TICK),
            ),
            &number(value, decimals(x_step)),
            style.aligned(Align::Center),
            Align::Center,
        );
    }
    let y_step = nice_step(y.1 - y.0, 4);
    let y_first = (y.0 / y_step - 1.0e-5).ceil();
    for offset in 0..12 {
        let value = (y_first + offset as f32) * y_step;
        if value > y.1 + y_step * 1.0e-4 {
            break;
        }
        let pixel = pixel_y(inner, y.0, y.1, value);
        painter.segment(
            Vec2::new(inner.min.x, pixel),
            Vec2::new(inner.max.x, pixel),
            0.5,
            Color::white_alpha(0.045),
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(inner.min.x - LEFT + 4.0, pixel - TICK * 0.5),
                Vec2::new(inner.min.x - 6.0, pixel + TICK * 0.5),
            ),
            &number(value, decimals(y_step)),
            style.aligned(Align::End),
            Align::Center,
        );
    }
}

fn value_at(inner: Rect, low: f32, high: f32, pixel: f32) -> f32 {
    let span = high - low;
    if span <= 0.0 {
        return low;
    }
    let position = ((pixel - inner.min.x) / inner.width().max(1.0)).clamp(0.0, 1.0);
    low + position * span
}

fn value_at_y(inner: Rect, low: f32, high: f32, pixel: f32) -> f32 {
    let span = high - low;
    if span <= 0.0 {
        return low;
    }
    let position = ((inner.max.y - pixel) / inner.height().max(1.0)).clamp(0.0, 1.0);
    low + position * span
}

fn pixel_x(inner: Rect, low: f32, high: f32, value: f32) -> f32 {
    let span = high - low;
    if span <= 0.0 {
        return inner.min.x;
    }
    inner.min.x + ((value - low) / span).clamp(0.0, 1.0) * inner.width()
}

fn pixel_y(inner: Rect, low: f32, high: f32, value: f32) -> f32 {
    let span = high - low;
    if span <= 0.0 {
        return inner.max.y;
    }
    inner.max.y - ((value - low) / span).clamp(0.0, 1.0) * inner.height()
}

fn nice_step(range: f32, target: usize) -> f32 {
    if range <= 0.0 {
        return 1.0;
    }
    let raw = range / target.max(1) as f32;
    let magnitude = 10.0f32.powf(raw.log10().floor());
    let normalized = raw / magnitude;
    let step = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 2.5 {
        2.5
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    step * magnitude
}

fn decimals(step: f32) -> usize {
    for places in 0..=4 {
        let scaled = step * 10f32.powi(places as i32);
        if (scaled - scaled.round()).abs() < 1.0e-3 * scaled.max(1.0) {
            return places;
        }
    }
    4
}

fn number(value: f32, places: usize) -> String {
    let unit = 10f32.powi(-(places as i32));
    let value = if value.abs() <= 0.5 * unit {
        0.0
    } else {
        value
    };
    if value.abs() >= 100_000.0 {
        format!("{:.0}k", value / 1000.0)
    } else if value.abs() >= 10_000.0 {
        format!("{:.1}k", value / 1000.0)
    } else {
        format!("{value:.places$}")
    }
}
