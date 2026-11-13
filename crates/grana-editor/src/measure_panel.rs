use glam::Vec2;
use grana_i18n::Language;
use grana_measure::{Aggregate, Column, Filter, Group, MeasureView, PlotAxis, Scope};
use grana_scene::{Item, ObjectSetId, Selection};
use grana_session::Command;
use grana_ui::{
    Align, Axis, BuildContext, Color, Corners, Cursor, Element, Icon, Key, Length, Level, Rect,
    Role, State, TextStyle, icon,
};

use crate::message::Message;
use crate::state::{DistributionStyle, View};
use crate::view;

const ROW_HEIGHT: f32 = 28.0;
const HEADER_HEIGHT: f32 = 32.0;
const MIN_COLUMN_WIDTH: f32 = 58.0;
const TOOLBAR_HEIGHT: f32 = 32.0;

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    rect: Rect,
) -> Vec<Element<'a, Message>> {
    if state.scene.objects.is_empty() {
        let language = ctx.language();
        return vec![
            view::empty_state(
                ctx,
                Icon::Table,
                grana_i18n::measure::no_measurements(language),
                grana_i18n::measure::no_measurements_detail(language),
            )
            .at(rect),
        ];
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
    if measured && mode != MeasureView::Table && mode.columned() {
        let bar = Rect::from_min_size(
            Vec2::new(rect.min.x, top + 6.0),
            Vec2::new(rect.width(), 30.0),
        );
        elements.extend(plot_bar(ctx, state, bar));
        top += 36.0;
    }
    elements.extend(filters(ctx, state, rect, &mut top));
    top += 8.0;
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
    let molecule = state
        .scene
        .objects
        .get(chosen)
        .and_then(|object| object.localization())
        .is_some_and(grana_scene::LocalizationSource::clustered);
    let group = state.measure.group();
    let language = ctx.language();
    let mut options = vec![grana_i18n::common::objects(language).to_string()];
    let mut groups = vec![Group::Object];
    if trackable {
        options.push(grana_i18n::common::tracks(language).to_string());
        groups.push(Group::Track);
    }
    if molecule {
        options.push(grana_i18n::common::molecules(language).to_string());
        groups.push(Group::Molecule);
    }
    options.push(grana_i18n::common::frames(language).to_string());
    groups.push(Group::Timepoint);
    let group_index = groups.iter().position(|value| *value == group).unwrap_or(0);
    let mut items = vec![
        field_label(ctx, grana_i18n::measure::set(language)),
        combo(
            ctx,
            "measure.set",
            148.0,
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
        move |index| match groups.get(index) {
            Some(next) if *next != group => Message::Command(Command::SetMeasureGroup(*next)),
            _ => Message::Noop,
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
            .map(|value| aggregate_label(language, *value))
            .collect::<Vec<_>>();
        items.push(combo(
            ctx,
            "measure.aggregate",
            98.0,
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
    if state.measure.table().is_some() && !matches!(group, Group::Track | Group::Molecule) {
        let scope = state.measure.scope();
        let scope_index = Scope::ALL
            .iter()
            .position(|value| *value == scope)
            .unwrap_or(0);
        items.push(view::segmented(
            ctx,
            "measure.scope",
            &[
                grana_i18n::common::all(language),
                grana_i18n::common::current(language),
            ],
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
    let labels = MeasureView::ALL
        .iter()
        .copied()
        .map(|value| view_label(language, value))
        .map(String::from)
        .collect::<Vec<_>>();
    let mode = state.measure.view();
    let total = state.measure.total();
    let shown = state.measure.rows().len();
    let caption = if state.scene.region.enabled() {
        grana_i18n::measure::rows_roi(language, shown, total)
    } else {
        grana_i18n::measure::rows(language, shown, total)
    };
    let caption_style = TextStyle::new(
        ctx.theme().font(Level::Caption),
        ctx.theme().palette.on_surface_variant,
    );
    let caption_width = ctx.measure(&caption, caption_style).x;
    let measured = state.measure.table().is_some();
    let view_label_style = TextStyle::new(
        ctx.theme().label_font(),
        ctx.theme().palette.on_surface_variant,
    );
    let view_width = if measured { 136.0 } else { 0.0 };
    let view_label_width = if measured {
        ctx.measure(grana_i18n::measure::view(language), view_label_style)
            .x
            + 8.0
    } else {
        0.0
    };
    let chart = measured && mode != MeasureView::Table;
    let icon_count = if chart { 4usize } else { 3usize };
    let group_gap = ctx.theme().metrics.group_gap;
    let icons_width = icon_count as f32 * 30.0 + (icon_count.saturating_sub(1)) as f32 * group_gap;
    let right_width = view_label_width + view_width + caption_width + icons_width + 24.0;
    let right = Rect::from_min_size(
        Vec2::new((row.max.x - right_width).max(row.min.x), row.min.y),
        Vec2::new(right_width.min(row.width()), TOOLBAR_HEIGHT),
    );
    let mut right_items = Vec::new();
    if measured {
        let index = MeasureView::ALL
            .iter()
            .position(|view| *view == mode)
            .unwrap_or(0);
        right_items.push(field_label(ctx, grana_i18n::measure::view(language)));
        right_items.push(combo(
            ctx,
            "measure.view",
            136.0,
            index,
            &labels,
            move |index| match MeasureView::ALL.get(index) {
                Some(value) if *value != mode => Message::Command(Command::SetMeasureView(*value)),
                _ => Message::Noop,
            },
        ));
    }
    right_items.push(view::caption(ctx, &caption));
    let inner = ctx.theme().metrics.corner_xs;
    let mut icon_group = Vec::with_capacity(icon_count);
    icon_group.push(
        view::icon_button(
            ctx,
            "measure.filter",
            Icon::Filter,
            30.0,
            Role::SurfaceHigh,
            grana_i18n::measure::add_filter(language),
            Message::Command(Command::AddFilter),
        )
        .shaped(Corners::group_horizontal(
            0,
            icon_count,
            Corners::CAPSULE,
            inner,
            false,
        )),
    );
    icon_group.push(
        view::icon_button(
            ctx,
            "measure.columns",
            Icon::Columns,
            30.0,
            Role::SurfaceHigh,
            grana_i18n::measure::choose_columns(language),
            Message::Noop,
        )
        .shaped(Corners::group_horizontal(
            1,
            icon_count,
            Corners::CAPSULE,
            inner,
            false,
        )),
    );
    icon_group.push(
        view::icon_button(
            ctx,
            "measure.export",
            Icon::Export,
            30.0,
            Role::SurfaceHigh,
            grana_i18n::measure::export_csv(language),
            Message::Command(Command::ExportMeasure),
        )
        .shaped(Corners::group_horizontal(
            2,
            icon_count,
            Corners::CAPSULE,
            inner,
            false,
        )),
    );
    if chart {
        icon_group.push(
            view::icon_button(
                ctx,
                "measure.chart",
                Icon::Chart,
                30.0,
                Role::PrimaryContainer,
                grana_i18n::measure::export_plot(language),
                Message::Command(Command::SaveChart),
            )
            .shaped(Corners::group_horizontal(
                3,
                icon_count,
                Corners::CAPSULE,
                inner,
                false,
            )),
        );
    }
    right_items.push(
        grana_ui::row()
            .gap(group_gap)
            .align(Align::Center)
            .children(icon_group),
    );
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
    let language = ctx.language();
    let Some(table) = state.measure.table() else {
        return grana_ui::column();
    };
    let visible = state.measure.visible();
    let mut rows = vec![
        grana_ui::widget::menu_row(
            &theme,
            grana_i18n::measure::show_all_columns(language),
            false,
        )
        .named("measure.show.all")
        .press(Message::Command(Command::ShowAllColumns)),
        grana_ui::widget::separator().named("measure.columns.separator"),
    ];
    for (index, column) in table.columns().iter().enumerate() {
        let shown = visible.get(index).copied().unwrap_or(false);
        rows.push(
            grana_ui::widget::menu_row(&theme, &view::column_label(language, column), shown)
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
    let language = ctx.language();
    let view = state.measure.view();
    let axes = state.measure.axes();
    let options = table
        .columns()
        .iter()
        .enumerate()
        .map(|(index, column)| (index, view::column_label(language, column)))
        .collect::<Vec<_>>();
    let labels = options
        .iter()
        .map(|(_, label)| label.clone())
        .collect::<Vec<_>>();
    let lasso = state.measure.lasso().is_some();
    let gated = match view {
        MeasureView::Ripley => false,
        MeasureView::Trend => state
            .measure
            .filters()
            .iter()
            .any(|filter| filter.column() == axes[0]),
        MeasureView::Distribution => {
            lasso
                || state
                    .measure
                    .filters()
                    .iter()
                    .any(|filter| filter.column() == axes[0])
        }
        _ => {
            lasso
                || state.measure.filters().iter().any(|filter| {
                    filter.column() == axes[0]
                        || (view == MeasureView::Scatter && filter.column() == axes[1])
                })
        }
    };
    let selected_x = options
        .iter()
        .position(|(index, _)| *index == axes[0])
        .unwrap_or(0);
    let selected_y = options
        .iter()
        .position(|(index, _)| *index == axes[1])
        .unwrap_or(0);
    let options_x = options.clone();
    let mut items = Vec::new();
    match view {
        MeasureView::Ripley => {}
        MeasureView::Distribution | MeasureView::Trend => {
            items.push(field_label(ctx, grana_i18n::measure::value(language)));
            items.push(combo(
                ctx,
                "measure.plot.x",
                180.0,
                selected_x,
                &labels,
                move |index| match options_x.get(index) {
                    Some((column, _)) if *column != axes[0] => {
                        Message::Command(Command::SetPlotAxis {
                            axis: PlotAxis::X,
                            column: *column,
                        })
                    }
                    _ => Message::Noop,
                },
            ));
            if view == MeasureView::Distribution {
                let styles = DistributionStyle::ALL
                    .iter()
                    .copied()
                    .map(|style| distribution_style_label(language, style))
                    .collect::<Vec<_>>();
                items.push(field_label(ctx, grana_i18n::measure::style(language)));
                items.push(view::segmented(
                    ctx,
                    "measure.distribution",
                    &styles,
                    state.workspace.distribution_style.index(),
                    Message::SetDistributionStyle,
                ));
            }
        }
        MeasureView::Table | MeasureView::Histogram | MeasureView::Scatter => {
            items.push(field_label(ctx, grana_i18n::measure::x_axis(language)));
            items.push(combo(
                ctx,
                "measure.plot.x",
                180.0,
                selected_x,
                &labels,
                move |index| match options_x.get(index) {
                    Some((column, _)) if *column != axes[0] => {
                        Message::Command(Command::SetPlotAxis {
                            axis: PlotAxis::X,
                            column: *column,
                        })
                    }
                    _ => Message::Noop,
                },
            ));
            if view == MeasureView::Scatter {
                items.push(field_label(ctx, grana_i18n::measure::y_axis(language)));
                items.push(combo(
                    ctx,
                    "measure.plot.y",
                    180.0,
                    selected_y,
                    &labels,
                    move |index| match options.get(index) {
                        Some((column, _)) if *column != axes[1] => {
                            Message::Command(Command::SetPlotAxis {
                                axis: PlotAxis::Y,
                                column: *column,
                            })
                        }
                        _ => Message::Noop,
                    },
                ));
            }
        }
    }
    if gated {
        items.push(
            view::button(
                ctx,
                grana_i18n::measure::clear_gate(language),
                Role::SurfaceHigh,
            )
            .press(Message::ClearGates),
        );
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
    let language = ctx.language();
    let filters = state.measure.filters().to_vec();
    let filtered = !filters.is_empty();
    if filtered || state.measure.lasso().is_some() {
        let row = Rect::from_min_size(
            Vec2::new(rect.min.x, *top + 4.0),
            Vec2::new(rect.width(), 30.0),
        );
        let theme = *ctx.theme();
        let visibility = state.measure.visibility();
        let mut items = vec![
            grana_ui::widget::checkbox(
                &theme,
                grana_i18n::measure::hide_filtered_rows(language),
                visibility,
            )
            .key(Key::new("hide.filtered"))
            .on_toggle(|value| Message::Command(Command::SetMeasureVisibility(value))),
        ];
        if state.access.author() {
            items.push(
                view::button(
                    ctx,
                    grana_i18n::measure::extract_filtered(language),
                    Role::TertiaryContainer,
                )
                .press(Message::Command(Command::ExtractFiltered)),
            );
            items.push(
                view::button(
                    ctx,
                    grana_i18n::measure::delete_excluded(language),
                    Role::Error,
                )
                .press(Message::Command(Command::DeleteFiltered)),
            );
        }
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
        *top += 34.0;
    }
    let options = table
        .columns()
        .iter()
        .enumerate()
        .map(|(index, column)| (index, view::column_label(language, column)))
        .collect::<Vec<_>>();
    let labels = options
        .iter()
        .map(|(_, label)| label.clone())
        .collect::<Vec<_>>();
    let surface = ctx.theme().surface(Role::Card, State::Rest);
    let count = filters.len();
    for (index, filter) in filters.iter().enumerate() {
        let card = Rect::from_min_size(
            Vec2::new(rect.min.x, *top + 4.0),
            Vec2::new(rect.width(), 38.0),
        );
        let remove = Rect::from_center_size(
            Vec2::new(card.max.x - 22.0, card.center().y),
            Vec2::splat(24.0),
        );
        let controls = Rect::from_min_max(
            Vec2::new(card.min.x + 12.0, card.min.y + 4.0),
            Vec2::new(remove.min.x - 8.0, card.max.y - 4.0),
        );
        let theme = *ctx.theme();
        let mut surface = surface;
        surface.corners = Corners::group_vertical(
            index,
            count,
            theme.metrics.corner_lg,
            theme.metrics.corner_xs,
            false,
        );
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
                field_label(ctx, grana_i18n::measure::filter(language)),
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
                24.0,
                Role::Standard,
                grana_i18n::measure::remove_filter(language),
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
    let language = ctx.language();
    let Some(set) = state.measure.set() else {
        return vec![
            view::empty_state(
                ctx,
                Icon::Table,
                grana_i18n::measure::no_set_selected(language),
                grana_i18n::measure::no_set_selected_detail(language),
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
                    grana_i18n::measure::measuring(language),
                    grana_i18n::measure::measuring_detail(language),
                )
            } else {
                view::empty_state(
                    ctx,
                    Icon::Table,
                    grana_i18n::measure::no_detection(language),
                    grana_i18n::measure::no_detection_detail(language),
                )
            }
            .at(rect),
        ];
    };
    if table.rows() == 0 {
        let detail = match state.measure.group() {
            Group::Track => grana_i18n::measure::no_rows_track_detail(language),
            Group::Molecule => grana_i18n::measure::no_rows_molecule_detail(language),
            Group::Object | Group::Timepoint => grana_i18n::measure::no_rows_detail(language),
        };
        return vec![
            view::empty_state(
                ctx,
                Icon::Table,
                grana_i18n::measure::no_rows(language),
                detail,
            )
            .at(rect),
        ];
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
                grana_i18n::measure::no_visible_columns(language),
                grana_i18n::measure::no_visible_columns_detail(language),
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
        grana_ui::canvas(Length::Fill, rect.height())
            .paint(move |painter, info| {
                painter.surface(info.rect, surface);
            })
            .at(rect),
    ];
    let band = Rect::from_min_size(rect.min, Vec2::new(total, HEADER_HEIGHT));
    elements.push(
        grana_ui::canvas(Length::Fill, HEADER_HEIGHT)
            .paint(move |painter, info| {
                painter.rect(
                    info.rect,
                    Corners::top(theme.metrics.corner_lg),
                    palette.surface_container_high,
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
        let label = view::column_label(language, column);
        let style = TextStyle::new(
            theme.font(Level::Caption).semibold(),
            if sorted.is_some() {
                palette.primary
            } else {
                palette.on_surface_variant
            },
        )
        .aligned(Align::End)
        .truncating();
        let icon_color = palette.primary;
        let arrow = match sorted {
            Some(true) => Some(Icon::ChevronDown),
            Some(false) => Some(Icon::ChevronUp),
            None => None,
        };
        elements.push(
            grana_ui::canvas(Length::Fixed(width), HEADER_HEIGHT)
                .paint(move |painter, info| {
                    if info.hover > 0.01 {
                        painter.rect(
                            info.rect.shrink(2.0),
                            Corners::all(theme.metrics.corner_xs),
                            palette.on_surface.with_alpha(0.08 * info.hover),
                        );
                    }
                    painter.text_in(
                        Rect::from_min_max(
                            info.rect.min + Vec2::new(8.0, 0.0),
                            Vec2::new(
                                info.rect.max.x - arrow.map_or(8.0, |_| 18.0),
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
                            Vec2::new(info.rect.max.x - 10.0, info.rect.center().y),
                            9.5,
                            icon_color,
                            1.5,
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
            grana_ui::canvas(Length::Fixed(4.0), HEADER_HEIGHT)
                .paint(|_, _| {})
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
    let body = Rect::from_min_max(
        Vec2::new(rect.min.x, band.max.y),
        Vec2::new(rect.min.x + total.max(1.0), rect.max.y),
    );
    let rows = state.measure.rows();
    let time = state.scene.time();
    let selection = state.scene.selection();
    let font = theme.font(Level::Monospace);
    let on_selected = palette.on_secondary_container;
    let on_row = palette.on_surface;
    let document = state.measure.document_hash();
    let digest = widths.iter().fold(0u64, |hash, width| {
        hash.rotate_left(7) ^ u64::from(width.to_bits())
    });
    let rows_key = Key::new("measure.rows");
    elements.push(
        ctx.scroll(
            rows_key,
            Axis::Column,
            Length::Fill,
            ROW_HEIGHT,
            shown,
            move |window| {
                let mut children = Vec::new();
                for row in window.span() {
                    let Some(index) = state_index(rows, row) else {
                        continue;
                    };
                    let observation = table.observation(index);
                    let selected = matches_selection(selection, set, observation, time);
                    let cells = columns
                        .iter()
                        .zip(widths.iter())
                        .map(|((column_index, column), width)| {
                            (column.format(table.value(index, *column_index)), *width)
                        })
                        .collect::<Vec<_>>();
                    children.push(
                        grana_ui::canvas(Length::Fill, ROW_HEIGHT)
                            .view(
                                (document, row, selected, digest, cells.len()),
                                move |_, painter, info| {
                                    let rect = info.rect;
                                    let body = rect.shrink2(Vec2::new(4.0, 1.0));
                                    if selected {
                                        painter.rect(
                                            body,
                                            Corners::all(8.0),
                                            palette.secondary_container,
                                        );
                                    } else if row % 2 == 1 {
                                        painter.rect(
                                            body,
                                            Corners::all(4.0),
                                            Color::white_alpha(0.02),
                                        );
                                    }
                                    if info.hover > 0.01 {
                                        painter.rect(
                                            body,
                                            Corners::all(8.0),
                                            palette.on_surface.with_alpha(0.07 * info.hover),
                                        );
                                    }
                                    let mut left = rect.min.x;
                                    for (text, width) in cells.iter() {
                                        let cell = Rect::from_min_size(
                                            Vec2::new(left, rect.min.y),
                                            Vec2::new(*width, ROW_HEIGHT),
                                        );
                                        painter.text_in(
                                            cell.shrink2(Vec2::new(8.0, 0.0)),
                                            text,
                                            TextStyle::new(
                                                font,
                                                if selected { on_selected } else { on_row },
                                            )
                                            .aligned(Align::End),
                                            Align::Center,
                                        );
                                        left += *width;
                                    }
                                },
                            )
                            .key(Key::index("measure.row", row))
                            .on_pointer(move |pointer| {
                                (pointer.phase == grana_ui::Phase::Up
                                    && pointer.rect.contains(pointer.position))
                                .then_some(Message::Locate(row))
                            }),
                    );
                }
                children
            },
        )
        .at(body),
    );
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
        TextStyle::new(theme.label_font(), theme.palette.on_surface_variant),
    )
}

fn view_label(language: Language, view: MeasureView) -> &'static str {
    match view {
        MeasureView::Table => grana_i18n::measure::table(language),
        MeasureView::Histogram => grana_i18n::measure::histogram(language),
        MeasureView::Scatter => grana_i18n::measure::scatter(language),
        MeasureView::Distribution => grana_i18n::measure::distribution(language),
        MeasureView::Trend => grana_i18n::measure::trend(language),
        MeasureView::Ripley => grana_i18n::measure::ripley(language),
    }
}

fn distribution_style_label(language: Language, style: DistributionStyle) -> &'static str {
    match style {
        DistributionStyle::Box => grana_i18n::measure::box_plot(language),
        DistributionStyle::Violin => grana_i18n::measure::violin_plot(language),
    }
}

fn aggregate_label(language: Language, aggregate: Aggregate) -> String {
    if aggregate == Aggregate::MEAN {
        grana_i18n::measure::aggregate_mean(language).to_string()
    } else if aggregate == Aggregate::SUM {
        grana_i18n::measure::aggregate_sum(language).to_string()
    } else if aggregate == Aggregate::MINIMUM {
        grana_i18n::measure::aggregate_minimum(language).to_string()
    } else if aggregate == Aggregate::MAXIMUM {
        grana_i18n::measure::aggregate_maximum(language).to_string()
    } else if aggregate == Aggregate::DEVIATION {
        grana_i18n::measure::aggregate_deviation(language).to_string()
    } else if Aggregate::quantile(50) == Some(aggregate) {
        grana_i18n::measure::aggregate_median(language).to_string()
    } else {
        aggregate.label()
    }
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
    let style = TextStyle::new(theme.font(Level::Label), palette.on_surface).truncating();
    let rest = theme.surface(Role::Field, State::Rest);
    let hover = theme.surface(Role::Field, State::Hover);
    let anchor = grana_ui::canvas(Length::Fixed(width), 30.0)
        .paint(move |painter, info| {
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
                palette.on_surface_variant,
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
    let language = state.workspace.language;
    let initial = columns
        .iter()
        .map(|(_, column)| {
            let header = view::column_label(language, column).chars().count() as f32 * 5.9 + 22.0;
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
