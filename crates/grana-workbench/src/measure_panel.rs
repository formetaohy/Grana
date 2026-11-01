use glam::Vec2;
use grana_measure::{Aggregate, Column, Filter, Group};
use grana_scene::{Item, Selection};
use grana_ui::{Align, Axis, Block, Color, Corners, Icon, Length, Rect, Role, TextStyle, Ui, icon};

use crate::action::Action;
use crate::panel::empty_state;
use crate::state::State;
use crate::widget;

const ROW_HEIGHT: f32 = 28.0;
const HEADER_HEIGHT: f32 = 30.0;
const MIN_COLUMN_WIDTH: f32 = 58.0;

pub fn build(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
    if state.scene.objects.is_empty() {
        empty_state(
            ui,
            Icon::Table,
            "No measurements yet",
            "Add and detect an object set to build a measurement table, then filter, sort and export it.",
        );
        return;
    }
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
                    if let Some(value) = grana_ui::popover::combo(
                        ui,
                        "measure.aggregate",
                        88.0,
                        &options,
                        Some(aggregate),
                    ) && value != aggregate
                    {
                        aggregate = value;
                        state.measure.set_aggregate(aggregate, state.scene);
                    }
                }
            },
        );
    });
    let total = state.measure.table().map_or(0, |table| table.rows());
    let shown = state.measure.rows().len();
    ui.place(right, |ui| {
        ui.row(
            Block::row()
                .gaps(6.0)
                .align(Align::Center)
                .justify(Align::End)
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
                    actions.push(Action::ExportMeasure);
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
                columns_menu(ui, state, response.rect);
                if widget::icon_button(
                    ui,
                    "measure.filter",
                    Icon::Filter,
                    28.0,
                    Role::Quiet,
                    "Add filter",
                ) {
                    state.measure.add_filter();
                }
            },
        );
    });
}

fn columns_menu(ui: &mut Ui, state: &mut State<'_>, anchor: Rect) {
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
            state.measure.show_all_columns();
        }
        grana_ui::popover::separator(ui);
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
        ui.row(Block::row().gaps(8.0).align(Align::Center), |ui| {
            if let Some(value) =
                widget::checkbox(ui, "hide.filtered", "Hide filtered rows", visibility)
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
        state.measure.retarget_filter(index, column);
    }
    if let Some((index, filter)) = applied {
        state.measure.set_filter(index, filter);
    }
    if let Some(index) = removed {
        state.measure.remove_filter(index);
    }
}

fn card_look(ui: &Ui) -> grana_ui::Look {
    let mut look = ui.theme().look(Role::Card, grana_ui::State::Rest);
    look.corners = Corners::all(ui.theme().metrics.card_radius);
    look
}

fn table(ui: &mut Ui, state: &mut State<'_>, actions: &mut Vec<Action>) {
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
    let card_radius = ui.theme().metrics.card_radius;
    {
        let mut painter = ui.painter();
        painter.outlined(
            surface,
            Corners::all(card_radius),
            grana_ui::Fill::Solid(Color::rgba(0.025, 0.032, 0.045, 0.88)),
            grana_ui::Border {
                width: 0.5,
                color: Color::white_alpha(0.06),
            },
            grana_ui::Shadow::NONE,
        );
    }
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
            let header = label(column).chars().count() as f32 * 5.9 + 22.0;
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

fn label(column: &Column) -> String {
    let unit = column.unit().suffix();
    if unit.is_empty() {
        column.label().to_string()
    } else {
        format!("{} ({unit})", column.label())
    }
}
