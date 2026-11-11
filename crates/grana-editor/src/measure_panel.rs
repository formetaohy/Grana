use glam::Vec2;
use grana_measure::{Aggregate, Column, Filter, Group, MeasureView, PlotAxis, Scope};
use grana_scene::{Item, ObjectSetId, Selection};
use grana_session::Command;
use grana_ui::{
    Align, Axis, BuildContext, Color, Corners, Cursor, Element, Icon, Key, Length, Level, Rect,
    Role, State, TextStyle, icon,
};

use crate::message::Message;
use crate::state::View;
use crate::view;

const ROW_HEIGHT: f32 = 26.0;
const HEADER_HEIGHT: f32 = 30.0;
const MIN_COLUMN_WIDTH: f32 = 58.0;
const TOOLBAR_HEIGHT: f32 = 30.0;

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    rect: Rect,
) -> Vec<Element<'a, Message>> {
    if state.scene.objects.is_empty() {
        return vec![view::empty_state(
            ctx,
            Icon::Table,
            "No Measurements Yet",
            "Add and detect an object set to build a measurement table, then filter, sort and export it.",
        )
        .at(rect)];
    }
    let mut elements = Vec::new();
    let mut top = rect.min.y;
    elements.extend(toolbar(
        ctx,
        state,
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), TOOLBAR_HEIGHT)),
    ));
    top += TOOLBAR_HEIGHT;
    let measured = state.measure.table().is_some();
    let mode = state.measure.view();
    if measured && mode != MeasureView::Table {
        let bar = Rect::from_min_size(
            Vec2::new(rect.min.x, top + 4.0),
            Vec2::new(rect.width(), 28.0),
        );
        elements.extend(plot_bar(ctx, state, bar));
        top += 32.0;
    }
    elements.extend(filters(ctx, state, rect, &mut top));
    top += 6.0;
    let body = Rect::from_min_max(Vec2::new(rect.min.x, top), rect.max);
    if measured && mode != MeasureView::Table {
        elements.extend(crate::plot::view(ctx, state, body));
    } else {
        elements.extend(table(ctx, state, body));
    }
    elements
}

fn toolbar<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    row: Rect,
) -> Vec<Element<'a, Message>> {
    let sets = state
        .scene
        .objects
        .iter()
        .map(|set| (set.id(), set.name().to_string()))
        .collect::<Vec<_>>();
    let current = state.measure.set();
    let chosen = current
        .or_else(|| sets.first().map(|(id, _)| *id))
        .unwrap_or_else(|| sets[0].0);
    let selected = sets.iter().position(|(id, _)| *id == chosen).unwrap_or(0);
    let trackable = state
        .scene
        .objects
        .get(chosen)
        .is_some_and(|object| object.kind().trackable());
    let group = state.measure.group();
    let mut options = vec!["Objects".to_string()];
    if trackable {
        options.push("Tracks".to_string());
    }
    options.push("Frames".to_string());
    let group_index = match group {
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
    let mut items = vec![
        field_label(ctx, "Set"),
        combo(
            ctx,
            "measure.set",
            140.0,
            selected,
            &sets
                .iter()
                .map(|(_, name)| name.clone())
                .collect::<Vec<_>>(),
            move |index| {
                let id = sets.get(index).map(|(id, _)| *id);
                match id {
                    Some(id) if current != Some(id) => {
                        Message::Command(Command::SetMeasureSet(Some(id)))
                    }
                    _ => Message::Noop,
                }
            },
        ),
    ];
    let group_options = options.iter().map(String::as_str).collect::<Vec<_>>();
    items.push(view::segmented(
        ctx,
        "measure.group",
        &group_options,
        group_index,
        move |index| {
            let next = match index {
                0 => Group::Object,
                1 if trackable => Group::Track,
                _ => Group::Timepoint,
            };
            if next != group {
                Message::Command(Command::SetMeasureGroup(next))
            } else {
                Message::Noop
            }
        },
    ));
    if group != Group::Object {
        let aggregate = state.measure.aggregate();
        let aggregate_index = Aggregate::ALL
            .iter()
            .position(|value| *value == aggregate)
            .unwrap_or(0);
        let labels = Aggregate::ALL
            .iter()
            .map(|value| value.label())
            .collect::<Vec<_>>();
        items.push(combo(
            ctx,
            "measure.aggregate",
            96.0,
            aggregate_index,
            &labels,
            move |index| match Aggregate::ALL.get(index) {
                Some(value) if *value != aggregate => {
                    Message::Command(Command::SetMeasureAggregate(*value))
                }
                _ => Message::Noop,
            },
        ));
    }
    if state.measure.table().is_some() && group != Group::Track {
        let scope = state.measure.scope();
        let scope_index = Scope::ALL
            .iter()
            .position(|value| *value == scope)
            .unwrap_or(0);
        items.push(view::segmented(
            ctx,
            "measure.scope",
            &["All", "Current"],
            scope_index,
            move |index| match Scope::ALL.get(index) {
                Some(value) if *value != scope => {
                    Message::Command(Command::SetMeasureScope(*value))
                }
                _ => Message::Noop,
            },
        ));
    }
    let left = grana_ui::row()
        .gap(8.0)
        .align(Align::Center)
        .at(Rect::from_min_size(
            row.min,
            Vec2::new(row.width(), TOOLBAR_HEIGHT),
        ))
        .children(items);
    let labels = ["Table", "Histogram", "Scatter"];
    let mode = state.measure.view();
    let total = state.measure.total();
    let shown = state.measure.rows().len();
    let caption = if state.scene.region.enabled() {
        format!("{shown}/{total} rows · ROI")
    } else {
        format!("{shown}/{total} rows")
    };
    let caption_style = TextStyle::new(
        ctx.theme().font(Level::Caption),
        ctx.theme().palette.text_tertiary,
    );
    let caption_width = ctx.measure(&caption, caption_style).x;
    let segmented_width = if state.measure.table().is_some() {
        segmented_width(ctx, &labels)
    } else {
        0.0
    };
    let right_width = segmented_width + caption_width + 3.0 * 28.0 + 4.0 * 8.0 + 4.0;
    let right = Rect::from_min_size(
        Vec2::new((row.max.x - right_width).max(row.min.x), row.min.y),
        Vec2::new(right_width.min(row.width()), TOOLBAR_HEIGHT),
    );
    let mut right_items = Vec::new();
    if state.measure.table().is_some() {
        let index = match mode {
            MeasureView::Table => 0,
            MeasureView::Histogram => 1,
            MeasureView::Scatter => 2,
        };
        right_items.push(view::segmented(
            ctx,
            "measure.view",
            &labels,
            index,
            move |index| match MeasureView::ALL.get(index) {
                Some(value) if *value != mode => Message::Command(Command::SetMeasureView(*value)),
                _ => Message::Noop,
            },
        ));
    }
    right_items.push(view::caption(ctx, &caption));
    right_items.push(view::icon_button(
        ctx,
        "measure.filter",
        Icon::Filter,
        28.0,
        Role::Quiet,
        "Add filter",
        Message::Command(Command::AddFilter),
    ));
    right_items.push(view::icon_button(
        ctx,
        "measure.columns",
        Icon::Columns,
        28.0,
        Role::Quiet,
        "Choose columns",
        Message::Noop,
    ));
    right_items.push(view::icon_button(
        ctx,
        "measure.export",
        Icon::Export,
        28.0,
        Role::Quiet,
        "Export CSV",
        Message::Command(Command::ExportMeasure),
    ));
    let right_row = grana_ui::row()
        .gap(8.0)
        .align(Align::Center)
        .justify(Align::End)
        .children(right_items);
    let mut children = vec![left, right_row.at(right)];
    children.push(columns_menu(ctx, state));
    children
}

fn columns_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let Some(table) = state.measure.table() else {
        return grana_ui::column();
    };
    let visible = state.measure.visible();
    let mut rows = vec![
        grana_ui::widget::menu_row(&theme, "Show All Columns", false)
            .named("measure.show.all")
            .press(Message::Command(Command::ShowAllColumns)),
        grana_ui::widget::separator().named("measure.columns.separator"),
    ];
    for (index, column) in table.columns().iter().enumerate() {
        let shown = visible.get(index).copied().unwrap_or(false);
        rows.push(
            grana_ui::widget::menu_row(&theme, &view::column_label(column), shown)
                .key(Key::index("measure.column", index))
                .press(Message::Command(Command::SetColumnVisible {
                    index,
                    visible: !shown,
                })),
        );
    }
    grana_ui::popover::popup(Key::new("measure.columns"), 240.0, rows)
}

fn plot_bar<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    rect: Rect,
) -> Vec<Element<'a, Message>> {
    let Some(table) = state.measure.table() else {
        return Vec::new();
    };
    let view = state.measure.view();
    let axes = state.measure.axes();
    let options = table
        .columns()
        .iter()
        .enumerate()
        .map(|(index, column)| (index, view::column_label(column)))
        .collect::<Vec<_>>();
    let labels = options
        .iter()
        .map(|(_, label)| label.clone())
        .collect::<Vec<_>>();
    let lasso = state.measure.lasso().is_some();
    let gated = lasso
        || state.measure.filters().iter().any(|filter| {
            filter.column() == axes[0]
                || (view == MeasureView::Scatter && filter.column() == axes[1])
        });
    let selected_x = options
        .iter()
        .position(|(index, _)| *index == axes[0])
        .unwrap_or(0);
    let selected_y = options
        .iter()
        .position(|(index, _)| *index == axes[1])
        .unwrap_or(0);
    let options_x = options.clone();
    let mut items = vec![
        field_label(ctx, "X Axis"),
        combo(
            ctx,
            "measure.plot.x",
            180.0,
            selected_x,
            &labels,
            move |index| match options_x.get(index) {
                Some((column, _)) if *column != axes[0] => Message::Command(Command::SetPlotAxis {
                    axis: PlotAxis::X,
                    column: *column,
                }),
                _ => Message::Noop,
            },
        ),
    ];
    if view == MeasureView::Scatter {
        items.push(field_label(ctx, "Y Axis"));
        items.push(combo(
            ctx,
            "measure.plot.y",
            180.0,
            selected_y,
            &labels,
            move |index| match options.get(index) {
                Some((column, _)) if *column != axes[1] => Message::Command(Command::SetPlotAxis {
                    axis: PlotAxis::Y,
                    column: *column,
                }),
                _ => Message::Noop,
            },
        ));
    }
    if gated {
        items.push(view::button(ctx, "Clear Gate", Role::Quiet).press(Message::ClearGates));
    }
    vec![
        grana_ui::row()
            .gap(8.0)
            .align(Align::Center)
            .at(rect)
            .children(items),
    ]
}

fn filters<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    rect: Rect,
    top: &mut f32,
) -> Vec<Element<'a, Message>> {
    let mut elements = Vec::new();
    let Some(table) = state.measure.table() else {
        return elements;
    };
    let filters = state.measure.filters().to_vec();
    let filtered = !filters.is_empty();
    if filtered || state.measure.lasso().is_some() {
        let row = Rect::from_min_size(
            Vec2::new(rect.min.x, *top + 4.0),
            Vec2::new(rect.width(), 28.0),
        );
        let theme = *ctx.theme();
        let visibility = state.measure.visibility();
        let mut items = vec![
            grana_ui::widget::checkbox(&theme, "Hide Filtered Rows", visibility)
                .key(Key::new("hide.filtered"))
                .on_toggle(|value| Message::Command(Command::SetMeasureVisibility(value))),
            view::button(ctx, "Extract Filtered", Role::Tinted)
                .press(Message::Command(Command::ExtractFiltered)),
            view::button(ctx, "Delete Excluded", Role::Danger)
                .press(Message::Command(Command::DeleteFiltered)),
        ];
        if !filtered {
            items.remove(0);
        }
        elements.push(
            grana_ui::row()
                .gap(8.0)
                .align(Align::Center)
                .at(row)
                .children(items),
        );
        *top += 32.0;
    }
    let options = table
        .columns()
        .iter()
        .enumerate()
        .map(|(index, column)| (index, view::column_label(column)))
        .collect::<Vec<_>>();
    let labels = options
        .iter()
        .map(|(_, label)| label.clone())
        .collect::<Vec<_>>();
    let surface = ctx.theme().surface(Role::Card, State::Rest);
    for (index, filter) in filters.iter().enumerate() {
        let card = Rect::from_min_size(
            Vec2::new(rect.min.x, *top + 4.0),
            Vec2::new(rect.width(), 36.0),
        );
        let remove = Rect::from_center_size(
            Vec2::new(card.max.x - 22.0, card.center().y),
            Vec2::splat(22.0),
        );
        let controls = Rect::from_min_max(
            Vec2::new(card.min.x + 10.0, card.min.y + 4.0),
            Vec2::new(remove.min.x - 8.0, card.max.y - 4.0),
        );
        let theme = *ctx.theme();
        let mut surface = surface;
        surface.corners = Corners::all(theme.metrics.card_radius);
        elements.push(grana_ui::column().at(card).background(surface));
        let selected = options
            .iter()
            .position(|(column, _)| *column == filter.column())
            .unwrap_or(0);
        let column_index = filter.column();
        let minimum = filter.minimum();
        let maximum = filter.maximum();
        let speed = ((maximum - minimum).abs().max(1.0)) / 200.0;
        let minimum_value = grana_ui::widget::drag_value(&theme, minimum, 4)
            .key(Key::index("measure.filter.minimum", index))
            .on_drag_value(speed, move |value| {
                let low = value.min(maximum);
                let high = value.max(maximum);
                if (low - minimum).abs() > f32::EPSILON || (high - maximum).abs() > f32::EPSILON {
                    Message::Command(Command::SetFilter {
                        index,
                        filter: Filter::new(column_index, low, high),
                    })
                } else {
                    Message::Noop
                }
            });
        let maximum_value = grana_ui::widget::drag_value(&theme, maximum, 4)
            .key(Key::index("measure.filter.maximum", index))
            .on_drag_value(speed, move |value| {
                let low = minimum.min(value);
                let high = minimum.max(value);
                if (low - minimum).abs() > f32::EPSILON || (high - maximum).abs() > f32::EPSILON {
                    Message::Command(Command::SetFilter {
                        index,
                        filter: Filter::new(column_index, low, high),
                    })
                } else {
                    Message::Noop
                }
            });
        let options_for_combo = options.clone();
        let combo_key = format!("measure.filter.column.{index}");
        let remove_key = format!("measure.filter.remove.{index}");
        let row = grana_ui::row()
            .gap(6.0)
            .align(Align::Center)
            .at(controls)
            .children([
                field_label(ctx, "Filter"),
                combo(ctx, &combo_key, 180.0, selected, &labels, move |choice| {
                    match options_for_combo.get(choice) {
                        Some((column, _)) if *column != column_index => {
                            Message::Command(Command::RetargetFilter {
                                index,
                                column: *column,
                            })
                        }
                        _ => Message::Noop,
                    }
                }),
                minimum_value,
                view::caption(ctx, "–"),
                maximum_value,
            ]);
        elements.push(row);
        elements.push(
            view::icon_button(
                ctx,
                &remove_key,
                Icon::Close,
                22.0,
                Role::Quiet,
                "Remove filter",
                Message::Command(Command::RemoveFilter(index)),
            )
            .at(remove),
        );
        *top += 40.0;
    }
    elements
}

fn table<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    rect: Rect,
) -> Vec<Element<'a, Message>> {
    let Some(set) = state.measure.set() else {
        return vec![
            view::empty_state(
                ctx,
                Icon::Table,
                "No Object Set Selected",
                "Choose an object set to inspect its measurements.",
            )
            .at(rect),
        ];
    };
    let Some(table) = state.measure.table() else {
        return vec![
            if state.measuring {
                view::empty_state(
                    ctx,
                    Icon::Table,
                    "Measuring",
                    "The measurements are computed in the background and appear here when they settle.",
                )
            } else {
                view::empty_state(
                    ctx,
                    Icon::Table,
                    "No Detection Yet",
                    "Detect this object set to build its measurement table.",
                )
            }
            .at(rect),
        ];
    };
    if table.rows() == 0 {
        let detail = match state.measure.group() {
            Group::Track => "Track the objects to summarise their trajectories.",
            Group::Object | Group::Timepoint => {
                "Detect objects in the region to build a measurement table."
            }
        };
        return vec![view::empty_state(ctx, Icon::Table, "No Rows", detail).at(rect)];
    }
    let visible = state.measure.visible();
    let columns = table
        .columns()
        .iter()
        .enumerate()
        .filter(|(index, _)| visible.get(*index).copied().unwrap_or(true))
        .map(|(index, column)| (index, column.clone()))
        .collect::<Vec<_>>();
    if columns.is_empty() {
        return vec![
            view::empty_state(
                ctx,
                Icon::Columns,
                "No Visible Columns",
                "Choose at least one column from the columns menu.",
            )
            .at(rect),
        ];
    }
    let available = rect.width();
    let widths = column_widths(state, &columns, available - 1.0);
    let total = widths.iter().sum::<f32>();
    let surface = ctx.theme().surface(Role::Card, State::Rest);
    let palette = ctx.theme().palette;
    let theme = *ctx.theme();
    let mut elements = vec![
        grana_ui::canvas(Length::Fill, rect.height(), move |painter, info| {
            painter.outlined(
                info.rect,
                surface.corners,
                surface.fill,
                surface.border,
                surface.shadow,
            );
        })
        .at(rect),
    ];
    let band = Rect::from_min_size(rect.min, Vec2::new(total, HEADER_HEIGHT));
    elements.push(
        grana_ui::canvas(Length::Fill, HEADER_HEIGHT, move |painter, info| {
            painter.rect(info.rect, Corners::top(16.0), Color::white_alpha(0.025));
            painter.rect(
                Rect::from_min_size(
                    Vec2::new(info.rect.min.x, info.rect.max.y - 1.0),
                    Vec2::new(info.rect.width(), 1.0),
                ),
                Corners::ZERO,
                palette.border,
            );
        })
        .at(band),
    );
    let mut left = band.min.x;
    let sort = state.measure.sort();
    for ((index, column), width) in columns.iter().zip(widths.iter()) {
        let width = *width;
        let cell =
            Rect::from_min_size(Vec2::new(left, band.min.y), Vec2::new(width, HEADER_HEIGHT));
        let sorted = sort
            .filter(|(current, _)| *current == *index)
            .map(|(_, descending)| descending);
        let label = view::column_label(column);
        let style = TextStyle::new(
            theme.label_font(),
            if sorted.is_some() {
                palette.text_primary
            } else {
                palette.text_tertiary
            },
        )
        .aligned(Align::End)
        .truncating();
        let icon_color = palette.text_secondary;
        let arrow = match sorted {
            Some(true) => Some(Icon::ChevronDown),
            Some(false) => Some(Icon::ChevronUp),
            None => None,
        };
        elements.push(
            grana_ui::canvas(Length::Fixed(width), HEADER_HEIGHT, move |painter, info| {
                if info.hover > 0.01 {
                    painter.rect(
                        info.rect,
                        Corners::ZERO,
                        palette.material_hover.with_alpha(0.55 * info.hover),
                    );
                }
                painter.text_in(
                    Rect::from_min_max(
                        info.rect.min + Vec2::new(8.0, 0.0),
                        Vec2::new(
                            info.rect.max.x - arrow.map_or(8.0, |_| 16.0),
                            info.rect.max.y,
                        ),
                    ),
                    &label,
                    style,
                    Align::Center,
                );
                if let Some(arrow) = arrow {
                    icon::paint(
                        arrow,
                        painter,
                        Vec2::new(info.rect.max.x - 9.0, info.rect.center().y),
                        9.0,
                        icon_color,
                        1.4,
                    );
                }
            })
            .at(cell)
            .key(Key::index("measure.head", *index))
            .press(Message::Command(Command::ToggleSort(*index))),
        );
        let grip = Rect::from_min_max(
            Vec2::new(cell.max.x - 3.0, cell.min.y),
            Vec2::new(cell.max.x + 1.0, cell.max.y),
        );
        let grip_key = Key::index("measure.width", *index);
        let column = *index;
        let spare = (available - 1.0 - total).max(0.0);
        let ceiling = (width + spare).max(MIN_COLUMN_WIDTH);
        elements.push(
            grana_ui::canvas(Length::Fixed(4.0), HEADER_HEIGHT, |_, _| {})
                .at(grip)
                .key(grip_key)
                .cursor(Cursor::ResizeHorizontal)
                .on_pointer(move |pointer| {
                    (pointer.phase == grana_ui::Phase::Move).then_some(Message::SetColumnWidth {
                        index: column,
                        width: (width + pointer.delta.x).clamp(MIN_COLUMN_WIDTH, ceiling),
                    })
                }),
        );
        left += width;
    }
    let shown = state.measure.rows().len();
    let content = shown as f32 * ROW_HEIGHT;
    let body = Rect::from_min_max(
        Vec2::new(rect.min.x, band.max.y),
        Vec2::new(rect.min.x + total.max(1.0), rect.max.y),
    );
    let rows_key = Key::new("measure.rows");
    let offset = ctx.scroll_offset(rows_key);
    let first = (offset / ROW_HEIGHT).floor().max(0.0) as usize;
    let visible_rows = (body.height() / ROW_HEIGHT).ceil() as usize + 1;
    let last = (first + visible_rows).min(shown);
    let rows = state.measure.rows();
    let time = state.scene.time();
    let selection = state.scene.selection();
    let accent = palette.accent;
    let font = theme.font(Level::Monospace);
    let text_primary = palette.text_primary;
    let text_secondary = palette.text_secondary;
    let separator = palette.separator;
    let hover = palette.material_hover;
    let mut children = vec![grana_ui::canvas(
        Length::Fill,
        content,
        move |painter, info| {
            for row in first..last {
                let Some(index) = state_index(rows, row) else {
                    continue;
                };
                let observation = table.observation(index);
                let selected = matches_selection(selection, set, observation, time);
                let row_rect = Rect::from_min_size(
                    Vec2::new(info.rect.min.x, info.rect.min.y + row as f32 * ROW_HEIGHT),
                    Vec2::new(total, ROW_HEIGHT),
                );
                if selected {
                    painter.rect(row_rect, Corners::ZERO, accent.with_alpha(0.16));
                    painter.rect(
                        Rect::from_min_size(row_rect.min, Vec2::new(2.5, row_rect.height())),
                        Corners::ZERO,
                        accent,
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
                    separator,
                );
                let mut left = row_rect.min.x;
                for ((column_index, column), width) in columns.iter().zip(widths.iter()) {
                    let cell = Rect::from_min_size(
                        Vec2::new(left, row_rect.min.y),
                        Vec2::new(*width, ROW_HEIGHT),
                    );
                    painter.text_in(
                        cell.shrink2(Vec2::new(8.0, 0.0)),
                        &column.format(table.value(index, *column_index)),
                        TextStyle::new(
                            font,
                            if selected {
                                text_primary
                            } else {
                                text_secondary
                            },
                        )
                        .aligned(Align::End),
                        Align::Center,
                    );
                    left += *width;
                }
            }
        },
    )];
    for row in first..last {
        let row_rect = Rect::from_min_size(
            Vec2::new(body.min.x, body.min.y + row as f32 * ROW_HEIGHT - offset),
            Vec2::new(total, ROW_HEIGHT),
        );
        children.push(
            grana_ui::canvas(Length::Fill, ROW_HEIGHT, move |painter, info| {
                if info.hover > 0.01 {
                    painter.rect(
                        info.rect,
                        Corners::ZERO,
                        hover.with_alpha(0.65 * info.hover),
                    );
                }
            })
            .at(row_rect)
            .key(Key::index("measure.row", row))
            .on_pointer(move |pointer| {
                (pointer.phase == grana_ui::Phase::Up && row_rect.contains(pointer.position))
                    .then_some(Message::Locate(row))
            }),
        );
    }
    let scroller = Element::new(grana_ui::Layout::column())
        .at(body)
        .scroll(Axis::Column, Length::Fill, Some(content + 4.0))
        .clipped()
        .children(children);
    elements.push(scroller);
    elements
}

fn state_index(rows: &[usize], row: usize) -> Option<usize> {
    rows.get(row).copied()
}

fn matches_selection(
    selection: Option<Selection>,
    set: ObjectSetId,
    observation: grana_measure::Observation,
    time: u32,
) -> bool {
    match (observation.time(), observation.object()) {
        (Some(object_time), Some(object)) => {
            selection
                == Some(Selection {
                    set,
                    item: Item::Object(object),
                })
                && time == object_time
        }
        _ => match observation.track() {
            Some(track) => {
                selection
                    == Some(Selection {
                        set,
                        item: Item::Track(track),
                    })
            }
            None => false,
        },
    }
}

fn field_label<'a>(ctx: &mut BuildContext<'_, Message>, text: &str) -> Element<'a, Message> {
    let theme = *ctx.theme();
    grana_ui::text(
        text,
        TextStyle::new(theme.label_font(), theme.palette.text_tertiary),
    )
}

fn segmented_width(ctx: &mut BuildContext<'_, Message>, labels: &[&str]) -> f32 {
    let theme = *ctx.theme();
    let style = TextStyle::new(theme.font(Level::Callout).medium(), Color::WHITE);
    labels
        .iter()
        .map(|label| ctx.measure(label, style).x + 24.0)
        .sum::<f32>()
        + 6.0
}

fn combo<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    width: f32,
    selected: usize,
    labels: &[String],
    message: impl Fn(usize) -> Message + 'static,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let label = labels.get(selected).cloned().unwrap_or_default();
    let style =
        TextStyle::new(theme.font(Level::Callout).medium(), palette.text_primary).truncating();
    let rest = theme.surface(Role::Control, State::Rest);
    let hover = theme.surface(Role::Control, State::Hover);
    let anchor = grana_ui::canvas(Length::Fixed(width), 28.0, move |painter, info| {
        let surface = if info.hover > 0.5 { hover } else { rest };
        painter.outlined(
            info.rect,
            surface.corners,
            surface.fill,
            surface.border,
            surface.shadow,
        );
        painter.text_in(
            Rect::from_min_max(
                info.rect.min + Vec2::new(10.0, 0.0),
                Vec2::new(info.rect.max.x - 22.0, info.rect.max.y),
            ),
            &label,
            style,
            Align::Center,
        );
        icon::chevron(
            painter,
            Vec2::new(info.rect.max.x - 11.0, info.rect.center().y),
            5.0,
            std::f32::consts::FRAC_PI_2,
            palette.text_tertiary,
            1.5,
        );
    })
    .named(key)
    .press(Message::Noop)
    .cursor(Cursor::Pointing);
    let mut rows = Vec::with_capacity(labels.len());
    for (index, label) in labels.iter().enumerate() {
        rows.push(
            grana_ui::widget::menu_row(&theme, label, index == selected)
                .key(Key::index(key, index))
                .press(message(index)),
        );
    }
    let popup = grana_ui::popover::popup(Key::new(key), width.max(120.0), rows);
    grana_ui::column()
        .width(Length::Auto)
        .children([anchor, popup])
}

fn column_widths(state: &View<'_>, columns: &[(usize, Column)], available: f32) -> Vec<f32> {
    let initial = columns
        .iter()
        .map(|(_, column)| {
            let header = view::column_label(column).chars().count() as f32 * 5.9 + 22.0;
            let value = (f32::from(column.decimals()) + 6.0) * 7.0 + 18.0;
            header.max(value).max(MIN_COLUMN_WIDTH)
        })
        .collect::<Vec<_>>();
    let total = initial.iter().sum::<f32>();
    let mut widths: Vec<f32> = if total <= available {
        let extra = (available - total) / columns.len().max(1) as f32;
        initial.iter().map(|width| width + extra).collect()
    } else {
        let scale = available / total;
        initial.iter().map(|width| width * scale).collect()
    };
    for (slot, (index, _)) in columns.iter().enumerate() {
        if let Some(width) = state.workspace.column_widths.get(index) {
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
