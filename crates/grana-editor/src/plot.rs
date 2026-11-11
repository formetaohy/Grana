use glam::Vec2;
use grana_chart::{
    Axis, Brush, BrushKind, Decor, Gates, Heading, Highlight, Style, View as ChartView,
};
use grana_measure::{
    Distribution, Filter, Histogram, Lasso, MeasureView, Observation, Plot, Scatter, Table, Trend,
};
use grana_scene::{Item, Measure, ObjectSetId, Scene, Selection};
use grana_session::Command;
use grana_ui::{BuildContext, Cursor, Element, Key, Length, Phase, Rect, Role, State};

use crate::message::{Message, PlotEvent};
use crate::state::{PlotDrag, View, Workspace};

const POINT_LIMIT: u64 = 4096;
const BRUSH_SLOP: f32 = 0.005;
const LASSO_STEP: f32 = 0.002;
const HOVER_REACH: f32 = 11.0;
const SCATTER_HINT: &str = "Drag: Lasso · Shift-Drag: Box · Click: Locate";
const GATE_HINT: &str = "Drag: Gate · Click: Locate";

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
    Group(usize),
    Frame {
        time: u32,
        point: Option<usize>,
    },
}

struct PlotView<'a> {
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
    gate: Option<BrushKind>,
    moved: bool,
}

impl PlotView<'_> {
    fn decor(&self) -> Decor<'_> {
        Decor {
            gates: self.gates(),
            highlight: self.highlight(),
            pointer: self.hovering.then_some(self.pointer),
            lasso: self.lasso,
            brush: self.brush(),
            sparse: self.rows.len() as u64 <= POINT_LIMIT,
        }
    }

    fn gates(&self) -> Gates {
        match self.plot {
            Plot::Histogram(histogram) => Gates {
                x: range(self.filters, histogram.column()),
                y: None,
            },
            Plot::Scatter(scatter) => Gates {
                x: range(self.filters, scatter.x()),
                y: range(self.filters, scatter.y()),
            },
            Plot::Distribution(distribution) => Gates {
                x: range(self.filters, distribution.column()),
                y: None,
            },
            Plot::Trend(trend) => Gates {
                x: None,
                y: range(self.filters, trend.column()),
            },
        }
    }

    fn highlight(&self) -> Option<Highlight> {
        let row = self.selection?;
        match self.plot {
            Plot::Histogram(histogram) => {
                Some(Highlight::Value(self.table.value(row, histogram.column())))
            }
            Plot::Scatter(scatter) => Some(Highlight::Point(Vec2::new(
                self.table.value(row, scatter.x()),
                self.table.value(row, scatter.y()),
            ))),
            Plot::Distribution(distribution) => {
                let observation = self.table.observation(row);
                let value = self.table.value(row, distribution.column());
                group_index(distribution, observation)
                    .map(|index| Highlight::Group { index, value })
            }
            Plot::Trend(trend) => {
                let time = self.table.observation(row).time()?;
                Some(Highlight::Point(Vec2::new(
                    time as f32 + 1.0,
                    self.table.value(row, trend.column()),
                )))
            }
        }
    }

    fn brush(&self) -> Option<Brush<'_>> {
        if !self.moved {
            return None;
        }
        let from = self.origin?;
        Some(Brush {
            kind: self.gate?,
            from,
            to: self.pointer,
            points: self.vertices,
        })
    }
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
    let theme = *ctx.theme();
    let distribution = state.workspace.distribution_style;
    let style = Style::card(&theme, distribution);
    let frame = style.metrics.frame(rect);
    if rect.width() < 140.0 || rect.height() < 80.0 {
        return Vec::new();
    }
    let pointer = ctx.pointer();
    let drag = &state.workspace.drag;
    let active = drag.view == Some(plot.view()) && drag.moved && drag.origin.is_some();
    let hovering = frame.contains(pointer) && !active;
    let readout = match plot {
        Plot::Histogram(histogram) if hovering => histogram_readout(histogram, frame, pointer),
        Plot::Scatter(scatter) if hovering => {
            scatter_readout(scatter, table, measure, frame, pointer)
        }
        Plot::Distribution(distribution) if hovering => {
            distribution_readout(distribution, frame, pointer)
        }
        Plot::Trend(trend) if hovering => trend_readout(trend, frame, pointer),
        Plot::Histogram(_) | Plot::Scatter(_) | Plot::Distribution(_) | Plot::Trend(_) => {
            Readout::None
        }
    };
    let (title_y, title_x) = titles(plot, table);
    let heading =
        Heading::new("", title_y, title_x).with_summary(summary_text(plot, table, readout));
    let chart = PlotView {
        plot,
        table,
        rows: measure.rows(),
        filters: measure.filters(),
        lasso: measure.lasso(),
        selection: selected_row(table, measure.rows(), state.scene, measure.set()),
        pointer,
        hovering,
        origin: drag.origin.filter(|_| drag.view == Some(plot.view())),
        vertices: &drag.vertices,
        gate: drag.gate,
        moved: drag.moved,
    };
    let canvas = grana_ui::canvas(Length::Fill, rect.height(), move |painter, info| {
        let card = info.rect;
        let surface = theme.surface(Role::Card, State::Rest);
        painter.outlined(
            card,
            surface.corners,
            surface.fill,
            surface.border,
            surface.shadow,
        );
        let style = Style::card(&theme, distribution);
        let view = ChartView {
            heading: &heading,
            plot: chart.plot,
            style,
            decor: chart.decor(),
        };
        view.paint(painter, card);
    })
    .at(rect)
    .key(Key::new("plot.surface"))
    .cursor(Cursor::Crosshair)
    .on_pointer(move |pointer| {
        if pointer.phase == Phase::Down && !frame.contains(pointer.position) {
            return None;
        }
        Some(Message::Plot(PlotEvent {
            phase: pointer.phase,
            position: normalize(frame, pointer.position),
            shift: pointer.modifiers.shift,
        }))
    });
    vec![canvas]
}

fn normalize(frame: Rect, position: Vec2) -> Vec2 {
    Vec2::new(
        (position.x - frame.min.x) / frame.width().max(1.0),
        (frame.max.y - position.y) / frame.height().max(1.0),
    )
}

fn range(filters: &[Filter], column: usize) -> Option<(f32, f32)> {
    filters
        .iter()
        .find(|filter| filter.column() == column)
        .map(|filter| (filter.minimum(), filter.maximum()))
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
        Plot::Distribution(distribution) => (
            format!(
                "Y: {}",
                crate::view::column_label(&columns[distribution.column()])
            ),
            "X: Groups".to_string(),
        ),
        Plot::Trend(trend) => (
            format!("Y: {}", crate::view::column_label(&columns[trend.column()])),
            "X: Frame".to_string(),
        ),
    }
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
        (Plot::Distribution(distribution), Readout::Group(index)) => {
            let Some(group) = distribution.groups().get(index) else {
                return GATE_HINT.to_string();
            };
            let column = &table.columns()[distribution.column()];
            let places = column.decimals() as usize;
            let unit = column.unit().suffix();
            let gating = distribution.gated() < distribution.total();
            let series = if gating {
                group.gated()
            } else {
                Some(group.total())
            };
            let Some(series) = series else {
                return format!("{} · no gated samples · {GATE_HINT}", group.label());
            };
            let mut text = format!(
                "{} · n={} · Median {:.places$} {unit} · IQR {:.places$}–{:.places$} · Mean {:.places$} · σ {:.places$}",
                group.label(),
                series.count(),
                series.median(),
                series.lower_quartile(),
                series.upper_quartile(),
                series.mean(),
                series.deviation()
            );
            if series.count() < group.total().count() {
                text.push_str(&format!(
                    " · {} of {} gated",
                    series.count(),
                    group.total().count()
                ));
            }
            text.push_str(&format!(" · {GATE_HINT}"));
            text
        }
        (Plot::Distribution(distribution), _) => {
            let groups = distribution.groups().len();
            if distribution.total() == 0 {
                "No objects to plot".to_string()
            } else if distribution.gated() < distribution.total() {
                format!(
                    "{} of {} gated · {groups} groups · {GATE_HINT}",
                    distribution.gated(),
                    distribution.total()
                )
            } else {
                format!(
                    "{} objects · {groups} groups · {GATE_HINT}",
                    distribution.total()
                )
            }
        }
        (Plot::Trend(trend), Readout::Frame { time, point }) => {
            let column = &table.columns()[trend.column()];
            let places = column.decimals() as usize;
            let unit = column.unit().suffix();
            match point.and_then(|index| trend.points().get(index)) {
                Some(point) => format!(
                    "Frame {} · Mean {:.places$} {unit} · 95% CI {:.places$}–{:.places$} · n={} · {GATE_HINT}",
                    time + 1,
                    point.mean(),
                    point.lower(),
                    point.upper(),
                    point.count()
                ),
                None => format!("Frame {} · no samples · {GATE_HINT}", time + 1),
            }
        }
        (Plot::Trend(trend), _) => {
            if trend.points().is_empty() {
                "No frames to trend".to_string()
            } else if trend.gated() < trend.total() {
                format!(
                    "{} of {} values over {} frames · {GATE_HINT}",
                    trend.gated(),
                    trend.total(),
                    trend.frames()
                )
            } else {
                format!(
                    "{} values over {} frames · {GATE_HINT}",
                    trend.total(),
                    trend.frames()
                )
            }
        }
        (Plot::Histogram(histogram), _) => {
            let statistics = histogram.summary();
            let column = &table.columns()[histogram.column()];
            let places = column.decimals() as usize;
            let unit = column.unit().suffix();
            if statistics.count() == 0 {
                format!("{total} objects · Ungated · {GATE_HINT}")
            } else {
                format!(
                    "{} of {total} gated · Median {:.places$} {unit} · MAD {:.places$} · IQR {:.places$}–{:.places$} · Mean {:.places$} · σ {:.places$} · {GATE_HINT}",
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

fn histogram_readout(histogram: &Histogram, frame: Rect, pointer: Vec2) -> Readout {
    let bins = histogram.bins();
    if bins == 0 {
        return Readout::None;
    }
    let position = ((pointer.x - frame.min.x) / frame.width().max(1.0)).clamp(0.0, 1.0);
    let bin = ((position * bins as f32) as usize).min(bins - 1);
    let (low, high) = histogram.bin_range(bin);
    Readout::Bin {
        low,
        high,
        total: histogram.total(bin),
        gated: histogram.gated(bin),
    }
}

fn scatter_axes(scatter: &Scatter) -> (Axis, Axis) {
    (
        Axis::new(scatter.x_low(), scatter.x_high(), 5),
        Axis::new(scatter.y_low(), scatter.y_high(), 4),
    )
}

fn scatter_readout(
    scatter: &Scatter,
    table: &Table,
    measure: &Measure,
    frame: Rect,
    pointer: Vec2,
) -> Readout {
    let (x_axis, y_axis) = scatter_axes(scatter);
    let x = x_axis.value_at_x(frame, pointer.x);
    let y = y_axis.value_at_y(frame, pointer.y);
    let observation = table
        .nearest(
            measure.rows(),
            &[scatter.x(), scatter.y()],
            &[x, y],
            &[frame.width(), frame.height()],
        )
        .filter(|row| {
            let centre = Vec2::new(
                x_axis.x(frame, table.value(*row, scatter.x())),
                y_axis.y(frame, table.value(*row, scatter.y())),
            );
            centre.distance(pointer) <= HOVER_REACH
        })
        .map(|row| table.observation(row));
    Readout::Cell { x, y, observation }
}

fn distribution_readout(distribution: &Distribution, frame: Rect, pointer: Vec2) -> Readout {
    match grana_chart::group_at(frame, distribution.groups().len(), pointer) {
        Some(index) => Readout::Group(index),
        None => Readout::None,
    }
}

fn trend_axis(trend: &Trend) -> Axis {
    Axis::frames(1.0, trend.frames().max(2) as f32, 5)
}

fn trend_readout(trend: &Trend, frame: Rect, pointer: Vec2) -> Readout {
    let value = trend_axis(trend).value_at_x(frame, pointer.x);
    let last = trend.frames().max(1);
    let time = ((value.round() as i64) - 1).clamp(0, last as i64 - 1) as u32;
    let point = trend.points().iter().position(|point| point.time() == time);
    Readout::Frame { time, point }
}

fn group_index(distribution: &Distribution, observation: Observation) -> Option<usize> {
    let groups = distribution.groups();
    if groups.len() == 1 {
        return Some(0);
    }
    let time = observation.time()?;
    groups.iter().position(|group| group.time() == Some(time))
}

fn push_vertex(vertices: &mut Vec<Vec2>, vertex: Vec2) {
    if vertices
        .last()
        .is_none_or(|last| last.distance(vertex) > LASSO_STEP)
    {
        vertices.push(vertex);
    }
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

pub fn apply(
    event: PlotEvent,
    measure: &Measure,
    workspace: &mut Workspace,
    commands: &mut Vec<Command>,
) {
    let (Some(table), Some(plot)) = (measure.table(), measure.plot()) else {
        return;
    };
    match plot {
        Plot::Histogram(histogram) => {
            histogram_apply(event, measure, table, histogram, workspace, commands)
        }
        Plot::Scatter(scatter) => {
            scatter_apply(event, measure, table, scatter, workspace, commands)
        }
        Plot::Distribution(distribution) => {
            distribution_apply(event, measure, table, distribution, workspace, commands)
        }
        Plot::Trend(trend) => trend_apply(event, measure, table, trend, workspace, commands),
    }
}

fn histogram_apply(
    event: PlotEvent,
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
            workspace.drag.gate = Some(BrushKind::Rectangle);
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
                BrushKind::Rectangle
            } else {
                BrushKind::Lasso
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
            if workspace.drag.moved && workspace.drag.gate == Some(BrushKind::Lasso) {
                push_vertex(&mut workspace.drag.vertices, position);
            }
        }
        Phase::Up => {
            let Some(origin) = workspace.drag.origin else {
                return;
            };
            let moved = workspace.drag.moved || origin.distance(position) > BRUSH_SLOP;
            if moved {
                if workspace.drag.gate == Some(BrushKind::Lasso) {
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

fn distribution_apply(
    event: PlotEvent,
    measure: &Measure,
    table: &Table,
    distribution: &Distribution,
    workspace: &mut Workspace,
    commands: &mut Vec<Command>,
) {
    if workspace
        .drag
        .view
        .is_some_and(|view| view != MeasureView::Distribution)
    {
        workspace.drag = PlotDrag::default();
    }
    let raw = event.position;
    let position = raw.clamp(Vec2::ZERO, Vec2::ONE);
    match event.phase {
        Phase::Down => {
            workspace.drag.view = Some(MeasureView::Distribution);
            workspace.drag.gate = Some(BrushKind::Rectangle);
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
                if (position.y - origin.y).abs() >= BRUSH_SLOP {
                    let from = distribution_value(distribution, origin.y);
                    let to = distribution_value(distribution, position.y);
                    commands.push(Command::SetGate {
                        column: distribution.column(),
                        minimum: from.min(to),
                        maximum: from.max(to),
                    });
                }
            } else if inside(raw)
                && let Some(set) = measure.set()
                && let Some(group) = group_from_position(distribution, position.x)
                && let Some(row) = nearest_group_row(
                    table,
                    measure.rows(),
                    distribution,
                    group,
                    distribution_value(distribution, position.y),
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

fn distribution_value(distribution: &Distribution, position: f32) -> f32 {
    let low = distribution.low();
    low + position.clamp(0.0, 1.0) * (distribution.high() - low)
}

fn trend_apply(
    event: PlotEvent,
    measure: &Measure,
    table: &Table,
    trend: &Trend,
    workspace: &mut Workspace,
    commands: &mut Vec<Command>,
) {
    if workspace
        .drag
        .view
        .is_some_and(|view| view != MeasureView::Trend)
    {
        workspace.drag = PlotDrag::default();
    }
    let raw = event.position;
    let position = raw.clamp(Vec2::ZERO, Vec2::ONE);
    match event.phase {
        Phase::Down => {
            workspace.drag.view = Some(MeasureView::Trend);
            workspace.drag.gate = Some(BrushKind::Rectangle);
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
                if (position.y - origin.y).abs() >= BRUSH_SLOP {
                    let from = trend_value(trend, origin.y);
                    let to = trend_value(trend, position.y);
                    commands.push(Command::SetGate {
                        column: trend.column(),
                        minimum: from.min(to),
                        maximum: from.max(to),
                    });
                }
            } else if inside(raw)
                && let Some(set) = measure.set()
                && let Some((time, value)) = trend_target(trend, position)
                && let Some(row) = nearest_trend_row(table, measure.rows(), trend, time, value)
            {
                locate_observation(set, table.observation(row), commands);
            }
            workspace.drag.origin = None;
            workspace.drag.moved = false;
            workspace.drag.vertices.clear();
        }
    }
}

fn trend_value(trend: &Trend, position: f32) -> f32 {
    let low = trend.low();
    low + position.clamp(0.0, 1.0) * (trend.high() - low)
}

fn trend_target(trend: &Trend, position: Vec2) -> Option<(u32, f32)> {
    let frames = trend.frames();
    if frames == 0 {
        return None;
    }
    let value = 1.0 + position.x.clamp(0.0, 1.0) * (frames.max(2) as f32 - 1.0);
    let time = ((value.round() as i64) - 1).clamp(0, frames as i64 - 1) as u32;
    Some((time, trend_value(trend, position.y)))
}

fn nearest_trend_row(
    table: &Table,
    rows: &[usize],
    trend: &Trend,
    time: u32,
    value: f32,
) -> Option<usize> {
    let mut best = None;
    let mut closest = f32::INFINITY;
    for row in rows {
        if table.observation(*row).time() != Some(time) {
            continue;
        }
        let current = table.value(*row, trend.column());
        if !current.is_finite() {
            continue;
        }
        let distance = (current - value).abs();
        if distance < closest {
            closest = distance;
            best = Some(*row);
        }
    }
    best
}

fn group_from_position(distribution: &Distribution, position: f32) -> Option<usize> {
    let count = distribution.groups().len();
    if count == 0 {
        return None;
    }
    Some(
        (position.clamp(0.0, 1.0) * count as f32)
            .floor()
            .min(count as f32 - 1.0) as usize,
    )
}

fn nearest_group_row(
    table: &Table,
    rows: &[usize],
    distribution: &Distribution,
    group: usize,
    value: f32,
) -> Option<usize> {
    let groups = distribution.groups();
    let target = groups.get(group)?.time();
    let split = groups.len() > 1;
    let column = distribution.column();
    let mut best = None;
    let mut closest = f32::INFINITY;
    for row in rows {
        if split && table.observation(*row).time() != target {
            continue;
        }
        let current = table.value(*row, column);
        if !current.is_finite() {
            continue;
        }
        let distance = (current - value).abs();
        if distance < closest {
            closest = distance;
            best = Some(*row);
        }
    }
    best
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

pub fn locate(measure: &Measure, row: usize, commands: &mut Vec<Command>) {
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
    let view = measure.view();
    let gated = |column: usize| match view {
        MeasureView::Table => false,
        MeasureView::Histogram => column == axes[0],
        MeasureView::Scatter => column == axes[0] || column == axes[1],
        MeasureView::Distribution => column == axes[0],
        MeasureView::Trend => column == axes[0],
    };
    let mut indices = measure
        .filters()
        .iter()
        .enumerate()
        .filter(|(_, filter)| gated(filter.column()))
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
