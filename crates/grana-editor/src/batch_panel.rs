use glam::Vec2;
use grana_chart::{Decor, Style};
use grana_session::Command;
use grana_ui::{
    Align, Axis, BuildContext, Color, Corners, Cursor, Edges, Element, Icon, Key, Length, Level,
    Phase, Rect, Role, State, TextStyle, icon,
};

use crate::chart::{ComparisonRequest, comparison_chart, group_names, set_names};
use crate::message::Message;
use crate::state::{ComparisonMetric, View};
use crate::view;

pub fn view<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let col_a = grana_ui::column()
        .width(Length::Fixed(400.0))
        .gap(8.0)
        .children([protocol(ctx, state), datasets(ctx, state)]);
    let col_b = grana_ui::column()
        .width(Length::Fixed(328.0))
        .gap(8.0)
        .children([output(ctx, state)]);
    let col_c = grana_ui::column()
        .width(Length::Fill)
        .gap(8.0)
        .children([comparison(ctx, state)]);
    grana_ui::row()
        .gap(12.0)
        .align(Align::Start)
        .children([col_a, col_b, col_c])
}

fn protocol<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let running = state.batch_running();
    let has_protocol = state.batch_protocol().is_some();
    let mut actions = Vec::new();
    let theme = *ctx.theme();
    let language = ctx.language();
    if !running {
        actions.push(
            grana_ui::widget::action_button(
                &theme,
                Icon::Camera,
                grana_i18n::batch::capture(language),
                Role::Control,
            )
            .named("batch.capture")
            .press(Message::Command(Command::CaptureProtocol)),
        );
        actions.push(
            view::button(ctx, grana_i18n::batch::load(language), Role::Quiet)
                .press(Message::Command(Command::LoadProtocol)),
        );
        actions.push(
            view::button(ctx, grana_i18n::common::save(language), Role::Quiet)
                .press(Message::Command(Command::SaveProtocol)),
        );
        actions.push(
            view::button(ctx, grana_i18n::batch::provenance(language), Role::Quiet)
                .press(Message::Command(Command::ExportProvenance)),
        );
        if has_protocol {
            actions.push(
                view::button(ctx, grana_i18n::common::apply(language), Role::Quiet)
                    .press(Message::Command(Command::ApplyProtocol)),
            );
            if let Some(text) = state.batch_protocol() {
                actions.push(
                    view::button(ctx, grana_i18n::batch::copy(language), Role::Quiet)
                        .press(Message::Copy(text.to_string())),
                );
            }
        }
    }
    let row = grana_ui::row()
        .gap(6.0)
        .align(Align::Center)
        .children(actions);
    let mut children = vec![
        view::card_title(
            ctx,
            Icon::Sliders,
            grana_i18n::batch::protocol(language),
            if has_protocol {
                Some(grana_i18n::batch::ready(language))
            } else {
                None
            },
        ),
        row,
    ];
    match state.batch_protocol() {
        Some(text) => {
            let surface = ctx.theme().surface(Role::Field, State::Rest);
            let style = TextStyle::new(
                ctx.theme().font(Level::Monospace),
                ctx.theme().palette.text_secondary,
            )
            .wrapping();
            let body = grana_ui::text(text, style);
            children.push(
                grana_ui::column()
                    .padding(Edges::symmetric(10.0, 6.0))
                    .gap(4.0)
                    .background(surface)
                    .children([body]),
            );
        }
        None => children.push(view::weak(ctx, grana_i18n::batch::capture_hint(language))),
    }
    view::card(ctx, 6.0, children)
}

fn datasets<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let running = state.batch_running();
    let language = ctx.language();
    let count = state.batch_datasets().len();
    let mut children = vec![view::card_title(
        ctx,
        Icon::Folder,
        grana_i18n::batch::datasets(language),
        Some(&grana_i18n::batch::item_count(language, count)),
    )];
    if state.batch_datasets().is_empty() {
        children.push(view::weak(ctx, grana_i18n::batch::datasets_hint(language)));
    } else {
        let theme = *ctx.theme();
        let palette = theme.palette;
        let callout = theme.font(Level::Callout);
        let micro = theme.font(Level::Micro);
        let mut rows = Vec::new();
        let reviewable = !running || state.batch_paused();
        for (index, dataset) in state.batch_datasets().iter().enumerate() {
            let tint = state_tint(&dataset.state, &theme);
            let name = dataset.name.clone();
            let label = state_label(language, &dataset.state).to_string();
            let detail = dataset.detail.clone();
            let mut row = grana_ui::canvas(Length::Fill, 30.0, move |painter, info| {
                if info.hover > 0.01 {
                    painter.rect(
                        info.rect,
                        Corners::all(8.0),
                        Color::white_alpha(0.04 * info.hover),
                    );
                }
                icon::paint(
                    Icon::Folder,
                    painter,
                    Vec2::new(info.rect.min.x + 10.0, info.rect.center().y),
                    14.0,
                    palette.text_tertiary,
                    1.5,
                );
                painter.text_in(
                    Rect::from_min_max(
                        Vec2::new(info.rect.min.x + 22.0, info.rect.min.y),
                        Vec2::new(info.rect.max.x - 100.0, info.rect.max.y),
                    ),
                    &name,
                    TextStyle::new(callout, palette.text_primary).truncating(),
                    Align::Center,
                );
                let badge = Rect::from_min_size(
                    Vec2::new(info.rect.max.x - 92.0, info.rect.center().y - 9.0),
                    Vec2::new(92.0, 18.0),
                );
                painter.rect(badge, Corners::capsule(), tint.with_alpha(0.16));
                painter.text_in(
                    badge,
                    &label,
                    TextStyle::new(micro, tint).aligned(Align::Center),
                    Align::Center,
                );
            })
            .tooltip(&detail);
            if reviewable {
                row = row
                    .press(Message::Command(Command::ReviewBatchDataset(index)))
                    .cursor(Cursor::Pointing);
            }
            rows.push(row);
        }
        children.push(
            Element::new(grana_ui::Layout::column())
                .scroll(Axis::Column, Length::Fixed(96.0), None)
                .clipped()
                .children(rows),
        );
    }
    let mut actions = Vec::new();
    if !running {
        let theme = *ctx.theme();
        actions.push(
            grana_ui::widget::action_button(
                &theme,
                Icon::Plus,
                grana_i18n::batch::add_folder(language),
                Role::Control,
            )
            .named("batch.add")
            .press(Message::Command(Command::AddBatchDatasets)),
        );
        if !state.batch_datasets().is_empty() {
            actions.push(
                view::button(ctx, grana_i18n::common::clear(language), Role::Quiet)
                    .press(Message::Command(Command::ClearBatchDatasets)),
            );
        }
    }
    children.push(
        grana_ui::row()
            .gap(8.0)
            .align(Align::Center)
            .children(actions),
    );
    if let Some(index) = state.batch_review() {
        let name = state.batch_datasets().get(index).map_or_else(
            || grana_i18n::batch::dataset_fallback(language).to_string(),
            |dataset| dataset.name.clone(),
        );
        children.push(view::divider(ctx));
        children.push(view::caption(
            ctx,
            &grana_i18n::batch::reviewing(language, &name),
        ));
        let ready = state.batch_review_ready() && !running;
        let corrected = state
            .batch_datasets()
            .get(index)
            .is_some_and(|row| row.state.as_str() == "corrected");
        let theme = *ctx.theme();
        let record = grana_ui::widget::button_enabled(
            &theme,
            grana_i18n::batch::record_correction(language),
            Role::Prominent,
            Length::Fill,
            ready,
        )
        .named("batch.correct");
        let record = if ready {
            record.press(Message::Command(Command::RecordBatchCorrection))
        } else {
            record
        };
        let mut buttons = vec![record];
        if corrected && !running {
            buttons.push(
                view::button(
                    ctx,
                    grana_i18n::batch::clear_correction(language),
                    Role::Quiet,
                )
                .press(Message::Command(Command::ClearBatchCorrection)),
            );
        }
        children.push(
            grana_ui::row()
                .gap(8.0)
                .align(Align::Center)
                .children(buttons),
        );
        children.push(view::weak(
            ctx,
            grana_i18n::batch::correction_hint(language),
        ));
    }
    view::card(ctx, 6.0, children)
}

fn output<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let language = ctx.language();
    let mut children = vec![view::card_title(
        ctx,
        Icon::Export,
        grana_i18n::batch::output_execution(language),
        None,
    )];
    if let Some(path) = state.batch_output() {
        let theme = *ctx.theme();
        let look = theme.surface(Role::Field, State::Rest);
        let style =
            TextStyle::new(theme.font(Level::Monospace), theme.palette.text_secondary).truncating();
        let path = path.to_string();
        children.push(grana_ui::canvas(
            Length::Fill,
            28.0,
            move |painter, info| {
                painter.outlined(info.rect, look.corners, look.fill, look.border, look.shadow);
                painter.text_in(
                    info.rect.shrink2(Vec2::new(10.0, 0.0)),
                    &path,
                    style,
                    Align::Center,
                );
            },
        ));
    } else {
        children.push(view::weak(ctx, grana_i18n::batch::output_hint(language)));
    }
    if !state.batch_running() {
        let theme = *ctx.theme();
        let choose = grana_ui::widget::action_button(
            &theme,
            Icon::Folder,
            grana_i18n::batch::choose_folder(language),
            Role::Control,
        )
        .named("batch.output")
        .press(Message::Command(Command::ChooseBatchOutput));
        let open = view::button(ctx, grana_i18n::batch::open_run(language), Role::Quiet)
            .named("batch.open")
            .press(Message::Command(Command::OpenBatchRun));
        children.push(
            grana_ui::row()
                .gap(8.0)
                .align(Align::Center)
                .children([choose, open]),
        );
        let jobs = state.batch_jobs();
        let workers = grana_ui::widget::drag_value(&theme, jobs as f32, 0)
            .named("batch.jobs")
            .on_drag_value(0.15, move |value| {
                let next = (value.round() as i64).clamp(1, 16) as u32;
                if next as usize != jobs {
                    Message::Command(Command::SetBatchJobs(next))
                } else {
                    Message::Noop
                }
            });
        let workers = grana_ui::row().gap(8.0).align(Align::Center).children([
            workers,
            view::caption(ctx, &grana_i18n::batch::concurrent(language, jobs)),
        ]);
        children.push(view::form_row(
            ctx,
            grana_i18n::batch::workers(language),
            workers,
        ));
        children.push(view::toggle_row(
            ctx,
            "batch.project",
            grana_i18n::batch::save_project(language),
            state.batch_project(),
            Command::SetBatchProject,
        ));
        children.push(view::toggle_row(
            ctx,
            "batch.channels",
            grana_i18n::batch::export_channels(language),
            state.batch_channels(),
            Command::SetBatchChannels,
        ));
    }
    children.push(view::divider(ctx));
    if state.batch_running() {
        let paused = state.batch_paused();
        let mut actions = vec![
            view::spinner(ctx, 14.0),
            view::caption(
                ctx,
                if paused {
                    grana_i18n::batch::paused(language)
                } else {
                    grana_i18n::batch::running_protocol(language)
                },
            ),
        ];
        if paused {
            let theme = *ctx.theme();
            actions.push(
                grana_ui::widget::action_button(
                    &theme,
                    Icon::Play,
                    grana_i18n::batch::resume(language),
                    Role::Quiet,
                )
                .named("batch.resume")
                .press(Message::Command(Command::ResumeBatch)),
            );
        } else {
            let theme = *ctx.theme();
            actions.push(
                grana_ui::widget::action_button(
                    &theme,
                    Icon::Pause,
                    grana_i18n::batch::pause(language),
                    Role::Quiet,
                )
                .named("batch.pause")
                .press(Message::Command(Command::PauseBatch)),
            );
        }
        actions.push(
            view::button(ctx, grana_i18n::common::cancel(language), Role::Quiet)
                .press(Message::Command(Command::CancelBatch)),
        );
        children.push(
            grana_ui::row()
                .gap(8.0)
                .align(Align::Center)
                .children(actions),
        );
    } else {
        let ready = state.batch_protocol().is_some()
            && !state.batch_datasets().is_empty()
            && state.batch_output().is_some();
        let theme = *ctx.theme();
        let run = grana_ui::widget::button_enabled(
            &theme,
            grana_i18n::batch::run_batch(language),
            Role::Prominent,
            Length::Fill,
            ready,
        )
        .named("batch.run");
        children.push(if ready {
            run.press(Message::Command(Command::RunBatch))
        } else {
            run
        });
    }
    if let Some(message) = state.batch_message() {
        let style =
            TextStyle::new(ctx.theme().font(Level::Caption), ctx.theme().palette.danger).wrapping();
        children.push(grana_ui::text(message, style));
    }
    children.push(view::weak(ctx, grana_i18n::batch::resume_hint(language)));
    view::card(ctx, 7.0, children)
}

const CHART_HEIGHT: f32 = 214.0;

fn comparison<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let language = ctx.language();
    let rows = state.batch_comparison();
    let significant = state
        .batch_significance()
        .iter()
        .filter(|test| test.frame.is_none() && test.significant())
        .count();
    let tests = state
        .batch_significance()
        .iter()
        .filter(|test| test.frame.is_none())
        .count();
    let note = (!rows.is_empty()).then(|| {
        if tests == 0 {
            grana_i18n::batch::no_group_test(language).to_string()
        } else {
            grana_i18n::batch::significant(language, significant, tests)
        }
    });
    let mut children = vec![view::card_title(
        ctx,
        Icon::Table,
        grana_i18n::batch::comparison(language),
        note.as_deref(),
    )];
    if rows.is_empty() {
        children.push(view::weak(
            ctx,
            grana_i18n::batch::comparison_hint(language),
        ));
        return view::card(ctx, 6.0, children);
    }
    let metric = state.workspace.comparison_metric;
    let axis = state.workspace.comparison_axis;
    let labels = ComparisonMetric::ALL.map(|metric| metric_label(language, metric));
    let axes = [
        grana_i18n::batch::conditions(language),
        grana_i18n::batch::frames(language),
    ];
    children.push(grana_ui::row().gap(8.0).align(Align::Center).children([
        view::segmented(
            ctx,
            "comparison.axis",
            &axes,
            usize::from(axis.index()),
            Message::SetComparisonAxis,
        ),
        view::segmented(
            ctx,
            "comparison.metric",
            &labels,
            metric.index(),
            Message::SetComparisonMetric,
        ),
        view::icon_button(
            ctx,
            "comparison.export",
            Icon::Export,
            28.0,
            Role::Quiet,
            grana_i18n::batch::export_comparison(language),
            Message::Command(Command::SaveComparison),
        ),
    ]));
    let groups = group_names(rows);
    let group = state
        .workspace
        .comparison_group
        .clone()
        .filter(|group| groups.contains(group));
    if groups.len() > 1 {
        let mut items = vec![(None, grana_i18n::batch::all_groups(language).to_string())];
        items.extend(
            groups
                .iter()
                .map(|group| (Some(group.clone()), group.clone())),
        );
        let group_combo = crate::inspector::combo(
            ctx,
            "comparison.group",
            Length::Fill,
            220.0,
            &items,
            Some(group.clone()),
            Message::SetComparisonGroup,
        );
        children.push(view::form_row(
            ctx,
            grana_i18n::batch::group(language),
            group_combo,
        ));
    }
    let sets = set_names(rows, group.as_deref());
    let key = state
        .workspace
        .comparison_key
        .clone()
        .filter(|key| sets.iter().any(|(other, _)| other == key));
    if sets.len() > 1 {
        let mut items = vec![(None, grana_i18n::batch::all_sets(language).to_string())];
        items.extend(sets.iter().cloned().map(|(key, name)| (Some(key), name)));
        let set_combo = crate::inspector::combo(
            ctx,
            "comparison.set",
            Length::Fill,
            220.0,
            &items,
            Some(key.clone()),
            Message::SetComparisonKey,
        );
        children.push(view::form_row(
            ctx,
            grana_i18n::batch::set(language),
            set_combo,
        ));
    }
    let visible = rows
        .iter()
        .filter(|row| group.as_ref().is_none_or(|group| group == &row.group))
        .filter(|row| key.as_ref().is_none_or(|key| key == &row.key))
        .collect::<Vec<_>>();
    if visible.is_empty() {
        children.push(view::weak(ctx, grana_i18n::batch::filter_empty(language)));
        return view::card(ctx, 6.0, children);
    }
    let Some(built) = comparison_chart(ComparisonRequest {
        rows,
        tests: state.batch_significance(),
        interactions: state.batch_interactions(),
        metric,
        axis,
        group: group.as_deref(),
        key: key.as_deref(),
        style: state.workspace.distribution_style,
        language: ctx.language(),
    }) else {
        children.push(view::weak(ctx, grana_i18n::batch::filter_empty(language)));
        return view::card(ctx, 6.0, children);
    };
    let slots = built.slots;
    let page = built.page;
    let style = Style::card(ctx.theme(), page.distribution()).with_language(ctx.language());
    let pointer = ctx.pointer();
    let count = slots.len();
    let canvas = grana_ui::canvas(Length::Fill, CHART_HEIGHT, move |painter, info| {
        let decor = Decor {
            pointer: info.rect.contains(pointer).then_some(pointer),
            ..Decor::default()
        };
        page.paint(painter, info.rect, &style, &decor);
    })
    .key(Key::new("comparison.chart"))
    .cursor(Cursor::Crosshair)
    .on_pointer(move |pointer| {
        if pointer.phase != Phase::Up || !pointer.rect.contains(pointer.position) || count == 0 {
            return None;
        }
        let frame = style.metrics.frame(pointer.rect);
        let index = grana_chart::group_at(frame, count, pointer.position)?;
        let (group, key) = slots.get(index)?.clone();
        Some(Message::ToggleComparisonDetail { group, key })
    });
    children.push(canvas);
    let open = state
        .workspace
        .comparison_open
        .as_ref()
        .and_then(|(group, key)| {
            visible
                .iter()
                .find(|row| &row.group == group && &row.key == key)
                .copied()
        });
    if let Some(row) = open {
        children.push(view::divider(ctx));
        children.extend(detail(ctx, state, row, metric));
    }
    children.push(view::weak(
        ctx,
        grana_i18n::batch::comparison_note(language),
    ));
    view::card(ctx, 6.0, children)
}

fn detail<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    row: &grana_session::BatchComparison,
    metric: ComparisonMetric,
) -> Vec<Element<'a, Message>> {
    let language = ctx.language();
    let theme = *ctx.theme();
    let palette = theme.palette;
    let tests = state
        .batch_significance()
        .iter()
        .filter(|test| test.frame.is_none())
        .filter(|test| test.key == row.key && test.metric == metric && test.involves(&row.group))
        .collect::<Vec<_>>();
    let best = tests.iter().map(|test| test.adjusted).fold(1.0, f64::min);
    let mut readout = match (metric.value(row), metric.deviation(row)) {
        (Some(mean), Some(deviation)) => grana_i18n::batch::value_deviation(
            language,
            &title(row),
            &metric.format(mean),
            &metric.format(deviation),
        ),
        (Some(mean), None) => {
            grana_i18n::batch::value_mean(language, &title(row), &metric.format(mean))
        }
        (None, _) => grana_i18n::batch::value_missing(language, &title(row)),
    };
    readout.push_str(&grana_i18n::batch::objects_in_images(
        language,
        row.objects,
        row.images,
    ));
    if best < 0.05 {
        readout.push_str(&grana_i18n::batch::q_value(
            language,
            best,
            grana_measure::stars(best),
        ));
    }
    let mut parts = vec![
        grana_ui::text(
            readout,
            TextStyle::new(theme.font(Level::Callout), palette.text_primary).wrapping(),
        ),
        grana_ui::text(
            sample_line(language, row, metric),
            TextStyle::new(theme.font(Level::Caption), palette.text_secondary).wrapping(),
        ),
    ];
    if !tests.is_empty() {
        parts.push(grana_ui::text(
            test_line(language, &row.group, &tests),
            TextStyle::new(theme.font(Level::Caption), palette.text_secondary).wrapping(),
        ));
    }
    parts
}

fn title(row: &grana_session::BatchComparison) -> String {
    format!("{} · {}", row.group, row.name)
}

fn sample_line(
    language: grana_i18n::Language,
    row: &grana_session::BatchComparison,
    metric: ComparisonMetric,
) -> String {
    const LIMIT: usize = 12;
    if row.samples.is_empty() {
        return grana_i18n::batch::no_samples(language).to_string();
    }
    let mut parts = row
        .samples
        .iter()
        .take(LIMIT)
        .map(|sample| match metric.sample(sample) {
            Some(value) => grana_i18n::batch::sample_value(
                language,
                &sample.dataset,
                &metric.format(value),
                sample.objects,
            ),
            None => grana_i18n::batch::sample_missing(language, &sample.dataset),
        })
        .collect::<Vec<_>>();
    if row.samples.len() > LIMIT {
        parts.push(grana_i18n::batch::more_samples(
            language,
            row.samples.len() - LIMIT,
        ));
    }
    parts.join("   ·   ")
}

fn test_line(
    language: grana_i18n::Language,
    group: &str,
    tests: &[&grana_session::BatchSignificance],
) -> String {
    tests
        .iter()
        .map(|test| {
            let effect = test
                .effect
                .map_or_else(|| "-".to_string(), |effect| format!("{effect:.2}"));
            let interval = test.interval_text().unwrap_or_else(|| "-".to_string());
            let experiment = grana_i18n::batch::test_groups(
                language,
                group,
                test.other(group),
                test.test.samples_a(),
                test.test.clusters_a(),
                &test.p_text(),
                &test.q_text(),
            );
            let rank = grana_i18n::batch::test_rank(
                language,
                &test.rank_p_text(),
                &test.rank_q_text(),
                &effect,
                test.rank.delta(),
                &interval,
                test.mark(),
            );
            format!("{experiment} · {rank}")
        })
        .collect::<Vec<_>>()
        .join("   ·   ")
}

fn metric_label(language: grana_i18n::Language, metric: ComparisonMetric) -> &'static str {
    match metric {
        ComparisonMetric::Objects => grana_i18n::batch::metric_objects(language),
        ComparisonMetric::Volume => grana_i18n::batch::metric_volume(language),
        ComparisonMetric::Area => grana_i18n::batch::metric_area(language),
        ComparisonMetric::Size => grana_i18n::batch::metric_size(language),
        ComparisonMetric::Intensity => grana_i18n::batch::metric_intensity(language),
    }
}

fn state_tint(state: &str, theme: &grana_ui::Theme) -> Color {
    match state {
        "complete" | "done" | "resumed" => theme.palette.success,
        "corrected" => theme.palette.warning,
        "failed" => theme.palette.danger,
        "cancelled" => theme.palette.text_tertiary,
        "running" => theme.palette.warning,
        _ => theme.palette.text_secondary,
    }
}

fn state_label(language: grana_i18n::Language, state: &str) -> &str {
    match state {
        "complete" | "done" => grana_i18n::batch::complete(language),
        "resumed" => grana_i18n::batch::resumed(language),
        "corrected" => grana_i18n::batch::corrected(language),
        "failed" => grana_i18n::batch::failed(language),
        "cancelled" => grana_i18n::batch::cancelled(language),
        "running" => grana_i18n::batch::running_state(language),
        "pending" => grana_i18n::batch::pending(language),
        "queued" => grana_i18n::batch::queued(language),
        other => other,
    }
}
