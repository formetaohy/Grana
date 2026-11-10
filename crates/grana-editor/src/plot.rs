use glam::Vec2;
use grana_measure::{
    Filter, Histogram, Lasso, MeasureView, Observation, Plot, SCATTER_BINS, Scatter, Table,
};
use grana_scene::{Item, Measure, ObjectSetId, Scene, Selection};
use grana_session::Command;
use grana_ui::{
    Align, Border, BuildContext, Color, Corners, Cursor, Element, Fill, Key, Length, Level, Phase,
    Rect, Role, Shadow, State, TextStyle, TypeStyle,
};

use crate::message::{Message, PlotEvent};
use crate::state::{PlotDrag, PlotGate, View, Workspace};

const LEFT: f32 = 44.0;
const RIGHT: f32 = 14.0;
const TOP: f32 = 28.0;
const TICK: f32 = 18.0;
const SUMMARY: f32 = 22.0;
const POINT_LIMIT: u64 = 4096;
const BRUSH_SLOP: f32 = 0.005;
const LASSO_STEP: f32 = 0.002;
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

struct Chart<'a> {
    theme: grana_ui::Theme,
    plot: &'a Plot,
    table: &'a Table,
    rows: &'a [usize],
    filters: &'a [Filter],
    lasso: Option<&'a Lasso>,
    selection: Option<usize>,
    pointer: Vec2,
    hovering: bool,
    origin: Option<Vec2>,
    vertices: &'a [Vec2],
    gate: Option<PlotGate>,
    moved: bool,
}

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    rect: Rect,
) -> Vec<Element<'a, Message>> {
    let measure = state.measure;
    let (Some(table), Some(plot)) = (measure.table(), measure.plot()) else {
        return Vec::new();
    };
    if rect.width() < 140.0 || rect.height() < 80.0 {
        return Vec::new();
    }
    let inner = inner_rect(rect);
    let pointer = ctx.pointer();
    let drag = &state.workspace.drag;
    let active = drag.view == Some(plot.view()) && drag.moved && drag.origin.is_some();
    let hovering = inner.contains(pointer) && !active;
    let readout = match plot {
        Plot::Histogram(histogram) if hovering => histogram_readout(histogram, pointer, inner),
        Plot::Scatter(scatter) if hovering => {
            scatter_readout(scatter, table, measure, pointer, inner)
        }
        Plot::Histogram(_) | Plot::Scatter(_) => Readout::None,
    };
    let summary = summary_text(plot, table, readout);
    let (title_y, title_x) = titles(plot, table);
    let origin = drag.origin.filter(|_| drag.view == Some(plot.view()));
    let chart = Chart {
        theme: *ctx.theme(),
        plot,
        table,
        rows: measure.rows(),
        filters: measure.filters(),
        lasso: measure.lasso(),
        selection: selected_row(table, measure.rows(), state.scene, measure.set()),
        pointer,
        hovering,
        origin,
        vertices: &drag.vertices,
        gate: drag.gate,
        moved: drag.moved,
    };
    let node_key = Key::new("plot.surface");
    let canvas = grana_ui::canvas(Length::Fill, rect.height(), move |painter, info| {
        let card = info.rect;
        let inner = inner_rect(card);
        let surface = chart.theme.surface(Role::Card, State::Rest);
        painter.outlined(
            card,
            surface.corners,
            surface.fill,
            surface.border,
            surface.shadow,
        );
        titles_paint(painter, &chart.theme, card, &title_y, &title_x);
        summary_paint(painter, &chart.theme, card, &summary);
        match chart.plot {
            Plot::Histogram(histogram) => histogram_paint(painter, &chart, histogram, inner),
            Plot::Scatter(scatter) => scatter_paint(painter, &chart, scatter, inner),
        }
    })
    .at(rect)
    .key(node_key)
    .cursor(Cursor::Crosshair)
    .on_pointer(move |pointer| {
        if pointer.phase == Phase::Down && !inner.contains(pointer.position) {
            return None;
        }
        Some(Message::Plot(PlotEvent {
            phase: pointer.phase,
            position: normalize(inner, pointer.position),
            shift: pointer.modifiers.shift,
        }))
    });
    vec![canvas]
}

fn inner_rect(card: Rect) -> Rect {
    Rect::from_min_max(
        card.min + Vec2::new(LEFT, TOP),
        Vec2::new(card.max.x - RIGHT, card.max.y - SUMMARY - TICK),
    )
}

fn normalize(inner: Rect, position: Vec2) -> Vec2 {
    Vec2::new(
        (position.x - inner.min.x) / inner.width().max(1.0),
        (inner.max.y - position.y) / inner.height().max(1.0),
    )
}

fn titles(plot: &Plot, table: &Table) -> (String, String) {
    let columns = table.columns();
    match plot {
        Plot::Histogram(histogram) => (
            "Y: Objects".to_string(),
            format!(
                "X: {}",
                crate::view::column_label(&columns[histogram.column()])
            ),
        ),
        Plot::Scatter(scatter) => (
            format!("Y: {}", crate::view::column_label(&columns[scatter.y()])),
            format!("X: {}", crate::view::column_label(&columns[scatter.x()])),
        ),
    }
}

fn titles_paint(
    painter: &mut grana_ui::Painter<'_>,
    theme: &grana_ui::Theme,
    card: Rect,
    title_y: &str,
    title_x: &str,
) {
    let style = TextStyle::new(
        TypeStyle::new(10.5, 500.0, 14.0),
        theme.palette.text_tertiary,
    );
    let strip = Rect::from_min_max(
        card.min + Vec2::new(12.0, 4.0),
        Vec2::new(card.max.x - 12.0, card.min.y + TOP),
    );
    painter.text_in(strip, title_y, style, Align::Center);
    painter.text_in(strip, title_x, style.aligned(Align::End), Align::Center);
}

fn summary_paint(
    painter: &mut grana_ui::Painter<'_>,
    theme: &grana_ui::Theme,
    card: Rect,
    text: &str,
) {
    let style = TextStyle::new(
        TypeStyle::new(10.5, 400.0, 14.0),
        theme.palette.text_tertiary,
    )
    .truncating();
    let rect = Rect::from_min_max(
        Vec2::new(card.min.x + 12.0, card.max.y - SUMMARY),
        card.max - Vec2::new(12.0, 2.0),
    );
    painter.text_in(rect, text, style, Align::Center);
}

fn summary_text(plot: &Plot, table: &Table, readout: Readout) -> String {
    let total = table.rows() as u64;
    match (plot, readout) {
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
                    "{} of {total} gated · Median {:.places$} {unit} · MAD {:.places$} · IQR {:.places$}–{:.places$} · Mean {:.places$} · σ {:.places$} · {HISTOGRAM_HINT}",
                    statistics.count(),
                    statistics.median(),
                    statistics.mad(),
                    statistics.lower_quartile(),
                    statistics.upper_quartile(),
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
    }
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

fn histogram_readout(histogram: &Histogram, pointer: Vec2, inner: Rect) -> Readout {
    let bins = histogram.bins();
    if bins == 0 {
        return Readout::None;
    }
    let position = ((pointer.x - inner.min.x) / inner.width().max(1.0)).clamp(0.0, 1.0);
    let bin = ((position * bins as f32) as usize).min(bins - 1);
    let (low, high) = histogram.bin_range(bin);
    Readout::Bin {
        low,
        high,
        total: histogram.total(bin),
        gated: histogram.gated(bin),
    }
}

fn scatter_readout(
    scatter: &Scatter,
    table: &Table,
    measure: &Measure,
    pointer: Vec2,
    inner: Rect,
) -> Readout {
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
    Readout::Cell { x, y, observation }
}

fn histogram_paint(
    painter: &mut grana_ui::Painter<'_>,
    chart: &Chart<'_>,
    histogram: &Histogram,
    inner: Rect,
) {
    let theme = &chart.theme;
    let accent = theme.palette.accent;
    let tick_color = theme.palette.text_tertiary;
    let font = theme.font(Level::Monospace);
    let empty = TextStyle::new(theme.font(Level::Caption), theme.palette.text_tertiary)
        .aligned(Align::Center);
    let bins = histogram.bins();
    let peak = histogram.peak().max(1);
    let width = inner.width() / bins.max(1) as f32;
    let gap = if width > 4.0 { 1.0 } else { 0.0 };
    axis_grid(
        painter,
        inner,
        (histogram.low(), histogram.high()),
        (0.0, peak as f32),
        font,
        tick_color,
    );
    if let Some(filter) = chart
        .filters
        .iter()
        .find(|filter| filter.column() == histogram.column())
    {
        let low = pixel_x(inner, histogram.low(), histogram.high(), filter.minimum());
        let high = pixel_x(inner, histogram.low(), histogram.high(), filter.maximum());
        gate(painter, inner, low, high, accent);
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
    if let Some(origin) = chart.origin
        && chart.moved
    {
        let from = inner.min.x + origin.x.clamp(0.0, 1.0) * inner.width();
        let to = chart.pointer.x.clamp(inner.min.x, inner.max.x);
        let (low, high) = (from.min(to), from.max(to));
        gate(painter, inner, low, high, accent);
        painter.rect(
            Rect::from_min_max(Vec2::new(low, inner.min.y), Vec2::new(high, inner.max.y)),
            Corners::ZERO,
            accent.with_alpha(0.06),
        );
    }
    if let Some(row) = chart.selection {
        let x = pixel_x(
            inner,
            histogram.low(),
            histogram.high(),
            chart.table.value(row, histogram.column()),
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
    if chart.hovering {
        painter.segment(
            Vec2::new(chart.pointer.x, inner.min.y),
            Vec2::new(chart.pointer.x, inner.max.y),
            0.6,
            Color::white_alpha(0.12),
        );
    }
    if histogram.peak() == 0 {
        painter.text_in(inner, "No objects to plot", empty, Align::Center);
    }
}

fn scatter_paint(
    painter: &mut grana_ui::Painter<'_>,
    chart: &Chart<'_>,
    scatter: &Scatter,
    inner: Rect,
) {
    let theme = &chart.theme;
    let accent = theme.palette.accent;
    let font = theme.font(Level::Monospace);
    let tick_color = theme.palette.text_tertiary;
    let empty = TextStyle::new(theme.font(Level::Caption), theme.palette.text_tertiary)
        .aligned(Align::Center);
    let scale = (1.0 + scatter.peak().max(1) as f32).ln();
    let cell_width = inner.width() / SCATTER_BINS as f32;
    let cell_height = inner.height() / SCATTER_BINS as f32;
    let points = chart.rows.len() as u64 <= POINT_LIMIT;
    let lasso = chart
        .lasso
        .filter(|lasso| lasso.x() == scatter.x() && lasso.y() == scatter.y());
    let lassoed = lasso.is_some();
    axis_grid(
        painter,
        inner,
        (scatter.x_low(), scatter.x_high()),
        (scatter.y_low(), scatter.y_high()),
        font,
        tick_color,
    );
    let x_gate = chart
        .filters
        .iter()
        .find(|filter| filter.column() == scatter.x());
    let y_gate = chart
        .filters
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
            painter,
            inner,
            (scatter.x_low(), scatter.x_high()),
            (scatter.y_low(), scatter.y_high()),
            lasso.points(),
            accent,
        );
    }
    if let Some(origin) = chart.origin
        && chart.moved
    {
        let pointer = Vec2::new(
            chart.pointer.x.clamp(inner.min.x, inner.max.x),
            chart.pointer.y.clamp(inner.min.y, inner.max.y),
        );
        if chart.gate == Some(PlotGate::Lasso) {
            let mut pixels = chart
                .vertices
                .iter()
                .map(|vertex| {
                    Vec2::new(
                        inner.min.x + vertex.x.clamp(0.0, 1.0) * inner.width(),
                        inner.max.y - vertex.y.clamp(0.0, 1.0) * inner.height(),
                    )
                })
                .collect::<Vec<_>>();
            pixels.push(pointer);
            if pixels.len() >= 2 {
                let mut previous = pixels[pixels.len() - 1];
                for current in pixels {
                    painter.segment(previous, current, 4.0, accent.with_alpha(0.10));
                    painter.segment(previous, current, 1.0, accent.with_alpha(0.75));
                    previous = current;
                }
            }
        } else {
            let from = Vec2::new(
                inner.min.x + origin.x.clamp(0.0, 1.0) * inner.width(),
                inner.max.y - origin.y.clamp(0.0, 1.0) * inner.height(),
            );
            let rect = Rect::from_min_max(from.min(pointer), from.max(pointer));
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
    }
    if let Some(row) = chart.selection {
        let centre = Vec2::new(
            pixel_x(
                inner,
                scatter.x_low(),
                scatter.x_high(),
                chart.table.value(row, scatter.x()),
            ),
            pixel_y(
                inner,
                scatter.y_low(),
                scatter.y_high(),
                chart.table.value(row, scatter.y()),
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
    if chart.hovering {
        painter.segment(
            Vec2::new(chart.pointer.x, inner.min.y),
            Vec2::new(chart.pointer.x, inner.max.y),
            0.6,
            Color::white_alpha(0.10),
        );
        painter.segment(
            Vec2::new(inner.min.x, chart.pointer.y),
            Vec2::new(inner.max.x, chart.pointer.y),
            0.6,
            Color::white_alpha(0.10),
        );
    }
    if scatter.peak() == 0 {
        painter.text_in(inner, "No objects to plot", empty, Align::Center);
    }
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
    painter: &mut grana_ui::Painter<'_>,
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

fn gate(painter: &mut grana_ui::Painter<'_>, inner: Rect, low: f32, high: f32, accent: Color) {
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
    painter: &mut grana_ui::Painter<'_>,
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

pub fn apply(
    event: PlotEvent,
    scene: &Scene,
    measure: &Measure,
    workspace: &mut Workspace,
    commands: &mut Vec<Command>,
) {
    let (Some(table), Some(plot)) = (measure.table(), measure.plot()) else {
        return;
    };
    match plot {
        Plot::Histogram(histogram) => {
            histogram_apply(event, scene, measure, table, histogram, workspace, commands)
        }
        Plot::Scatter(scatter) => {
            scatter_apply(event, scene, measure, table, scatter, workspace, commands)
        }
    }
}

fn histogram_apply(
    event: PlotEvent,
    _scene: &Scene,
    measure: &Measure,
    table: &Table,
    histogram: &Histogram,
    workspace: &mut Workspace,
    commands: &mut Vec<Command>,
) {
    if workspace
        .drag
        .view
        .is_some_and(|view| view != MeasureView::Histogram)
    {
        workspace.drag = PlotDrag::default();
    }
    let raw = event.position;
    let position = raw.clamp(Vec2::ZERO, Vec2::ONE);
    match event.phase {
        Phase::Down => {
            workspace.drag.view = Some(MeasureView::Histogram);
            workspace.drag.gate = Some(PlotGate::Rectangle);
            workspace.drag.origin = Some(position);
            workspace.drag.moved = false;
            workspace.drag.vertices.clear();
        }
        Phase::Move => {
            if let Some(origin) = workspace.drag.origin
                && origin.distance(position) > BRUSH_SLOP
            {
                workspace.drag.moved = true;
            }
        }
        Phase::Up => {
            let Some(origin) = workspace.drag.origin else {
                return;
            };
            let moved = workspace.drag.moved || origin.distance(position) > BRUSH_SLOP;
            if moved {
                let from = histogram_value(histogram, origin.x);
                let to = histogram_value(histogram, position.x);
                if (position.x - origin.x).abs() >= BRUSH_SLOP {
                    commands.push(Command::SetGate {
                        column: histogram.column(),
                        minimum: from.min(to),
                        maximum: from.max(to),
                    });
                }
            } else if inside(raw)
                && let Some(set) = measure.set()
                && let Some(row) = table.nearest(
                    measure.rows(),
                    &[histogram.column()],
                    &[histogram_value(histogram, position.x)],
                    &[1.0],
                )
            {
                locate_observation(set, table.observation(row), commands);
            }
            workspace.drag.origin = None;
            workspace.drag.moved = false;
        }
    }
}

fn scatter_apply(
    event: PlotEvent,
    _scene: &Scene,
    measure: &Measure,
    table: &Table,
    scatter: &Scatter,
    workspace: &mut Workspace,
    commands: &mut Vec<Command>,
) {
    if workspace
        .drag
        .view
        .is_some_and(|view| view != MeasureView::Scatter)
    {
        workspace.drag = PlotDrag::default();
    }
    let raw = event.position;
    let position = raw.clamp(Vec2::ZERO, Vec2::ONE);
    match event.phase {
        Phase::Down => {
            workspace.drag.view = Some(MeasureView::Scatter);
            workspace.drag.gate = Some(if event.shift {
                PlotGate::Rectangle
            } else {
                PlotGate::Lasso
            });
            workspace.drag.origin = Some(position);
            workspace.drag.moved = false;
            workspace.drag.vertices.clear();
        }
        Phase::Move => {
            let Some(origin) = workspace.drag.origin else {
                return;
            };
            if origin.distance(position) > BRUSH_SLOP {
                workspace.drag.moved = true;
            }
            if workspace.drag.moved && workspace.drag.gate == Some(PlotGate::Lasso) {
                push_vertex(&mut workspace.drag.vertices, position);
            }
        }
        Phase::Up => {
            let Some(origin) = workspace.drag.origin else {
                return;
            };
            let moved = workspace.drag.moved || origin.distance(position) > BRUSH_SLOP;
            if moved {
                if workspace.drag.gate == Some(PlotGate::Lasso) {
                    push_vertex(&mut workspace.drag.vertices, position);
                    let points = workspace
                        .drag
                        .vertices
                        .iter()
                        .map(|vertex| [scatter_x(scatter, vertex.x), scatter_y(scatter, vertex.y)])
                        .collect::<Vec<_>>();
                    if let Some(lasso) = Lasso::new(scatter.x(), scatter.y(), points) {
                        commands.push(Command::SetLasso(Some(lasso)));
                    }
                } else {
                    let from_x = scatter_x(scatter, origin.x);
                    let to_x = scatter_x(scatter, position.x);
                    let from_y = scatter_y(scatter, origin.y);
                    let to_y = scatter_y(scatter, position.y);
                    if (position.x - origin.x).abs() >= BRUSH_SLOP
                        && (position.y - origin.y).abs() >= BRUSH_SLOP
                    {
                        commands.push(Command::SetGate {
                            column: scatter.x(),
                            minimum: from_x.min(to_x),
                            maximum: from_x.max(to_x),
                        });
                        commands.push(Command::SetGate {
                            column: scatter.y(),
                            minimum: from_y.min(to_y),
                            maximum: from_y.max(to_y),
                        });
                    }
                }
            } else if inside(raw)
                && let Some(set) = measure.set()
                && let Some(row) = table.nearest(
                    measure.rows(),
                    &[scatter.x(), scatter.y()],
                    &[
                        scatter_x(scatter, position.x),
                        scatter_y(scatter, position.y),
                    ],
                    &[1.0, 1.0],
                )
            {
                locate_observation(set, table.observation(row), commands);
            }
            workspace.drag.origin = None;
            workspace.drag.moved = false;
            workspace.drag.vertices.clear();
        }
    }
}

fn inside(position: Vec2) -> bool {
    (0.0..=1.0).contains(&position.x) && (0.0..=1.0).contains(&position.y)
}

fn histogram_value(histogram: &Histogram, position: f32) -> f32 {
    histogram.low() + position.clamp(0.0, 1.0) * (histogram.high() - histogram.low())
}

fn scatter_x(scatter: &Scatter, position: f32) -> f32 {
    scatter.x_low() + position.clamp(0.0, 1.0) * (scatter.x_high() - scatter.x_low())
}

fn scatter_y(scatter: &Scatter, position: f32) -> f32 {
    scatter.y_low() + position.clamp(0.0, 1.0) * (scatter.y_high() - scatter.y_low())
}

pub fn locate(_scene: &Scene, measure: &Measure, row: usize, commands: &mut Vec<Command>) {
    let Some(table) = measure.table() else {
        return;
    };
    let Some(&index) = measure.rows().get(row) else {
        return;
    };
    let Some(set) = measure.set() else {
        return;
    };
    locate_observation(set, table.observation(index), commands);
}

fn locate_observation(set: ObjectSetId, observation: Observation, commands: &mut Vec<Command>) {
    match (
        observation.time(),
        observation.object(),
        observation.track(),
    ) {
        (Some(time), Some(object), _) => {
            commands.push(Command::SetTime(time));
            commands.push(Command::Select(Some(Selection {
                set,
                item: Item::Object(object),
            })));
        }
        (_, _, Some(track)) => {
            commands.push(Command::Select(Some(Selection {
                set,
                item: Item::Track(track),
            })));
        }
        (Some(time), _, _) => commands.push(Command::SetTime(time)),
        _ => {}
    }
}

pub fn clear_gates(measure: &Measure, commands: &mut Vec<Command>) {
    let axes = measure.axes();
    let scatter = measure.view() == MeasureView::Scatter;
    let mut indices = measure
        .filters()
        .iter()
        .enumerate()
        .filter(|(_, filter)| filter.column() == axes[0] || (scatter && filter.column() == axes[1]))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    indices.sort_unstable_by(|left, right| right.cmp(left));
    for index in indices {
        commands.push(Command::RemoveFilter(index));
    }
    if measure.lasso().is_some() {
        commands.push(Command::SetLasso(None));
    }
}
