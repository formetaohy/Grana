use glam::Vec2;
use grana_chart::{Comparison, Decor, Heading, Style};
use grana_session::Command;
use grana_ui::{
    Align, Axis, BuildContext, Color, Corners, Cursor, Edges, Element, Icon, Key, Length, Level,
    Phase, Rect, Role, State, TextStyle, icon,
};

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
    if !running {
        actions.push(
            grana_ui::widget::action_button(&theme, Icon::Camera, "Capture", Role::Control)
                .named("batch.capture")
                .press(Message::Command(Command::CaptureProtocol)),
        );
        actions.push(
            view::button(ctx, "Load", Role::Quiet).press(Message::Command(Command::LoadProtocol)),
        );
        actions.push(
            view::button(ctx, "Save", Role::Quiet).press(Message::Command(Command::SaveProtocol)),
        );
        actions.push(
            view::button(ctx, "Provenance", Role::Quiet)
                .press(Message::Command(Command::ExportProvenance)),
        );
        if has_protocol {
            actions.push(
                view::button(ctx, "Apply", Role::Quiet)
                    .press(Message::Command(Command::ApplyProtocol)),
            );
            if let Some(text) = state.batch_protocol() {
                actions.push(
                    view::button(ctx, "Copy", Role::Quiet).press(Message::Copy(text.to_string())),
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
            "Protocol",
            if has_protocol { Some("Ready") } else { None },
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
        None => children.push(view::weak(
            ctx,
            "Capture the current region, objects and settings into a reusable protocol.",
        )),
    }
    view::card(ctx, 6.0, children)
}

fn datasets<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let running = state.batch_running();
    let count = state.batch_datasets().len();
    let mut children = vec![view::card_title(
        ctx,
        Icon::Folder,
        "Datasets",
        Some(&format!(
            "{count} item{}",
            if count == 1 { "" } else { "s" }
        )),
    )];
    if state.batch_datasets().is_empty() {
        children.push(view::weak(
            ctx,
            "Choose a folder of datasets that share this protocol.",
        ));
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
            let label = state_label(&dataset.state).to_string();
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
            grana_ui::widget::action_button(&theme, Icon::Plus, "Add Folder…", Role::Control)
                .named("batch.add")
                .press(Message::Command(Command::AddBatchDatasets)),
        );
        if !state.batch_datasets().is_empty() {
            actions.push(
                view::button(ctx, "Clear", Role::Quiet)
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
        let name = state
            .batch_datasets()
            .get(index)
            .map_or_else(|| "the dataset".to_string(), |dataset| dataset.name.clone());
        children.push(view::divider(ctx));
        children.push(view::caption(
            ctx,
            &format!(
                "Reviewing {name}. Edit its objects or parameters, then record the correction."
            ),
        ));
        let ready = state.batch_review_ready() && !running;
        let corrected = state
            .batch_datasets()
            .get(index)
            .is_some_and(|row| row.state.as_str() == "corrected");
        let theme = *ctx.theme();
        let record = grana_ui::widget::button_enabled(
            &theme,
            "Record Correction",
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
                view::button(ctx, "Clear Correction", Role::Quiet)
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
            "The next run applies the correction to this dataset only.",
        ));
    }
    view::card(ctx, 6.0, children)
}

fn output<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let mut children = vec![view::card_title(
        ctx,
        Icon::Export,
        "Output & Execution",
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
        children.push(view::weak(
            ctx,
            "Choose a destination folder for the exported tables.",
        ));
    }
    if !state.batch_running() {
        let theme = *ctx.theme();
        let choose =
            grana_ui::widget::action_button(&theme, Icon::Folder, "Choose Folder…", Role::Control)
                .named("batch.output")
                .press(Message::Command(Command::ChooseBatchOutput));
        let open = view::button(ctx, "Open Run", Role::Quiet)
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
        let workers = grana_ui::row()
            .gap(8.0)
            .align(Align::Center)
            .children([workers, view::caption(ctx, &format!("{jobs} concurrent"))]);
        children.push(view::form_row(ctx, "Workers", workers));
        children.push(view::toggle_row(
            ctx,
            "batch.project",
            "Save Project per Dataset",
            state.batch_project(),
            Command::SetBatchProject,
        ));
        children.push(view::toggle_row(
            ctx,
            "batch.channels",
            "Export Derived Channels",
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
                    "Paused between analysis steps"
                } else {
                    "Running protocol across datasets…"
                },
            ),
        ];
        if paused {
            let theme = *ctx.theme();
            actions.push(
                grana_ui::widget::action_button(&theme, Icon::Play, "Resume", Role::Quiet)
                    .named("batch.resume")
                    .press(Message::Command(Command::ResumeBatch)),
            );
        } else {
            let theme = *ctx.theme();
            actions.push(
                grana_ui::widget::action_button(&theme, Icon::Pause, "Pause", Role::Quiet)
                    .named("batch.pause")
                    .press(Message::Command(Command::PauseBatch)),
            );
        }
        actions.push(
            view::button(ctx, "Cancel", Role::Quiet).press(Message::Command(Command::CancelBatch)),
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
            "Run Batch",
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
    children.push(view::weak(
        ctx,
        "Re-running the same protocol resumes where it stopped.",
    ));
    view::card(ctx, 7.0, children)
}

const CHART_HEIGHT: f32 = 214.0;
const NOTE_LIMIT: usize = 3;

fn comparison<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let rows = state.batch_comparison();
    let significant = state
        .batch_significance()
        .iter()
        .filter(|test| test.significant())
        .count();
    let tests = state.batch_significance().len();
    let note = (!rows.is_empty()).then(|| {
        if tests == 0 {
            "no group test".to_string()
        } else {
            format!("{significant}/{tests} significant")
        }
    });
    let mut children = vec![view::card_title(
        ctx,
        Icon::Table,
        "Comparison",
        note.as_deref(),
    )];
    if rows.is_empty() {
        children.push(view::weak(
            ctx,
            "Run the batch, or open a run record, to compare the reported groups.",
        ));
        return view::card(ctx, 6.0, children);
    }
    let metric = state.workspace.comparison_metric;
    let labels = ComparisonMetric::ALL.map(ComparisonMetric::name);
    children.push(view::segmented(
        ctx,
        "comparison.metric",
        &labels,
        metric.index(),
        Message::SetComparisonMetric,
    ));
    let groups = group_names(rows);
    let group = state
        .workspace
        .comparison_group
        .clone()
        .filter(|group| groups.contains(group));
    if groups.len() > 1 {
        let mut items = vec![(None, "All groups".to_string())];
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
        children.push(view::form_row(ctx, "Group", group_combo));
    }
    let sets = set_names(rows, group.as_deref());
    let key = state
        .workspace
        .comparison_key
        .clone()
        .filter(|key| sets.iter().any(|(other, _)| other == key));
    if sets.len() > 1 {
        let mut items = vec![(None, "All sets".to_string())];
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
        children.push(view::form_row(ctx, "Set", set_combo));
    }
    let visible = rows
        .iter()
        .filter(|row| group.as_ref().is_none_or(|group| group == &row.group))
        .filter(|row| key.as_ref().is_none_or(|key| key == &row.key))
        .collect::<Vec<_>>();
    if visible.is_empty() {
        children.push(view::weak(ctx, "No reported group matches this filter."));
        return view::card(ctx, 6.0, children);
    }
    let categories = visible
        .iter()
        .map(|row| grana_chart::Category {
            label: category_label(row, groups.len(), sets.len()),
            values: row
                .samples
                .iter()
                .filter_map(|sample| metric.sample(sample))
                .map(|value| value as f32)
                .collect(),
        })
        .collect::<Vec<_>>();
    let slots = visible
        .iter()
        .map(|row| (row.group.clone(), row.key.clone()))
        .collect::<Vec<_>>();
    let notes = comparison_notes(&slots, state.batch_significance(), metric);
    let heading =
        Heading::new("", format!("Y: {}", metric_axis(metric)), "X: Conditions").with_summary(
            format!("{} categories · images are the samples", visible.len()),
        );
    let chart = Comparison {
        heading,
        categories,
        notes,
    };
    let style = Style::card(ctx.theme(), state.workspace.distribution_style);
    let pointer = ctx.pointer();
    let count = slots.len();
    let canvas = grana_ui::canvas(Length::Fill, CHART_HEIGHT, move |painter, info| {
        let decor = Decor {
            pointer: info.rect.contains(pointer).then_some(pointer),
            ..Decor::default()
        };
        chart.paint(painter, info.rect, &style, &decor);
    })
    .key(Key::new("comparison.chart"))
    .cursor(Cursor::Crosshair)
    .on_pointer(move |pointer| {
        if pointer.phase != Phase::Up || !pointer.rect.contains(pointer.position) {
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
        "Objects are the samples and images are the clusters; the Welch test corrects for the clustering and the rank test compares image means. q is the Benjamini–Hochberg correction over the run. Click a condition for its numbers.",
    ));
    view::card(ctx, 6.0, children)
}

fn detail<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    row: &grana_session::BatchComparison,
    metric: ComparisonMetric,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let tests = state
        .batch_significance()
        .iter()
        .filter(|test| test.key == row.key && test.metric == metric && test.involves(&row.group))
        .collect::<Vec<_>>();
    let best = tests.iter().map(|test| test.adjusted).fold(1.0, f64::min);
    let mut readout = match (metric.value(row), metric.deviation(row)) {
        (Some(mean), Some(deviation)) => format!(
            "{} · {} ± {}",
            title(row),
            metric.format(mean),
            metric.format(deviation)
        ),
        (Some(mean), None) => format!("{} · {}", title(row), metric.format(mean)),
        (None, _) => format!("{} · no value", title(row)),
    };
    readout.push_str(&format!(
        " · {} objects in {} images",
        row.objects, row.images
    ));
    if best < 0.05 {
        readout.push_str(&format!(" · q {:.3} {}", best, grana_measure::stars(best)));
    }
    let mut parts = vec![
        grana_ui::text(
            readout,
            TextStyle::new(theme.font(Level::Callout), palette.text_primary).wrapping(),
        ),
        grana_ui::text(
            sample_line(row, metric),
            TextStyle::new(theme.font(Level::Caption), palette.text_secondary).wrapping(),
        ),
    ];
    if !tests.is_empty() {
        parts.push(grana_ui::text(
            test_line(&row.group, &tests),
            TextStyle::new(theme.font(Level::Caption), palette.text_secondary).wrapping(),
        ));
    }
    parts
}

fn title(row: &grana_session::BatchComparison) -> String {
    format!("{} · {}", row.group, row.name)
}

fn category_label(row: &grana_session::BatchComparison, groups: usize, sets: usize) -> String {
    match (groups, sets) {
        (1, _) => row.name.clone(),
        (_, 1) => row.group.clone(),
        _ => title(row),
    }
}

fn metric_axis(metric: ComparisonMetric) -> String {
    match metric.unit() {
        "" => metric.name().to_string(),
        unit => format!("{} ({unit})", metric.name()),
    }
}

fn comparison_notes(
    slots: &[(String, String)],
    tests: &[grana_session::BatchSignificance],
    metric: ComparisonMetric,
) -> Vec<grana_chart::Note> {
    let mut notes = tests
        .iter()
        .filter(|test| test.metric == metric && test.significant())
        .filter_map(|test| {
            let from = slots
                .iter()
                .position(|(group, key)| group == &test.group_a && key == &test.key)?;
            let to = slots
                .iter()
                .position(|(group, key)| group == &test.group_b && key == &test.key)?;
            if from == to {
                return None;
            }
            Some((
                (from.abs_diff(to), from.min(to), from.max(to)),
                grana_chart::Note {
                    from,
                    to,
                    mark: test.mark().to_string(),
                },
            ))
        })
        .collect::<Vec<_>>();
    notes.sort_by_key(|(order, _)| *order);
    notes
        .into_iter()
        .take(NOTE_LIMIT)
        .map(|(_, note)| note)
        .collect()
}

fn group_names(rows: &[grana_session::BatchComparison]) -> Vec<String> {
    let mut names = Vec::new();
    for row in rows {
        if !names.contains(&row.group) {
            names.push(row.group.clone());
        }
    }
    names
}

fn set_names(
    rows: &[grana_session::BatchComparison],
    group: Option<&str>,
) -> Vec<(String, String)> {
    let mut names = Vec::new();
    for row in rows {
        if group.is_some_and(|group| group != row.group) {
            continue;
        }
        if !names.iter().any(|(key, _)| key == &row.key) {
            names.push((row.key.clone(), row.name.clone()));
        }
    }
    names
}

fn sample_line(row: &grana_session::BatchComparison, metric: ComparisonMetric) -> String {
    const LIMIT: usize = 12;
    if row.samples.is_empty() {
        return "The run recorded no dataset samples.".to_string();
    }
    let mut parts = row
        .samples
        .iter()
        .take(LIMIT)
        .map(|sample| match metric.sample(sample) {
            Some(value) => format!(
                "{}: {} ({} objects)",
                sample.dataset,
                metric.format(value),
                sample.objects
            ),
            None => format!("{}: no value", sample.dataset),
        })
        .collect::<Vec<_>>();
    if row.samples.len() > LIMIT {
        parts.push(format!("{} more", row.samples.len() - LIMIT));
    }
    parts.join("   ·   ")
}

fn test_line(group: &str, tests: &[&grana_session::BatchSignificance]) -> String {
    tests
        .iter()
        .map(|test| {
            let effect = test
                .effect
                .map_or_else(|| "-".to_string(), |effect| format!("{effect:.2}"));
            let interval = test
                .interval_text()
                .unwrap_or_else(|| "-".to_string());
            format!(
                "{} vs {} · {} samples in {} images · Welch p={} q={} · image rank p={} q={} · g={} Δ={:.2} · CI {interval} {}",
                group,
                test.other(group),
                test.test.samples_a(),
                test.test.clusters_a(),
                test.p_text(),
                test.q_text(),
                test.rank_p_text(),
                test.rank_q_text(),
                effect,
                test.rank.delta(),
                test.mark()
            )
        })
        .collect::<Vec<_>>()
        .join("   ·   ")
}

fn state_tint(state: &str, theme: &grana_ui::Theme) -> Color {
    match state {
        "complete" | "done" | "resumed" => theme.palette.success,
        "corrected" => theme.palette.warning,
        "failed" => theme.palette.danger,
        "running" => theme.palette.warning,
        _ => theme.palette.text_secondary,
    }
}

fn state_label(state: &str) -> &str {
    match state {
        "complete" | "done" => "Complete",
        "resumed" => "Resumed",
        "corrected" => "Corrected",
        "failed" => "Failed",
        "running" => "Running",
        "pending" => "Pending",
        "queued" => "Queued",
        other => other,
    }
}
