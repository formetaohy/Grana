use glam::Vec2;
use grana_measure::{Aggregate, Column, Filter, Group};
use grana_scene::{Item, Selection};
use grana_ui::{Align, Axis, Block, Color, Corners, Length, Rect, Role, TextStyle, Ui};

use crate::action::Action;
use crate::state::State;
use crate::widget;

const ROW_HEIGHT: f32 = 26.0;
const HEADER_HEIGHT: f32 = 30.0;
const MIN_COLUMN_WIDTH: f32 = 58.0;

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    header(ui, state, actions);
    filters(ui, state);
    table(ui, state, actions);
}

fn header(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let sets = state
        .scene
        .objects
        .iter()
        .map(|set| (set.id(), set.name().to_string()))
        .collect::<Vec<_>>();
    if sets.is_empty() {
        widget::weak(ui, "add an object set and detect it to quantify it");
        return;
    }
    let current = state.measure.set();
    let mut chosen = current.or_else(|| sets.first().map(|(id, _)| *id));
    let mut group = state.measure.group();
    let mut aggregate = state.measure.aggregate();
    ui.row(Block::row().gaps(10.0).align(Align::Center), |ui| {
        widget::field_label(ui, "object set");
        if let Some(value) = grana_ui::popover::combo(ui, "measure.set", 168.0, &sets, chosen) {
            chosen = Some(value);
        }
        if let Some(id) = chosen
            && chosen != current
        {
            state.measure.select(id, state.scene);
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
                state.measure.set_group(group, state.scene);
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
            if let Some(value) =
                grana_ui::popover::combo(ui, "measure.aggregate", 96.0, &options, Some(aggregate))
                && value != aggregate
            {
                aggregate = value;
                state.measure.set_aggregate(aggregate, state.scene);
            }
        }
        let total = state.measure.table().map_or(0, |table| table.rows());
        let shown = state.measure.rows().len();
        widget::caption(ui, &format!("{shown} of {total} rows"));
        ui.space(8.0);
        if widget::button(ui, "export", "Export CSV", Role::Quiet) {
            actions.push(Action::ExportMeasure);
        }
        columns_menu(ui, state);
        if widget::button(ui, "add.filter", "Filter", Role::Quiet) {
            state.measure.add_filter();
        }
    });
}

fn columns_menu(ui: &mut Ui, state: &mut State<'_>) {
    let key = ui.id("measure.columns");
    let rect = ui.allocate(Vec2::new(84.0, ui.theme().metrics.control_height));
    let response = ui.interact(key, rect);
    if response.clicked {
        ui.toggle_popup(key);
    }
    let look = ui.look(key, Role::Control, &response);
    let font = ui.theme().font(grana_ui::Level::Callout);
    {
        let mut painter = ui.painter();
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        painter.text_in(
            rect,
            "Columns",
            TextStyle::new(font, look.text).aligned(Align::Center),
            Align::Center,
        );
    }
    let Some(table) = state.measure.table() else {
        return;
    };
    let columns = table.columns().to_vec();
    let mut toggles: Vec<(usize, bool)> = Vec::new();
    let visible = state.measure.visible().to_vec();
    let open = ui.popup_open(key);
    grana_ui::popover::popover(ui, "measure.columns", open, rect, 220.0, |ui| {
        if widget::button(ui, "show.all", "Show all", Role::Quiet) {
            state.measure.show_all_columns();
        }
        for (index, column) in columns.iter().enumerate() {
            let shown = visible.get(index).copied().unwrap_or(false);
            if let Some(next) =
                widget::checkbox(ui, &format!("column.{index}"), &label(column), shown)
            {
                toggles.push((index, next));
            }
        }
    });
    for (index, shown) in toggles {
        state.measure.set_column_visible(index, shown);
    }
}

fn filters(ui: &mut Ui, state: &mut State<'_>) {
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
    let mut visibility = state.measure.visibility();
    let filtered = !state.measure.filters().is_empty();
    if filtered {
        ui.row(Block::row().gaps(10.0).align(Align::Center), |ui| {
            widget::field_label(ui, "filter");
            if let Some(value) = widget::checkbox(ui, "hide.filtered", "hide filtered", visibility)
            {
                visibility = value;
                state.measure.set_visibility(value);
            }
        });
    }
    let filters = state.measure.filters().to_vec();
    let mut removed = None;
    let mut retargeted = None;
    let mut applied = None;
    for (index, filter) in filters.iter().enumerate() {
        ui.push_id(&format!("filter.{index}"));
        let look = ui.theme().look(Role::Card, grana_ui::State::Rest);
        let mut column = filter.column();
        let mut minimum = filter.minimum();
        let mut maximum = filter.maximum();
        ui.column(
            Block::column()
                .padding(grana_ui::Edges::symmetric(8.0, 6.0))
                .gaps(6.0)
                .background(look),
            |ui| {
                ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
                    if let Some(value) =
                        grana_ui::popover::combo(ui, "column", 170.0, &options, Some(column))
                    {
                        column = value;
                    }
                    let speed = ((maximum - minimum).abs().max(1.0)) / 200.0;
                    if let Some(value) = widget::drag_value(ui, "minimum", minimum, speed, 4) {
                        minimum = value;
                    }
                    if let Some(value) = widget::drag_value(ui, "maximum", maximum, speed, 4) {
                        maximum = value;
                    }
                    if widget::icon_button(ui, "remove", "\u{00D7}", 20.0, Role::Quiet) {
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
        state.measure.retarget_filter(index, column);
    }
    if let Some((index, filter)) = applied {
        state.measure.set_filter(index, filter);
    }
    if let Some(index) = removed {
        state.measure.remove_filter(index);
    }
}

fn table(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let Some(set) = state.measure.set() else {
        return;
    };
    let Some(table) = state.measure.table() else {
        widget::weak(ui, "this object set has no detection results yet");
        return;
    };
    if table.rows() == 0 {
        let message = match state.measure.group() {
            Group::Track => "no tracks yet: track the objects to summarise them",
            Group::Object | Group::Timepoint => "no detections to summarise",
        };
        widget::weak(ui, message);
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
        widget::weak(ui, "no columns are shown");
        return;
    }
    let available = ui.available().x;
    let widths = column_widths(&columns, available);
    let total = widths.iter().sum::<f32>();
    let band = ui.allocate(Vec2::new(total, HEADER_HEIGHT));
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
        if header_cell(ui, &format!("head.{index}"), &label(column), sorted, rect) {
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
                    painter.rect(row_rect, Corners::ZERO, accent.with_alpha(0.20));
                } else if hover > 0.01 {
                    painter.rect(row_rect, Corners::ZERO, Color::white_alpha(0.05 * hover));
                } else if row % 2 == 0 {
                    painter.rect(row_rect, Corners::ZERO, Color::white_alpha(0.022));
                }
                let mut left = row_rect.min.x;
                for ((column_index, column), width) in columns.iter().zip(widths.iter()) {
                    let cell = Rect::from_min_size(
                        Vec2::new(left, row_rect.min.y),
                        Vec2::new(*width, ROW_HEIGHT),
                    );
                    let value = table.value(index, *column_index);
                    painter.text_in(
                        cell.shrink2(Vec2::new(6.0, 0.0)),
                        &column.format(value),
                        TextStyle::new(font, color).aligned(Align::End),
                        Align::Center,
                    );
                    left += *width;
                }
            }
            if response.clicked {
                match (
                    observation.time(),
                    observation.object(),
                    observation.track(),
                ) {
                    (Some(time), Some(object), _) => {
                        actions.push(Action::SetTime(time));
                        actions.push(Action::Select(Some(Selection {
                            set,
                            item: Item::Object(object),
                        })));
                    }
                    (_, _, Some(track)) => {
                        actions.push(Action::Select(Some(Selection {
                            set,
                            item: Item::Track(track),
                        })));
                    }
                    (Some(time), _, _) => actions.push(Action::SetTime(time)),
                    _ => {}
                }
            }
        }
        let remaining = shown.saturating_sub(last);
        if remaining > 0 {
            ui.space(remaining as f32 * ROW_HEIGHT);
        }
    });
    for index in toggled {
        state.measure.toggle_sort(index);
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
    let label = match sorted {
        Some(true) => format!("{text} ▼"),
        Some(false) => format!("{text} ▲"),
        None => text.to_string(),
    };
    let style = TextStyle::new(
        ui.theme().font(grana_ui::Level::Caption),
        ui.theme().palette.text_tertiary,
    )
    .aligned(Align::End);
    let mut painter = ui.painter();
    if hover > 0.01 {
        painter.rect(rect, Corners::all(5.0), Color::white_alpha(0.05 * hover));
    }
    painter.text_in(
        rect.shrink2(Vec2::new(6.0, 2.0)),
        &label,
        style,
        Align::Center,
    );
    response.clicked
}

fn column_widths(columns: &[(usize, Column)], available: f32) -> Vec<f32> {
    let initial = columns
        .iter()
        .map(|(_, column)| {
            let header = label(column).chars().count() as f32 * 6.2 + 20.0;
            let value = (f32::from(column.decimals()) + 6.0) * 7.2 + 14.0;
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

fn label(column: &Column) -> String {
    let unit = column.unit().suffix();
    if unit.is_empty() {
        column.label().to_string()
    } else {
        format!("{} ({unit})", column.label())
    }
}
