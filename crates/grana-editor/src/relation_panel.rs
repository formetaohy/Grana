use std::collections::HashMap;

use glam::Vec2;
use grana_display::Palette;
use grana_i18n::Language;
use grana_object::{RelationFrame, RelationPair};
use grana_scene::{Item, Job, ObjectFrame, ObjectSet, ObjectSetId, Scene, Selection};
use grana_session::Command;
use grana_ui::{
    Align, BuildContext, Color, Corners, Element, Icon, Key, Length, Level, Rect, Role, TextStyle,
};

use crate::message::Message;
use crate::state::{RelationMetric, View};
use crate::view;

const CELL: f32 = 20.0;
const MINIMUM_CELL: f32 = 12.0;
const GUTTER: f32 = 38.0;
const HEADER: f32 = 18.0;
const ROW_HEIGHT: f32 = 24.0;
const ROW_LIMIT: usize = 48;
const COLUMN_LIMIT: usize = 72;
const PAIR_LIMIT: usize = 40;
const STRIP_COUNT: usize = 64;

#[derive(Clone, Copy)]
struct Choice {
    source: ObjectSetId,
    target: ObjectSetId,
    shift: i32,
    computed: usize,
    total: usize,
}

impl Choice {
    fn key(self) -> (ObjectSetId, ObjectSetId, i32) {
        (self.source, self.target, self.shift)
    }

    fn caption(self, language: Language, scene: &Scene) -> String {
        let source = set_name(language, scene, self.source);
        let target = set_name(language, scene, self.target);
        if self.shift == 0 {
            format!("{source} → {target}")
        } else {
            format!("{source} → {target} · t{:+}", self.shift)
        }
    }
}

#[derive(Clone, Copy)]
struct PairRow {
    source: usize,
    target: usize,
    value: f32,
    strength: f32,
}

struct Matrix {
    pairs: Vec<PairRow>,
    rows: Vec<usize>,
    columns: Vec<usize>,
    cells: HashMap<(usize, usize), f32>,
    minimum: f32,
    maximum: f32,
    source: ObjectSetId,
    target: ObjectSetId,
    cell: f32,
    filtered: bool,
}

enum Body {
    Pending,
    Ready(Box<Matrix>),
}

pub fn view<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let language = ctx.language();
    let choices = choices(state.scene);
    if choices.is_empty() {
        return view::empty_state(
            ctx,
            Icon::Grid,
            grana_i18n::relation::no_relations_title(language),
            grana_i18n::relation::no_relations_detail(language),
        );
    }
    let selected = state
        .workspace
        .relation
        .and_then(|key| choices.iter().position(|choice| choice.key() == key))
        .unwrap_or(0);
    let choice = choices[selected];
    let body = match state.scene.objects.get(choice.source) {
        Some(source) => body(ctx, state, source, choice),
        None => Body::Pending,
    };
    let mut children = vec![header(ctx, state, &choices, selected)];
    match body {
        Body::Pending => children.push(pending(ctx, state, choice)),
        Body::Ready(matrix) => {
            children.push(matrix_card(ctx, state, &matrix));
            children.push(pairs_card(ctx, state, &matrix));
        }
    }
    grana_ui::column()
        .gap(12.0)
        .width(Length::Fill)
        .children(children)
}

fn choices(scene: &Scene) -> Vec<Choice> {
    let mut choices = Vec::new();
    for set in scene.objects.iter() {
        for relation in set.relations() {
            choices.push(Choice {
                source: set.id(),
                target: relation.target(),
                shift: relation.shift(),
                computed: relation.computed(),
                total: relation.frame_count(),
            });
        }
    }
    choices
}

fn set_name(language: Language, scene: &Scene, id: ObjectSetId) -> String {
    scene.objects.get(id).map_or_else(
        || grana_i18n::relation::object_set(language).to_string(),
        |set| set.name().to_string(),
    )
}

fn header<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    choices: &[Choice],
    selected: usize,
) -> Element<'a, Message> {
    let language = ctx.language();
    let mut children = Vec::new();
    let mut buttons = Vec::with_capacity(choices.len());
    for (index, choice) in choices.iter().copied().enumerate() {
        let role = if index == selected {
            Role::Tonal
        } else {
            Role::SurfaceHigh
        };
        let running = view::progress(
            state.jobs,
            Job::Relation {
                source: choice.source,
                target: choice.target,
                shift: choice.shift,
            },
        );
        let label = match running {
            Some(progress) => format!(
                "{} · {}/{}",
                choice.caption(language, state.scene),
                progress.done,
                progress.total
            ),
            None => choice.caption(language, state.scene),
        };
        buttons.push(
            view::button(ctx, &label, role)
                .named(&format!("relation.choice.{index}"))
                .press(Message::SetRelation {
                    source: choice.source,
                    target: choice.target,
                    shift: choice.shift,
                }),
        );
    }
    children.push(grana_ui::row().gap(6.0).wrap().children(buttons));
    let metric = state.workspace.relation_metric;
    let labels = RelationMetric::ALL.map(|metric| metric.name(language));
    let segments = view::segmented(
        ctx,
        "relation.metric",
        &labels,
        metric.index(),
        Message::SetRelationMetric,
    );
    children.push(view::form_row(
        ctx,
        grana_i18n::relation::metric(language),
        segments,
    ));
    let filter = state.workspace.relation_filter;
    let slider = view::slider_event(
        ctx,
        "relation.filter",
        None,
        100.0 * filter,
        0.0..=100.0,
        &format!("≥ {:.0}%", 100.0 * filter),
        |value| Message::SetRelationFilter(value / 100.0),
    );
    children.push(view::form_row(
        ctx,
        grana_i18n::relation::filter(language),
        slider,
    ));
    let choice = choices[selected];
    let export = view::button(
        ctx,
        grana_i18n::relation::export_pairs(language),
        Role::SurfaceHigh,
    )
    .press(Message::Command(Command::ExportRelation {
        source: choice.source,
        target: choice.target,
        shift: choice.shift,
    }));
    children.push(grana_ui::row().gap(6.0).children([export]));
    children.push(view::caption(ctx, export_hint(choice, language)));
    view::card(ctx, 9.0, children)
}

fn export_hint(choice: Choice, language: Language) -> &'static str {
    match choice.computed {
        0 => grana_i18n::relation::no_frame_measured(language),
        computed if computed == choice.total => grana_i18n::relation::exports_every_pair(language),
        _ => grana_i18n::relation::exports_measured_pairs(language),
    }
}

fn body(
    ctx: &mut BuildContext<'_, Message>,
    state: &View<'_>,
    source: &ObjectSet,
    choice: Choice,
) -> Body {
    let Some(relation) = source.relation(choice.target, choice.shift) else {
        return Body::Pending;
    };
    let time = state.scene.time();
    let Some(frame) = relation.frame(time) else {
        return Body::Pending;
    };
    let target_objects = target_time(time, choice.shift)
        .and_then(|shifted| {
            state
                .scene
                .objects
                .get(choice.target)
                .and_then(|set| set.frame(shifted))
        })
        .map_or(0, ObjectFrame::object_count);
    Body::Ready(Box::new(matrix(ctx, state, frame, choice, target_objects)))
}

fn target_time(time: u32, shift: i32) -> Option<u32> {
    let shifted = i64::from(time) + i64::from(shift);
    (0..=i64::from(u32::MAX))
        .contains(&shifted)
        .then_some(shifted as u32)
}

fn matrix(
    ctx: &BuildContext<'_, Message>,
    state: &View<'_>,
    frame: &RelationFrame,
    choice: Choice,
    target_objects: usize,
) -> Matrix {
    let metric = state.workspace.relation_metric;
    let objects = frame.objects();
    let mut values = frame
        .pairs()
        .iter()
        .filter(|pair| {
            (pair.source() as usize) < objects && (pair.target() as usize) < target_objects
        })
        .map(|pair| (*pair, metric.value(*pair)))
        .collect::<Vec<(RelationPair, f32)>>();
    let (minimum, maximum) = bounds(&values);
    let near = metric.near();
    let strength = |value: f32| position(value, minimum, maximum, near);
    let filter = state.workspace.relation_filter;
    let filtered = filter > 0.0;
    values.retain(|(_, value)| strength(*value) + 1.0e-6 >= filter);
    let mut pairs = values
        .into_iter()
        .map(|(pair, value)| PairRow {
            source: pair.source() as usize,
            target: pair.target() as usize,
            value,
            strength: strength(value),
        })
        .collect::<Vec<_>>();
    pairs.sort_by(|left, right| {
        right
            .strength
            .total_cmp(&left.strength)
            .then(left.value.total_cmp(&right.value))
            .then((left.source, left.target).cmp(&(right.source, right.target)))
    });
    let usable = (ctx.size().x - 96.0 - GUTTER).max(MINIMUM_CELL * 4.0);
    let fit = ((usable / MINIMUM_CELL).floor() as usize).clamp(1, COLUMN_LIMIT);
    let mut rows = ranked(&pairs, |pair| pair.source);
    let mut columns = ranked(&pairs, |pair| pair.target);
    rows.truncate(ROW_LIMIT);
    columns.truncate(fit);
    let mut cells = HashMap::with_capacity(pairs.len());
    for pair in &pairs {
        if let (Some(row), Some(column)) = (
            rows.iter().position(|index| *index == pair.source),
            columns.iter().position(|index| *index == pair.target),
        ) {
            cells.insert((row, column), pair.strength);
        }
    }
    let cell = (usable / columns.len().max(1) as f32).clamp(MINIMUM_CELL, CELL);
    Matrix {
        pairs,
        rows,
        columns,
        cells,
        minimum,
        maximum,
        source: choice.source,
        target: choice.target,
        cell,
        filtered,
    }
}

fn bounds(values: &[(RelationPair, f32)]) -> (f32, f32) {
    let mut minimum = f32::INFINITY;
    let mut maximum = f32::NEG_INFINITY;
    for (_, value) in values {
        minimum = minimum.min(*value);
        maximum = maximum.max(*value);
    }
    if minimum.is_finite() && maximum.is_finite() {
        (minimum, maximum)
    } else {
        (0.0, 0.0)
    }
}

fn position(value: f32, minimum: f32, maximum: f32, near: bool) -> f32 {
    if maximum <= minimum {
        return 0.5;
    }
    let offset = ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0);
    if near { 1.0 - offset } else { offset }
}

fn heat(strength: f32) -> Color {
    let strength = strength.clamp(0.0, 1.0);
    Color::from_rgb_f32(Palette::Inferno.sample(0.30 + 0.70 * strength))
        .with_alpha(0.35 + 0.65 * strength)
}

fn ranked(pairs: &[PairRow], select: impl Fn(&PairRow) -> usize) -> Vec<usize> {
    let mut indices = pairs.iter().map(select).collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    indices
}

fn pending<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &View<'_>,
    choice: Choice,
) -> Element<'a, Message> {
    let running = view::progress(
        state.jobs,
        Job::Relation {
            source: choice.source,
            target: choice.target,
            shift: choice.shift,
        },
    );
    let language = ctx.language();
    let detail = match running {
        Some(progress) => grana_i18n::relation::measuring_frame(
            language,
            progress.done.min(progress.total).max(1),
            progress.total,
        ),
        None => grana_i18n::relation::frame_not_measured(
            language,
            choice.computed,
            choice.total,
            state.scene.time() + 1,
        ),
    };
    let header = view::section_header(
        ctx,
        grana_i18n::relation::pair_matrix(language),
        Some(&choice.caption(language, state.scene)),
    );
    let hint = view::weak(ctx, &detail);
    view::card(ctx, 6.0, vec![header, hint])
}

fn matrix_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &View<'a>,
    matrix: &Matrix,
) -> Element<'a, Message> {
    let language = ctx.language();
    let detail =
        grana_i18n::relation::matrix_scope(language, matrix.rows.len(), matrix.columns.len());
    let mut children = vec![view::card_title(
        ctx,
        Icon::Grid,
        grana_i18n::relation::pair_matrix(language),
        Some(&detail),
    )];
    if matrix.pairs.is_empty() {
        children.push(view::weak(
            ctx,
            if matrix.filtered {
                grana_i18n::relation::no_pair_filtered(language)
            } else {
                grana_i18n::relation::no_pairs_frame(language)
            },
        ));
        return view::card(ctx, 8.0, children);
    }
    children.push(header_row(ctx, matrix));
    children.push(body_row(ctx, matrix));
    children.push(legend(ctx, state.workspace.relation_metric, matrix));
    view::card(ctx, 8.0, children)
}

fn header_row<'a>(ctx: &mut BuildContext<'_, Message>, matrix: &Matrix) -> Element<'a, Message> {
    let cell = matrix.cell;
    let theme = *ctx.theme();
    let palette = theme.palette;
    let style = TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant)
        .aligned(Align::Center);
    let step = ((24.0 / cell).ceil() as usize).max(1);
    let mut labels = Vec::with_capacity(matrix.columns.len());
    for (index, column) in matrix.columns.iter().copied().enumerate() {
        let text = if index % step == 0 {
            (column + 1).to_string()
        } else {
            String::new()
        };
        let selection = Selection {
            set: matrix.target,
            item: Item::Object(column),
        };
        labels.push(
            grana_ui::canvas(Length::Fixed(cell), HEADER)
                .view(text.clone(), move |_, painter, info| {
                    painter.text_in(info.rect, &text, style, Align::Center);
                })
                .key(Key::index("relation.column", index))
                .press(Message::Command(Command::Select(Some(selection)))),
        );
    }
    let gutter_style = TextStyle::new(theme.font(Level::Caption), palette.outline);
    let source_target = grana_i18n::relation::source_target(ctx.language());
    let gutter = grana_ui::canvas(Length::Fixed(GUTTER), HEADER).view(
        source_target,
        move |_, painter, info| {
            painter.text_in(info.rect, source_target, gutter_style, Align::Center);
        },
    );
    let mut row = vec![gutter];
    row.extend(labels);
    grana_ui::row().height(Length::Fixed(HEADER)).children(row)
}

fn body_row<'a>(ctx: &mut BuildContext<'_, Message>, matrix: &Matrix) -> Element<'a, Message> {
    let cell = matrix.cell;
    let theme = *ctx.theme();
    let palette = theme.palette;
    let label_style =
        TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant).aligned(Align::End);
    let mut gutter = Vec::with_capacity(matrix.rows.len());
    for (index, row) in matrix.rows.iter().copied().enumerate() {
        let text = (row + 1).to_string();
        let selection = Selection {
            set: matrix.source,
            item: Item::Object(row),
        };
        gutter.push(
            grana_ui::canvas(Length::Fixed(GUTTER), cell)
                .view(text.clone(), move |_, painter, info| {
                    painter.text_in(
                        info.rect.shrink2(Vec2::new(6.0, 0.0)),
                        &text,
                        label_style,
                        Align::Center,
                    );
                })
                .key(Key::index("relation.row", index))
                .press(Message::Command(Command::Select(Some(selection)))),
        );
    }
    let columns = matrix.columns.len();
    let rows = matrix.rows.len();
    let cells = matrix.cells.clone();
    let width = (columns as f32 * cell).max(1.0);
    let height = (rows as f32 * cell).max(1.0);
    let grid = palette.surface_container_highest;
    let digest = cells.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, cell| {
        let ((source, target), strength) = cell;
        hash.rotate_left(7)
            ^ ((*source as u64) << 32)
            ^ (*target as u64)
            ^ u64::from(strength.to_bits())
    });
    let canvas = grana_ui::canvas(Length::Fixed(width), height).view(
        (digest, rows, columns),
        move |_, painter, info| {
            for row in 0..rows {
                for column in 0..columns {
                    let rect = Rect::from_min_size(
                        Vec2::new(
                            info.rect.min.x + column as f32 * cell,
                            info.rect.min.y + row as f32 * cell,
                        ),
                        Vec2::splat(cell),
                    )
                    .shrink(0.5);
                    let color = match cells.get(&(row, column)) {
                        Some(strength) => heat(*strength),
                        None => grid,
                    };
                    painter.rect(rect, Corners::all(3.0), color);
                }
            }
        },
    );
    let row = vec![
        grana_ui::column()
            .width(Length::Fixed(GUTTER))
            .children(gutter),
        canvas,
    ];
    grana_ui::row().height(Length::Fixed(height)).children(row)
}

fn legend<'a>(
    ctx: &mut BuildContext<'_, Message>,
    metric: RelationMetric,
    matrix: &Matrix,
) -> Element<'a, Message> {
    let language = ctx.language();
    let theme = *ctx.theme();
    let palette = theme.palette;
    let caption = TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant);
    let low = metric.format(matrix.minimum);
    let high = metric.format(matrix.maximum);
    let label = if matrix.maximum <= matrix.minimum {
        grana_i18n::relation::uniform_strength(language, &low)
    } else if metric.near() {
        grana_i18n::relation::near_far(language, &low, &high)
    } else {
        grana_i18n::relation::low_high(language, &low, &high)
    };
    grana_ui::canvas(Length::Fill, 16.0).view(caption, move |_, painter, info| {
        let rect = Rect::from_min_size(info.rect.min, Vec2::new(160.0, 8.0));
        let width = rect.width() / STRIP_COUNT as f32;
        for index in 0..STRIP_COUNT {
            let position = index as f32 / (STRIP_COUNT - 1) as f32;
            painter.rect(
                Rect::from_min_size(
                    Vec2::new(rect.min.x + index as f32 * width, rect.min.y),
                    Vec2::new(width + 0.5, rect.height()),
                ),
                Corners::ZERO,
                heat(position),
            );
        }
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(rect.max.x + 10.0, info.rect.min.y),
                Vec2::new(info.rect.width() - rect.width() - 10.0, info.rect.height()),
            ),
            &label,
            caption,
            Align::Center,
        );
    })
}

fn pairs_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &View<'_>,
    matrix: &Matrix,
) -> Element<'a, Message> {
    let language = ctx.language();
    let metric = state.workspace.relation_metric;
    let theme = *ctx.theme();
    let palette = theme.palette;
    let shown = matrix.pairs.len().min(PAIR_LIMIT);
    let detail = if matrix.pairs.len() > shown {
        grana_i18n::relation::strongest(language, shown, matrix.pairs.len())
    } else {
        grana_i18n::relation::pair_count(language, matrix.pairs.len())
    };
    let mut children = vec![view::card_title(
        ctx,
        Icon::Target,
        grana_i18n::relation::pairs(language),
        Some(&detail),
    )];
    if matrix.pairs.is_empty() {
        children.push(view::weak(
            ctx,
            grana_i18n::relation::no_pairs_list(language),
        ));
        return view::card(ctx, 6.0, children);
    }
    let source = set_name(language, state.scene, matrix.source);
    let target = set_name(language, state.scene, matrix.target);
    children.push(view::caption(ctx, &format!("{source} → {target}")));
    let mut rows = Vec::with_capacity(shown);
    for (index, pair) in matrix.pairs.iter().take(shown).enumerate() {
        let text = format!("#{} → #{}", pair.source + 1, pair.target + 1);
        let value = metric.format(pair.value);
        let strength = pair.strength;
        let body = TextStyle::new(theme.font(Level::Label), palette.on_surface_variant);
        let number =
            TextStyle::new(theme.font(Level::Monospace), palette.on_surface).aligned(Align::End);
        let message = Message::SelectPair {
            source: matrix.source,
            target: matrix.target,
            objects: (pair.source, pair.target),
        };
        rows.push(
            grana_ui::canvas(Length::Fill, ROW_HEIGHT)
                .view(
                    (text.clone(), value.clone(), strength.to_bits()),
                    move |_, painter, info| {
                        if info.hover > 0.01 {
                            painter.rect(
                                info.rect,
                                Corners::all(theme.metrics.corner_sm),
                                palette.on_surface.with_alpha(0.08 * info.hover),
                            );
                        }
                        let swatch = Rect::from_min_size(
                            Vec2::new(info.rect.min.x + 4.0, info.rect.center().y - 4.5),
                            Vec2::splat(9.0),
                        );
                        painter.rect(swatch, Corners::capsule(), heat(strength));
                        painter.text_in(
                            Rect::from_min_size(
                                Vec2::new(info.rect.min.x + 20.0, info.rect.min.y),
                                Vec2::new(150.0, info.rect.height()),
                            ),
                            &text,
                            body,
                            Align::Center,
                        );
                        painter.text_in(
                            Rect::from_min_size(
                                Vec2::new(info.rect.min.x + 186.0, info.rect.min.y),
                                Vec2::new(120.0, info.rect.height()),
                            ),
                            &value,
                            number,
                            Align::Center,
                        );
                    },
                )
                .key(Key::index("relation.pair", index))
                .on_pointer(move |pointer| {
                    (pointer.phase == grana_ui::Phase::Up).then_some(message.clone())
                }),
        );
    }
    children.push(grana_ui::column().gap(2.0).children(rows));
    view::card(ctx, 6.0, children)
}
