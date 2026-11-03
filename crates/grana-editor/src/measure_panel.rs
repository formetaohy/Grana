use glam::Vec2;
use grana_measure::{Aggregate, Column, Filter, Group, MeasureView, Observation, PlotAxis, Scope};
use grana_scene::{Item, ObjectSetId, Selection};
use grana_ui::{Align, Axis, Block, Color, Corners, Icon, Length, Rect, Role, TextStyle, Ui, icon};

use crate::panel::empty_state;
use crate::plot;
use crate::state::State;
use crate::widget;
use grana_session::Command;

const ROW_HEIGHT: f32 = 28.0;
const HEADER_HEIGHT: f32 = 30.0;
const MIN_COLUMN_WIDTH: f32 = 58.0;
const VIEW_WIDTH: f32 = 262.0;

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    if state.scene.objects.is_empty() {
        empty_state(
            ui,
            Icon::Table,
            "No measurements yet",
            "Add and detect an object set to build a measurement table, then filter, sort and export it.",
        );
        return;
    }
    header(ui, state, commands);
    let measured = state.measure.table().is_some();
    if measured {
        view_row(ui, state, commands);
    }
    filters(ui, state, commands);
    if measured && state.measure.view() != MeasureView::Table {
        plot::view(ui, state, commands);
    } else {
        table(ui, state, commands);
    }
}

fn view_row(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let Some(table) = state.measure.table() else {
        return;
    };
    let view = state.measure.view();
    let axes = state.measure.axes();
    let row = ui.allocate_full(30.0);
    let clear = Rect::from_center_size(
        Vec2::new(row.max.x - 12.0, row.center().y),
        Vec2::splat(24.0),
    );
    let actions = Rect::from_min_max(row.min, Vec2::new(clear.min.x - 8.0, row.max.y));
    let scope = state.measure.scope();
    let ranged = state.measure.group() != Group::Track;
    let scope_width = if ranged { 176.0 } else { 0.0 };
    let scope_rect = Rect::from_min_size(actions.min, Vec2::new(scope_width, row.height()));
    if ranged {
        ui.place(scope_rect, |ui| {
            ui.row(
                Block::row()
                    .gaps(6.0)
                    .align(Align::Center)
                    .height(Length::Fill),
                |ui| {
                    widget::field_label(ui, "RANGE");
                    let labels = Scope::ALL.map(Scope::label);
                    let index = Scope::ALL
                        .iter()
                        .position(|current| *current == scope)
                        .unwrap_or(0);
                    if let Some(chosen) = widget::segmented(ui, "measure.scope", &labels, index) {
                        let next = Scope::ALL[chosen];
                        if next != scope {
                            commands.push(Command::SetMeasureScope(next));
                        }
                    }
                },
            );
        });
    }
    let view_rect = Rect::from_min_size(
        Vec2::new(
            scope_rect.max.x + if ranged { 12.0 } else { 0.0 },
            row.min.y,
        ),
        Vec2::new(VIEW_WIDTH, row.height()),
    );
    ui.place(view_rect, |ui| {
        let index = match view {
            MeasureView::Table => 0,
            MeasureView::Histogram => 1,
            MeasureView::Scatter => 2,
        };
        if let Some(chosen) = widget::segmented(
            ui,
            "measure.view",
            &["Table", "Histogram", "Scatter"],
            index,
        ) && MeasureView::ALL[chosen] != view
        {
            commands.push(Command::SetMeasureView(MeasureView::ALL[chosen]));
        }
    });
    let lasso = state.measure.lasso().is_some();
    let gated = lasso
        || state.measure.filters().iter().any(|filter| {
            filter.column() == axes[0]
                || (view == MeasureView::Scatter && filter.column() == axes[1])
        });
    ui.place(clear, |ui| {
        if widget::icon_button_enabled(
            ui,
            "measure.gate.clear",
            Icon::Close,
            24.0,
            Role::Quiet,
            "Clear the gate and the lasso",
            gated,
        ) {
            let mut indices = state
                .measure
                .filters()
                .iter()
                .enumerate()
                .filter(|(_, filter)| {
                    filter.column() == axes[0]
                        || (view == MeasureView::Scatter && filter.column() == axes[1])
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            indices.sort_unstable_by(|left, right| right.cmp(left));
            for index in indices {
                commands.push(Command::RemoveFilter(index));
            }
            if lasso {
                commands.push(Command::SetLasso(None));
            }
        }
    });
    if view == MeasureView::Table {
        return;
    }
    let options = table
        .columns()
        .iter()
        .enumerate()
        .map(|(index, column)| (index, crate::panel::column_label(column)))
        .collect::<Vec<_>>();
    let area = Rect::from_min_max(Vec2::new(view_rect.max.x + 14.0, row.min.y), actions.max);
    ui.place(area, |ui| {
        ui.row(
            Block::row()
                .gaps(6.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                widget::field_label(ui, "X");
                let width = if view == MeasureView::Scatter {
                    156.0
                } else {
                    212.0
                };
                if let Some(column) =
                    grana_ui::popover::combo(ui, "measure.plot.x", width, &options, Some(axes[0]))
                    && column != axes[0]
                {
                    commands.push(Command::SetPlotAxis {
                        axis: PlotAxis::X,
                        column,
                    });
                }
                if view == MeasureView::Scatter {
                    widget::field_label(ui, "Y");
                    if let Some(column) = grana_ui::popover::combo(
                        ui,
                        "measure.plot.y",
                        width,
                        &options,
                        Some(axes[1]),
                    ) && column != axes[1]
                    {
                        commands.push(Command::SetPlotAxis {
                            axis: PlotAxis::Y,
                            column,
                        });
                    }
                }
            },
        );
    });
}

fn header(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let sets = state
        .scene
        .objects
        .iter()
        .map(|set| (set.id(), set.name().to_string()))
        .collect::<Vec<_>>();
    let current = state.measure.set();
    let mut chosen = current.or_else(|| sets.first().map(|(id, _)| *id));
    let mut group = state.measure.group();
    let mut aggregate = state.measure.aggregate();
    let row = ui.allocate_full(32.0);
    let right_width = 236.0;
    let left = Rect::from_min_size(
        row.min,
        Vec2::new((row.width() - right_width - 12.0).max(340.0), row.height()),
    );
    let right = Rect::from_min_size(
        Vec2::new(row.max.x - right_width, row.min.y),
        Vec2::new(right_width, row.height()),
    );
    ui.place(left, |ui| {
        ui.row(
            Block::row()
                .gaps(8.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                widget::field_label(ui, "SET");
                if let Some(value) =
                    grana_ui::popover::combo(ui, "measure.set", 148.0, &sets, chosen)
                {
                    chosen = Some(value);
                }
                if let Some(id) = chosen
                    && chosen != current
                {
                    commands.push(Command::SetMeasureSet(Some(id)));
                }
                let trackable = chosen
                    .and_then(|id| state.scene.objects.get(id))
                    .is_some_and(|object| object.kind().trackable());
                let mut options = vec!["objects"];
                if trackable {
                    options.push("tracks");
                }
                options.push("timepoints");
                let index = match group {
                    Group::Object => 0,
                    Group::Track => 1,
                    Group::Timepoint => {
                        if trackable {
                            2
                        } else {
                            1
                        }
                    }
                };
                if let Some(index) = widget::segmented(ui, "measure.group", &options, index) {
                    let next = match index {
                        0 => Group::Object,
                        1 if trackable => Group::Track,
                        1 => Group::Timepoint,
                        _ => Group::Timepoint,
                    };
                    if next != group {
                        group = next;
                        commands.push(Command::SetMeasureGroup(group));
                    }
                }
                if group != Group::Object {
                    let options = [
                        (Aggregate::Mean, "mean".to_string()),
                        (Aggregate::Sum, "sum".to_string()),
                        (Aggregate::Minimum, "min".to_string()),
                        (Aggregate::Maximum, "max".to_string()),
                        (Aggregate::Deviation, "σ".to_string()),
                    ];
                    if let Some(value) = grana_ui::popover::combo(
                        ui,
                        "measure.aggregate",
                        88.0,
                        &options,
                        Some(aggregate),
                    ) && value != aggregate
                    {
                        aggregate = value;
                        commands.push(Command::SetMeasureAggregate(aggregate));
                    }
                }
            },
        );
    });
    let total = state.measure.total();
    let shown = state.measure.rows().len();
    ui.place(right, |ui| {
        ui.row(
            Block::row()
                .gaps(6.0)
                .align(Align::Center)
                .justify(Align::End)
                .width(Length::Fill)
                .height(Length::Fill),
            |ui| {
                widget::caption(ui, &format!("{shown} / {total} rows"));
                if widget::icon_button(
                    ui,
                    "measure.export",
                    Icon::Export,
                    28.0,
                    Role::Quiet,
                    "Export CSV",
                ) {
                    commands.push(Command::ExportMeasure);
                }
                let key = ui.id("measure.columns");
                let response = widget::icon_button_response(
                    ui,
                    "measure.columns",
                    Icon::Columns,
                    28.0,
                    Role::Quiet,
                    "Columns",
                );
                if response.clicked {
                    ui.toggle_popup(key);
                }
                columns_menu(ui, state, commands, response.rect);
                if widget::icon_button(
                    ui,
                    "measure.filter",
                    Icon::Filter,
                    28.0,
                    Role::Quiet,
                    "Add filter",
                ) {
                    commands.push(Command::AddFilter);
                }
            },
        );
    });
}

fn columns_menu(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>, anchor: Rect) {
    let key = ui.id("measure.columns");
    let open = ui.popup_open(key);
    let Some(table) = state.measure.table() else {
        return;
    };
    let columns = table.columns().to_vec();
    let mut toggles: Vec<(usize, bool)> = Vec::new();
    let visible = state.measure.visible().to_vec();
    grana_ui::popover::popover(ui, "measure.columns", open, anchor, 240.0, |ui| {
        if widget::button(ui, "show.all", "Show all columns", Role::Quiet) {
            commands.push(Command::ShowAllColumns);
        }
        grana_ui::popover::separator(ui);
        for (index, column) in columns.iter().enumerate() {
            let shown = visible.get(index).copied().unwrap_or(false);
            if let Some(next) = widget::checkbox(
                ui,
                &format!("column.{index}"),
                &crate::panel::column_label(column),
                shown,
            ) {
                toggles.push((index, next));
            }
        }
    });
    for (index, shown) in toggles {
        commands.push(Command::SetColumnVisible {
            index,
            visible: shown,
        });
    }
}

fn filters(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let Some(columns) = state.measure.table().map(|table| {
        table
            .columns()
            .iter()
            .map(|column| {
                (
                    column.label().to_string(),
                    column.unit().suffix().to_string(),
                )
            })
            .collect::<Vec<_>>()
    }) else {
        return;
    };
    let options = columns
        .iter()
        .enumerate()
        .map(|(index, (name, unit))| {
            let text = if unit.is_empty() {
                name.clone()
            } else {
                format!("{name} ({unit})")
            };
            (index, text)
        })
        .collect::<Vec<_>>();
    let visibility = state.measure.visibility();
    let filtered = !state.measure.filters().is_empty();
    if filtered || state.measure.lasso().is_some() {
        if filtered {
            ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                if let Some(value) =
                    widget::checkbox(ui, "hide.filtered", "Hide filtered rows", visibility)
                {
                    commands.push(Command::SetMeasureVisibility(value));
                }
            });
        }
        ui.row(Block::row().gaps(6.0).align(Align::Center), |ui| {
            if widget::button(ui, "extract.filtered", "Extract filtered", Role::Tinted) {
                commands.push(Command::ExtractFiltered);
            }
            if widget::button(ui, "delete.excluded", "Delete excluded", Role::Danger) {
                commands.push(Command::DeleteFiltered);
            }
        });
    }
    let filters = state.measure.filters().to_vec();
    let mut removed = None;
    let mut retargeted = None;
    let mut applied = None;
    for (index, filter) in filters.iter().enumerate() {
        ui.push_id(&format!("filter.{index}"));
        let mut column = filter.column();
        let mut minimum = filter.minimum();
        let mut maximum = filter.maximum();
        ui.column(
            Block::column()
                .padding(grana_ui::Edges::symmetric(10.0, 6.0))
                .gaps(4.0)
                .background(card_look(ui)),
            |ui| {
                let row = ui.allocate_full(28.0);
                let remove = Rect::from_center_size(
                    Vec2::new(row.max.x - 12.0, row.center().y),
                    Vec2::splat(22.0),
                );
                let controls =
                    Rect::from_min_max(row.min, Vec2::new(remove.min.x - 8.0, row.max.y));
                ui.place(controls, |ui| {
                    ui.row(
                        Block::row()
                            .gaps(6.0)
                            .align(Align::Center)
                            .height(Length::Fill),
                        |ui| {
                            widget::field_label(ui, "FILTER");
                            if let Some(value) = grana_ui::popover::combo(
                                ui,
                                "column",
                                170.0,
                                &options,
                                Some(column),
                            ) {
                                column = value;
                            }
                            let speed = ((maximum - minimum).abs().max(1.0)) / 200.0;
                            if let Some(value) =
                                widget::drag_value(ui, "minimum", minimum, speed, 4)
                            {
                                minimum = value;
                            }
                            widget::caption(ui, "to");
                            if let Some(value) =
                                widget::drag_value(ui, "maximum", maximum, speed, 4)
                            {
                                maximum = value;
                            }
                        },
                    );
                });
                ui.place(remove, |ui| {
                    if widget::icon_button(
                        ui,
                        "remove",
                        Icon::Close,
                        22.0,
                        Role::Quiet,
                        "Remove filter",
                    ) {
                        removed = Some(index);
                    }
                });
            },
        );
        ui.pop_id();
        if column != filter.column() {
            retargeted = Some((index, column));
        } else if minimum != filter.minimum() || maximum != filter.maximum() {
            let (low, high) = if minimum <= maximum {
                (minimum, maximum)
            } else {
                (maximum, minimum)
            };
            applied = Some((index, Filter::new(column, low, high)));
        }
    }
    if let Some((index, column)) = retargeted {
        commands.push(Command::RetargetFilter { index, column });
    }
    if let Some((index, filter)) = applied {
        commands.push(Command::SetFilter { index, filter });
    }
    if let Some(index) = removed {
        commands.push(Command::RemoveFilter(index));
    }
}

fn card_look(ui: &Ui) -> grana_ui::Look {
    let mut look = ui.theme().look(Role::Card, grana_ui::State::Rest);
    look.corners = Corners::all(ui.theme().metrics.card_radius);
    look
}

fn table(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let Some(set) = state.measure.set() else {
        empty_state(
            ui,
            Icon::Table,
            "No object set selected",
            "Choose an object set to inspect its measurements.",
        );
        return;
    };
    let Some(table) = state.measure.table() else {
        empty_state(
            ui,
            Icon::Table,
            "No detection yet",
            "Detect this object set to build its measurement table.",
        );
        return;
    };
    if table.rows() == 0 {
        empty_state(
            ui,
            Icon::Table,
            "No rows",
            match state.measure.group() {
                Group::Track => "Track the objects to summarise their trajectories.",
                Group::Object | Group::Timepoint => {
                    "Detect objects in the region to build a measurement table."
                }
            },
        );
        return;
    }
    let visible = state.measure.visible().to_vec();
    let columns = table
        .columns()
        .iter()
        .enumerate()
        .filter(|(index, _)| visible.get(*index).copied().unwrap_or(true))
        .map(|(index, column)| (index, column.clone()))
        .collect::<Vec<_>>();
    if columns.is_empty() {
        empty_state(
            ui,
            Icon::Columns,
            "No columns",
            "Choose at least one column from the columns menu.",
        );
        return;
    }
    let surface = Rect::from_min_size(ui.cursor_position(), ui.available());
    crate::panel::card_surface(ui, surface);
    let card_radius = ui.theme().metrics.card_radius;
    let available = surface.width();
    let widths = column_widths(&columns, available - 1.0);
    let total = widths.iter().sum::<f32>();
    let band = ui.allocate(Vec2::new(total, HEADER_HEIGHT));
    {
        let mut painter = ui.painter();
        painter.rect(band, Corners::top(card_radius), Color::white_alpha(0.035));
        painter.rect(
            Rect::from_min_size(
                Vec2::new(band.min.x, band.max.y - 0.5),
                Vec2::new(band.width(), 0.5),
            ),
            Corners::ZERO,
            Color::white_alpha(0.07),
        );
    }
    let mut left = band.min.x;
    let mut toggled = Vec::new();
    for ((index, column), width) in columns.iter().zip(widths.iter()) {
        let rect = Rect::from_min_size(
            Vec2::new(left, band.min.y),
            Vec2::new(*width, HEADER_HEIGHT),
        );
        let sorted = state
            .measure
            .sort()
            .filter(|(current, _)| current == index)
            .map(|(_, descending)| descending);
        if header_cell(
            ui,
            &format!("head.{index}"),
            &crate::panel::column_label(column),
            sorted,
            rect,
        ) {
            toggled.push(*index);
        }
        left += *width;
    }
    let shown = state.measure.rows().len();
    let content = shown as f32 * ROW_HEIGHT;
    let key = ui.id("measure.rows");
    let info = ui.scroll_info(key);
    ui.scroll_axis(key, Axis::Column, Length::Fill, Some(content + 4.0), |ui| {
        let first = (info.offset / ROW_HEIGHT).floor().max(0.0) as usize;
        let visible_rows = (info.viewport / ROW_HEIGHT).ceil() as usize + 1;
        let last = (first + visible_rows).min(shown);
        if first > 0 {
            ui.space(first as f32 * ROW_HEIGHT);
        }
        for row in first..last {
            let Some(index) = state.measure.rows().get(row).copied() else {
                continue;
            };
            let observation = table.observation(index);
            let selected = match (observation.time(), observation.object()) {
                (Some(time), Some(object)) => {
                    state.scene.selection()
                        == Some(Selection {
                            set,
                            item: Item::Object(object),
                        })
                        && state.scene.time() == time
                }
                _ => match observation.track() {
                    Some(track) => {
                        state.scene.selection()
                            == Some(Selection {
                                set,
                                item: Item::Track(track),
                            })
                    }
                    None => false,
                },
            };
            let row_rect = ui.allocate(Vec2::new(total, ROW_HEIGHT));
            let response = ui.interact(ui.id(&format!("row.{row}")), row_rect);
            let hover = ui.animate(
                ui.id(&format!("row.{row}.hover")),
                if response.hovered { 1.0 } else { 0.0 },
                grana_ui::Motion::quick(),
            );
            {
                let accent = ui.theme().palette.accent;
                let color = ui.theme().palette.text_secondary;
                let font = ui.theme().font(grana_ui::Level::Monospace);
                let mut painter = ui.painter();
                if selected {
                    painter.rect(row_rect, Corners::ZERO, accent.with_alpha(0.15));
                    painter.rect(
                        Rect::from_min_size(row_rect.min, Vec2::new(2.0, row_rect.height())),
                        Corners::ZERO,
                        accent,
                    );
                } else if hover > 0.01 {
                    painter.rect(row_rect, Corners::ZERO, Color::white_alpha(0.045 * hover));
                } else if row % 2 == 0 {
                    painter.rect(row_rect, Corners::ZERO, Color::white_alpha(0.018));
                }
                let mut left = row_rect.min.x;
                for ((column_index, column), width) in columns.iter().zip(widths.iter()) {
                    let cell = Rect::from_min_size(
                        Vec2::new(left, row_rect.min.y),
                        Vec2::new(*width, ROW_HEIGHT),
                    );
                    let value = table.value(index, *column_index);
                    painter.text_in(
                        cell.shrink2(Vec2::new(8.0, 0.0)),
                        &column.format(value),
                        TextStyle::new(font, color).aligned(Align::End),
                        Align::Center,
                    );
                    left += *width;
                }
            }
            if response.clicked {
                locate(commands, set, observation);
            }
        }
        let remaining = shown.saturating_sub(last);
        if remaining > 0 {
            ui.space(remaining as f32 * ROW_HEIGHT);
        }
    });
    for index in toggled {
        commands.push(Command::ToggleSort(index));
    }
}

pub(crate) fn locate(commands: &mut Vec<Command>, set: ObjectSetId, observation: Observation) {
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

fn header_cell(ui: &mut Ui, id: &str, text: &str, sorted: Option<bool>, rect: Rect) -> bool {
    let key = ui.id(id);
    let response = ui.interact(key, rect);
    let hover = ui.animate(
        key,
        if response.hovered { 1.0 } else { 0.0 },
        grana_ui::Motion::quick(),
    );
    let style = TextStyle::new(
        ui.theme().font(grana_ui::Level::Micro),
        if sorted.is_some() {
            ui.theme().palette.text_secondary
        } else {
            ui.theme().palette.text_tertiary
        },
    )
    .aligned(Align::End);
    let arrow = match sorted {
        Some(true) => Some(Icon::ChevronDown),
        Some(false) => Some(Icon::ChevronUp),
        None => None,
    };
    let icon_color = ui.theme().palette.text_tertiary;
    let card_radius = ui.theme().metrics.card_radius;
    let mut painter = ui.painter();
    if hover > 0.01 {
        painter.rect(
            rect,
            Corners::top(card_radius),
            Color::white_alpha(0.04 * hover),
        );
    }
    painter.text_in(
        Rect::from_min_max(
            rect.min + Vec2::new(8.0, 0.0),
            Vec2::new(rect.max.x - arrow.map_or(8.0, |_| 16.0), rect.max.y),
        ),
        text,
        style,
        Align::Center,
    );
    if let Some(arrow) = arrow {
        icon::paint(
            arrow,
            &mut painter,
            Vec2::new(rect.max.x - 9.0, rect.center().y),
            9.0,
            icon_color,
            1.4,
        );
    }
    response.clicked
}

fn column_widths(columns: &[(usize, Column)], available: f32) -> Vec<f32> {
    let initial = columns
        .iter()
        .map(|(_, column)| {
            let header = crate::panel::column_label(column).chars().count() as f32 * 5.9 + 22.0;
            let value = (f32::from(column.decimals()) + 6.0) * 7.0 + 18.0;
            header.max(value).max(MIN_COLUMN_WIDTH)
        })
        .collect::<Vec<_>>();
    let total = initial.iter().sum::<f32>();
    if total <= available {
        let extra = (available - total) / columns.len() as f32;
        return initial.iter().map(|width| width + extra).collect();
    }
    let scale = available / total;
    initial.iter().map(|width| width * scale).collect()
}
