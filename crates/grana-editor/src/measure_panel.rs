use glam::Vec2;
use grana_measure::{Aggregate, Column, Filter, Group, MeasureView, Observation, PlotAxis, Scope};
use grana_scene::{Item, ObjectSetId, Selection};
use grana_ui::{
    Align, Axis, Block, Color, Corners, Cursor, Icon, Length, Level, Rect, Role, TextStyle, Ui,
    icon,
};

use crate::panel::empty_state;
use crate::plot;
use crate::state::State;
use crate::widget;
use grana_session::Command;

const ROW_HEIGHT: f32 = 26.0;
const HEADER_HEIGHT: f32 = 30.0;
const MIN_COLUMN_WIDTH: f32 = 58.0;

pub fn build(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    if state.scene.objects.is_empty() {
        empty_state(
            ui,
            Icon::Table,
            "No Measurements Yet",
            "Add and detect an object set to build a measurement table, then filter, sort and export it.",
        );
        return;
    }
    toolbar(ui, state, commands);
    let measured = state.measure.table().is_some();
    if measured && state.measure.view() != MeasureView::Table {
        plot_bar(ui, state, commands);
    }
    filters(ui, state, commands);
    ui.space(6.0);
    if measured && state.measure.view() != MeasureView::Table {
        plot::view(ui, state, commands);
    } else {
        table(ui, state, commands);
    }
}

fn toolbar(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
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
    let scope = state.measure.scope();
    let view = state.measure.view();
    let measured = state.measure.table().is_some();
    let row = ui.allocate_full(30.0);
    ui.place(row, |ui| {
        ui.row(
            Block::row()
                .gaps(8.0)
                .align(Align::Center)
                .width(Length::Fill)
                .height(Length::Fill),
            |ui| {
                widget::field_label(ui, "Set");
                if let Some(value) =
                    grana_ui::popover::combo(ui, "measure.set", 140.0, &sets, chosen)
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
                let mut options = vec!["Objects"];
                if trackable {
                    options.push("Tracks");
                }
                options.push("Frames");
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
                        _ => Group::Timepoint,
                    };
                    if next != group {
                        group = next;
                        commands.push(Command::SetMeasureGroup(group));
                    }
                }
                if group != Group::Object {
                    let options = [
                        (Aggregate::Mean, "Mean".to_string()),
                        (Aggregate::Sum, "Sum".to_string()),
                        (Aggregate::Minimum, "Min".to_string()),
                        (Aggregate::Maximum, "Max".to_string()),
                        (Aggregate::Deviation, "SD (σ)".to_string()),
                    ];
                    if let Some(value) = grana_ui::popover::combo(
                        ui,
                        "measure.aggregate",
                        86.0,
                        &options,
                        Some(aggregate),
                    ) && value != aggregate
                    {
                        aggregate = value;
                        commands.push(Command::SetMeasureAggregate(aggregate));
                    }
                }
                if measured && group != Group::Track {
                    let labels = ["All", "Current"];
                    let scope_idx = Scope::ALL
                        .iter()
                        .position(|current| *current == scope)
                        .unwrap_or(0);
                    if let Some(chosen) = widget::segmented(ui, "measure.scope", &labels, scope_idx)
                    {
                        let next = Scope::ALL[chosen];
                        if next != scope {
                            commands.push(Command::SetMeasureScope(next));
                        }
                    }
                }
                let total = state.measure.total();
                let shown = state.measure.rows().len();
                let label = format!("{shown}/{total} rows");
                let caption_width = ui
                    .measure_text(
                        &label,
                        TextStyle::new(
                            ui.theme().font(Level::Caption),
                            ui.theme().palette.text_tertiary,
                        ),
                    )
                    .x;
                let view_w = if measured { 222.0 } else { 0.0 };
                let right_content = view_w + caption_width + 3.0 * 28.0 + 4.0 * 8.0;
                ui.space((ui.available().x - right_content).max(4.0));
                if measured {
                    let view_idx = match view {
                        MeasureView::Table => 0,
                        MeasureView::Histogram => 1,
                        MeasureView::Scatter => 2,
                    };
                    if let Some(chosen) = widget::segmented(
                        ui,
                        "measure.view",
                        &["Table", "Histogram", "Scatter"],
                        view_idx,
                    ) && MeasureView::ALL[chosen] != view
                    {
                        commands.push(Command::SetMeasureView(MeasureView::ALL[chosen]));
                    }
                }
                widget::caption(ui, &label);
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
                let key = ui.id("measure.columns");
                let response = widget::icon_button_response(
                    ui,
                    "measure.columns",
                    Icon::Columns,
                    28.0,
                    Role::Quiet,
                    "Choose columns",
                );
                if response.clicked {
                    ui.toggle_popup(key);
                }
                columns_menu(ui, state, commands, response.rect);
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
            },
        );
    });
}

fn plot_bar(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let Some(table) = state.measure.table() else {
        return;
    };
    let view = state.measure.view();
    let axes = state.measure.axes();
    let options = table
        .columns()
        .iter()
        .enumerate()
        .map(|(index, column)| (index, crate::panel::column_label(column)))
        .collect::<Vec<_>>();
    let lasso = state.measure.lasso().is_some();
    let gated = lasso
        || state.measure.filters().iter().any(|filter| {
            filter.column() == axes[0]
                || (view == MeasureView::Scatter && filter.column() == axes[1])
        });
    ui.space(4.0);
    let row = ui.allocate_full(28.0);
    ui.place(row, |ui| {
        ui.row(
            Block::row()
                .gaps(8.0)
                .align(Align::Center)
                .height(Length::Fill),
            |ui| {
                widget::field_label(ui, "X Axis");
                if let Some(column) =
                    grana_ui::popover::combo(ui, "measure.plot.x", 180.0, &options, Some(axes[0]))
                    && column != axes[0]
                {
                    commands.push(Command::SetPlotAxis {
                        axis: PlotAxis::X,
                        column,
                    });
                }
                if view == MeasureView::Scatter {
                    widget::field_label(ui, "Y Axis");
                    if let Some(column) = grana_ui::popover::combo(
                        ui,
                        "measure.plot.y",
                        180.0,
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
                if gated && widget::button(ui, "measure.gate.clear", "Clear Gate", Role::Quiet) {
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
        if widget::button(ui, "show.all", "Show All Columns", Role::Quiet) {
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
        ui.space(4.0);
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if filtered
                && let Some(value) =
                    widget::checkbox(ui, "hide.filtered", "Hide Filtered Rows", visibility)
            {
                commands.push(Command::SetMeasureVisibility(value));
            }
            if widget::button(ui, "extract.filtered", "Extract Filtered", Role::Tinted) {
                commands.push(Command::ExtractFiltered);
            }
            if widget::button(ui, "delete.excluded", "Delete Excluded", Role::Danger) {
                commands.push(Command::DeleteFiltered);
            }
        });
    }
    let filters = state.measure.filters().to_vec();
    let mut removed = None;
    let mut retargeted = None;
    let mut applied = None;
    for (index, filter) in filters.iter().enumerate() {
        ui.space(4.0);
        ui.push_id(&format!("filter.{index}"));
        let mut column = filter.column();
        let mut minimum = filter.minimum();
        let mut maximum = filter.maximum();
        ui.column(
            Block::column()
                .padding(grana_ui::Edges::symmetric(10.0, 4.0))
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
                            widget::field_label(ui, "Filter");
                            if let Some(value) = grana_ui::popover::combo(
                                ui,
                                "column",
                                180.0,
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
                            widget::caption(ui, "–");
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

fn card_look(ui: &Ui) -> grana_ui::Surface {
    let mut look = ui.theme().surface(Role::Card, grana_ui::State::Rest);
    look.corners = Corners::all(ui.theme().metrics.card_radius);
    look
}

fn table(ui: &mut Ui, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let Some(set) = state.measure.set() else {
        empty_state(
            ui,
            Icon::Table,
            "No Object Set Selected",
            "Choose an object set to inspect its measurements.",
        );
        return;
    };
    let Some(table) = state.measure.table() else {
        empty_state(
            ui,
            Icon::Table,
            "No Detection Yet",
            "Detect this object set to build its measurement table.",
        );
        return;
    };
    if table.rows() == 0 {
        empty_state(
            ui,
            Icon::Table,
            "No Rows",
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
            "No Visible Columns",
            "Choose at least one column from the columns menu.",
        );
        return;
    }
    let surface = Rect::from_min_size(ui.cursor_position(), ui.available());
    crate::panel::card_surface(ui, surface);
    let available = surface.width();
    let widths = column_widths(ui, &columns, available - 1.0);
    let total = widths.iter().sum::<f32>();
    let band = ui.allocate(Vec2::new(total, HEADER_HEIGHT));
    let palette = ui.theme().palette;
    {
        let mut painter = ui.painter();
        painter.rect(band, Corners::top(16.0), Color::white_alpha(0.025));
        painter.rect(
            Rect::from_min_size(
                Vec2::new(band.min.x, band.max.y - 1.0),
                Vec2::new(band.width(), 1.0),
            ),
            Corners::ZERO,
            palette.border,
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
        let key = ui.id_index("head", *index);
        if header_cell(ui, key, &crate::panel::column_label(column), sorted, rect) {
            toggled.push(*index);
        }
        let grip = Rect::from_min_max(
            Vec2::new(rect.max.x - 3.0, rect.min.y),
            Vec2::new(rect.max.x + 1.0, rect.max.y),
        );
        let grip_key = ui.id_index("measure.grip", *index);
        let grip_response = ui.interact(grip_key, grip);
        if grip_response.hovered || grip_response.dragging {
            ui.set_cursor(Cursor::ResizeHorizontal);
        }
        if grip_response.dragging {
            let width_key = ui.id_index("measure.width", *index);
            let current = ui.value_of(width_key).unwrap_or(*width);
            let spare = available - 1.0 - total;
            let maximum = (current + spare).max(MIN_COLUMN_WIDTH);
            let next = (current + ui.input().delta().x).clamp(MIN_COLUMN_WIDTH, maximum);
            ui.store_value(width_key, next);
        }
        left += *width;
    }
    let shown = state.measure.rows().len();
    let content = shown as f32 * ROW_HEIGHT;
    let key = ui.id("measure.rows");
    let mut info = ui.scroll_info(key);
    if info.viewport <= 0.0 {
        info.viewport = ui.available().y;
    }
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
            let row_key = ui.id_index("row", row);
            let response = ui.interact(row_key, row_rect);
            let hover = ui.animate(
                ui.id_index("row.hover", row),
                if response.hovered { 1.0 } else { 0.0 },
                grana_ui::Motion::quick(),
            );
            {
                let palette = ui.theme().palette;
                let accent = palette.accent;
                let color = if selected {
                    palette.text_primary
                } else {
                    palette.text_secondary
                };
                let font = ui.theme().font(Level::Monospace);
                let mut painter = ui.painter();
                if selected {
                    painter.rect(row_rect, Corners::ZERO, accent.with_alpha(0.16));
                    painter.rect(
                        Rect::from_min_size(row_rect.min, Vec2::new(2.5, row_rect.height())),
                        Corners::ZERO,
                        accent,
                    );
                } else if hover > 0.01 {
                    painter.rect(
                        row_rect,
                        Corners::ZERO,
                        palette.material_hover.with_alpha(0.65 * hover),
                    );
                } else if row % 2 == 1 {
                    painter.rect(row_rect, Corners::ZERO, Color::white_alpha(0.015));
                }
                painter.rect(
                    Rect::from_min_size(
                        Vec2::new(row_rect.min.x, row_rect.max.y - 1.0),
                        Vec2::new(row_rect.width(), 1.0),
                    ),
                    Corners::ZERO,
                    palette.separator,
                );
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

fn header_cell(ui: &mut Ui, key: u64, text: &str, sorted: Option<bool>, rect: Rect) -> bool {
    let mut response = ui.interact(key, rect);
    if ui.focusable(key, rect, Corners::ZERO) && ui.activated(key) {
        response.clicked = true;
    }
    let hover = ui.animate(
        key,
        if response.hovered { 1.0 } else { 0.0 },
        grana_ui::Motion::quick(),
    );
    let palette = ui.theme().palette;
    let style = TextStyle::new(
        ui.theme().label_font(),
        if sorted.is_some() {
            palette.text_primary
        } else {
            palette.text_tertiary
        },
    )
    .aligned(Align::End)
    .truncating();
    let arrow = match sorted {
        Some(true) => Some(Icon::ChevronDown),
        Some(false) => Some(Icon::ChevronUp),
        None => None,
    };
    let icon_color = palette.text_secondary;
    let mut painter = ui.painter();
    if hover > 0.01 {
        painter.rect(
            rect,
            Corners::ZERO,
            palette.material_hover.with_alpha(0.55 * hover),
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

fn column_widths(ui: &Ui, columns: &[(usize, Column)], available: f32) -> Vec<f32> {
    let initial = columns
        .iter()
        .map(|(_, column)| {
            let header = crate::panel::column_label(column).chars().count() as f32 * 5.9 + 22.0;
            let value = (f32::from(column.decimals()) + 6.0) * 7.0 + 18.0;
            header.max(value).max(MIN_COLUMN_WIDTH)
        })
        .collect::<Vec<_>>();
    let total = initial.iter().sum::<f32>();
    let mut widths: Vec<f32> = if total <= available {
        let extra = (available - total) / columns.len() as f32;
        initial.iter().map(|width| width + extra).collect()
    } else {
        let scale = available / total;
        initial.iter().map(|width| width * scale).collect()
    };
    for (slot, (index, _)) in columns.iter().enumerate() {
        if let Some(width) = ui.value_of(ui.id_index("measure.width", *index)) {
            widths[slot] = width.max(MIN_COLUMN_WIDTH);
        }
    }
    let total = widths.iter().sum::<f32>();
    if total > available && total > 0.0 {
        let scale = available / total;
        for width in &mut widths {
            *width *= scale;
        }
    }
    widths
}
