use egui::{RichText, Ui};
use egui_extras::{Column as TableColumn, TableBuilder};
use grana_statistic::{Aggregate, Column, Filter, Group};

use crate::scene::{Item, Selection};
use crate::ui::{Actions, UiState};

const ROW_HEIGHT: f32 = 19.0;
const HEADER_HEIGHT: f32 = 22.0;
const MIN_COLUMN_WIDTH: f32 = 52.0;
const MAX_COLUMN_WIDTH: f32 = 168.0;

pub fn build(ui: &mut Ui, state: &mut UiState<'_>, actions: &mut Actions) {
    header(ui, state, actions);
    ui.separator();
    filters(ui, state);
    table(ui, state, actions);
}

fn header(ui: &mut Ui, state: &mut UiState<'_>, actions: &mut Actions) {
    let sets = state
        .scene
        .objects
        .iter()
        .map(|set| (set.id(), set.name().to_string()))
        .collect::<Vec<_>>();
    ui.horizontal(|ui| {
        ui.strong("Statistics");
        if sets.is_empty() {
            ui.label(
                RichText::new("add an object set and detect it to quantify it")
                    .small()
                    .weak(),
            );
            if ui.button("Hide").clicked() {
                state.statistics.set_open(false);
            }
            return;
        }
        let current = state.statistics.set();
        let mut chosen = current.or_else(|| sets.first().map(|(id, _)| *id));
        let text = chosen
            .and_then(|id| {
                sets.iter()
                    .find(|(candidate, _)| *candidate == id)
                    .map(|(_, name)| name.clone())
            })
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("grana.statistics.set")
            .selected_text(text)
            .show_ui(ui, |ui| {
                for (id, name) in &sets {
                    ui.selectable_value(&mut chosen, Some(*id), name);
                }
            });
        if let Some(id) = chosen
            && chosen != current
        {
            state.statistics.select(id, state.scene);
        }
        ui.separator();
        let trackable = chosen
            .and_then(|id| state.scene.objects.get(id))
            .is_some_and(|object| object.kind().trackable());
        let mut group = state.statistics.group();
        ui.selectable_value(&mut group, Group::Object, "objects");
        if trackable {
            ui.selectable_value(&mut group, Group::Track, "tracks");
        }
        ui.selectable_value(&mut group, Group::Timepoint, "timepoints");
        if group != state.statistics.group() {
            state.statistics.set_group(group, state.scene);
        }
        ui.separator();
        ui.add_enabled_ui(state.statistics.group() != Group::Object, |ui| {
            let mut aggregate = state.statistics.aggregate();
            egui::ComboBox::from_id_salt("grana.statistics.aggregate")
                .selected_text(aggregate.name())
                .show_ui(ui, |ui| {
                    for option in [
                        Aggregate::Mean,
                        Aggregate::Sum,
                        Aggregate::Minimum,
                        Aggregate::Maximum,
                        Aggregate::Deviation,
                    ] {
                        ui.selectable_value(&mut aggregate, option, option.name());
                    }
                });
            if aggregate != state.statistics.aggregate() {
                state.statistics.set_aggregate(aggregate, state.scene);
            }
        });
        let total = state.statistics.table().map_or(0, |table| table.rows());
        let shown = state.statistics.rows().len();
        ui.label(
            RichText::new(format!("{shown} of {total} rows"))
                .small()
                .weak(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Hide").clicked() {
                state.statistics.set_open(false);
            }
            if ui.button("Export CSV").clicked() {
                actions.export_statistics = true;
            }
            if state.statistics.table().is_some() {
                ui.menu_button("Columns", |ui| columns_menu(ui, state));
            }
        });
    });
}

fn filters(ui: &mut Ui, state: &mut UiState<'_>) {
    let Some(table) = state.statistics.table() else {
        return;
    };
    let columns = table
        .columns()
        .iter()
        .map(|column| {
            (
                column.label().to_string(),
                column.unit().suffix().to_string(),
            )
        })
        .collect::<Vec<_>>();
    ui.horizontal_wrapped(|ui| {
        if ui.button("Add filter").clicked() {
            state.statistics.add_filter();
        }
        let mut visibility = state.statistics.visibility();
        if ui
            .checkbox(&mut visibility, "hide filtered objects")
            .changed()
        {
            state.statistics.set_visibility(visibility);
        }
        let filters = state.statistics.filters().to_vec();
        let mut removed = None;
        for (index, filter) in filters.iter().enumerate() {
            ui.group(|ui| {
                let mut column = filter.column();
                egui::ComboBox::from_id_salt(("grana.statistics.filter", index))
                    .selected_text(column_text(&columns, column))
                    .show_ui(ui, |ui| {
                        for (candidate, (name, unit)) in columns.iter().enumerate() {
                            let text = if unit.is_empty() {
                                name.clone()
                            } else {
                                format!("{name} ({unit})")
                            };
                            ui.selectable_value(&mut column, candidate, text);
                        }
                    });
                let mut minimum = filter.minimum();
                let mut maximum = filter.maximum();
                let speed = ((maximum - minimum).abs().max(1.0)) / 200.0;
                let changed = ui
                    .add(
                        egui::DragValue::new(&mut minimum)
                            .speed(speed)
                            .max_decimals(4),
                    )
                    .changed()
                    | ui.add(
                        egui::DragValue::new(&mut maximum)
                            .speed(speed)
                            .max_decimals(4),
                    )
                    .changed();
                if ui.small_button("×").clicked() {
                    removed = Some(index);
                } else if column != filter.column() {
                    state.statistics.retarget_filter(index, column);
                } else if changed {
                    let (low, high) = if minimum <= maximum {
                        (minimum, maximum)
                    } else {
                        (maximum, minimum)
                    };
                    state
                        .statistics
                        .set_filter(index, Filter::new(column, low, high));
                }
            });
        }
        if let Some(index) = removed {
            state.statistics.remove_filter(index);
        }
    });
}

fn table(ui: &mut Ui, state: &mut UiState<'_>, actions: &mut Actions) {
    let Some(set) = state.statistics.set() else {
        return;
    };
    let Some(table) = state.statistics.table() else {
        ui.label(
            RichText::new("this object set has no detection results yet")
                .small()
                .weak(),
        );
        return;
    };
    if table.rows() == 0 {
        let message = match state.statistics.group() {
            Group::Track => "no tracks yet: track the objects to summarise them",
            Group::Object | Group::Timepoint => "no detections to summarise",
        };
        ui.label(RichText::new(message).small().weak());
        return;
    }
    let visible = state.statistics.visible().to_vec();
    let columns = table
        .columns()
        .iter()
        .enumerate()
        .filter(|(index, _)| visible.get(*index).copied().unwrap_or(true))
        .map(|(index, column)| (index, column.clone()))
        .collect::<Vec<_>>();
    if columns.is_empty() {
        ui.label(RichText::new("no columns are shown").small().weak());
        return;
    }
    let height = ui.available_height();
    egui::ScrollArea::horizontal()
        .id_salt(("grana.statistics.columns", set.raw()))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let mut builder = TableBuilder::new(ui)
                .id_salt(("grana.statistics", set.raw()))
                .striped(true)
                .resizable(true)
                .sense(egui::Sense::click())
                .min_scrolled_height(0.0)
                .max_scroll_height(height)
                .auto_shrink([false, false]);
            for (_, column) in &columns {
                builder =
                    builder.column(TableColumn::initial(width(column)).at_least(MIN_COLUMN_WIDTH));
            }
            builder
                .header(HEADER_HEIGHT, |mut header| {
                    for (index, column) in &columns {
                        header.col(|ui| {
                            let sorted = state
                                .statistics
                                .sort()
                                .filter(|(current, _)| current == index)
                                .map(|(_, descending)| descending);
                            let text = match sorted {
                                Some(descending) => format!(
                                    "{} {}",
                                    label(column),
                                    if descending { "▼" } else { "▲" }
                                ),
                                None => label(column),
                            };
                            if ui
                                .add(egui::Button::new(RichText::new(text).small()).frame(false))
                                .clicked()
                            {
                                state.statistics.toggle_sort(*index);
                            }
                        });
                    }
                })
                .body(|body| {
                    let shown = state.statistics.rows().len();
                    body.rows(ROW_HEIGHT, shown, |mut cells| {
                        let row = cells.index();
                        let Some(table) = state.statistics.table() else {
                            return;
                        };
                        let rows = state.statistics.rows();
                        let index = rows[row];
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
                        cells.set_selected(selected);
                        for (column, spec) in &columns {
                            let value = table.value(index, *column);
                            cells.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.monospace(spec.format(value));
                                    },
                                );
                            });
                        }
                        if cells.response().clicked() {
                            match (
                                observation.time(),
                                observation.object(),
                                observation.track(),
                            ) {
                                (Some(time), Some(object), _) => {
                                    actions.time = Some(time);
                                    actions.select = Some(Selection {
                                        set,
                                        item: Item::Object(object),
                                    });
                                }
                                (_, _, Some(track)) => {
                                    actions.select = Some(Selection {
                                        set,
                                        item: Item::Track(track),
                                    });
                                }
                                (Some(time), _, _) => actions.time = Some(time),
                                _ => {}
                            }
                        }
                    });
                });
        });
}

fn columns_menu(ui: &mut Ui, state: &mut UiState<'_>) {
    let Some(table) = state.statistics.table() else {
        return;
    };
    let columns = table.columns().to_vec();
    egui::ScrollArea::vertical()
        .id_salt("grana.statistics.column_menu")
        .max_height(360.0)
        .show(ui, |ui| {
            if ui.button("Show all").clicked() {
                state.statistics.show_all_columns();
            }
            for (index, column) in columns.iter().enumerate() {
                let mut visible = state
                    .statistics
                    .visible()
                    .get(index)
                    .copied()
                    .unwrap_or(false);
                if ui.checkbox(&mut visible, label(column)).changed() {
                    state.statistics.set_column_visible(index, visible);
                }
            }
        });
}

fn label(column: &Column) -> String {
    let unit = column.unit().suffix();
    if unit.is_empty() {
        column.label().to_string()
    } else {
        format!("{} ({unit})", column.label())
    }
}

fn column_text(columns: &[(String, String)], index: usize) -> String {
    let Some((name, unit)) = columns.get(index) else {
        return String::new();
    };
    if unit.is_empty() {
        name.clone()
    } else {
        format!("{name} ({unit})")
    }
}

fn width(column: &Column) -> f32 {
    let header = label(column).chars().count() as f32 * 6.4 + 22.0;
    let value = (f32::from(column.decimals()) + 6.0) * 7.0 + 12.0;
    header.max(value).clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH)
}
