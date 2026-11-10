use glam::Vec2;
use grana_session::Command;
use grana_ui::{
    Align, Axis, BuildContext, Color, Corners, Cursor, Edges, Element, Icon, Length, Level, Rect,
    Role, State, TextStyle, icon,
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

const COMPARISON_ROW: f32 = 58.0;

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
    let detail = (!rows.is_empty()).then(|| {
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
        detail.as_deref(),
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
    let max = visible
        .iter()
        .flat_map(|row| {
            std::iter::once(metric.value(row))
                .chain(row.samples.iter().map(|sample| metric.sample(sample)))
        })
        .flatten()
        .fold(0.0f64, f64::max)
        .max(f64::MIN_POSITIVE);
    let mut entries = Vec::with_capacity(visible.len());
    for row in &visible {
        entries.push(comparison_row(ctx, state, row, metric, max));
    }
    children.push(
        Element::new(grana_ui::Layout::column())
            .scroll(Axis::Column, Length::Fixed(240.0), None)
            .clipped()
            .children(entries),
    );
    children.push(view::weak(
        ctx,
        "Objects are the samples and images are the clusters; the Welch test corrects for the clustering and the rank test compares image means. q is the Benjamini–Hochberg correction over the run.",
    ));
    view::card(ctx, 6.0, children)
}

fn comparison_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    row: &grana_session::BatchComparison,
    metric: ComparisonMetric,
    max: f64,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let callout = theme.font(Level::Callout);
    let caption = theme.font(Level::Caption);
    let micro = theme.font(Level::Micro);
    let title = format!("{} · {}", row.group, row.name);
    let kind = row.kind.clone();
    let mean = metric.value(row);
    let tests = state
        .batch_significance()
        .iter()
        .filter(|test| test.key == row.key && test.metric == metric && test.involves(&row.group))
        .collect::<Vec<_>>();
    let best = tests.iter().map(|test| test.adjusted).fold(1.0, f64::min);
    let mark = if best < 0.05 {
        grana_measure::stars(best)
    } else {
        ""
    };
    let readout = match metric.value(row) {
        Some(mean) => match metric.deviation(row) {
            Some(deviation) => format!("{} ± {}", metric.format(mean), metric.format(deviation)),
            None => metric.format(mean),
        },
        None => "No value".to_string(),
    };
    let readout = match mark {
        "" => readout,
        mark => format!("{readout}   {mark}"),
    };
    let count = format!(
        "{} object{} in {} image{}",
        row.objects,
        if row.objects == 1 { "" } else { "s" },
        row.images,
        if row.images == 1 { "" } else { "s" }
    );
    let samples = row
        .samples
        .iter()
        .filter_map(|sample| metric.sample(sample))
        .map(|value| (value / max).clamp(0.0, 1.0) as f32)
        .collect::<Vec<_>>();
    let fraction = mean.map(|mean| (mean / max).clamp(0.0, 1.0) as f32);
    let open = state
        .workspace
        .comparison_open
        .as_ref()
        .is_some_and(|(group, key)| group == &row.group && key == &row.key);
    let key = row.key.clone();
    let group = row.group.clone();
    let canvas = grana_ui::canvas(Length::Fill, COMPARISON_ROW, move |painter, info| {
        let rect = info.rect;
        if info.hover > 0.01 {
            painter.rect(
                rect,
                Corners::all(8.0),
                Color::white_alpha(0.05 * info.hover),
            );
        }
        let top = Rect::from_min_max(
            Vec2::new(rect.min.x + 10.0, rect.min.y + 5.0),
            Vec2::new(rect.max.x - 10.0, rect.min.y + 22.0),
        );
        painter.text_in(
            top,
            &title,
            TextStyle::new(callout, palette.text_primary).truncating(),
            Align::Center,
        );
        painter.text_in(
            top,
            &kind,
            TextStyle::new(micro, palette.text_tertiary).aligned(Align::End),
            Align::Center,
        );
        let track = Rect::from_min_size(
            Vec2::new(rect.min.x + 10.0, rect.min.y + 26.0),
            Vec2::new((rect.width() - 20.0).max(1.0), 6.0),
        );
        painter.rect(track, Corners::capsule(), palette.track.with_alpha(0.55));
        if let Some(fraction) = fraction.filter(|fraction| *fraction > 0.0) {
            painter.rect(
                Rect::from_min_size(
                    track.min,
                    Vec2::new(track.width() * fraction, track.height()),
                ),
                Corners::capsule(),
                palette.accent.with_alpha(0.55),
            );
        }
        for sample in &samples {
            painter.ring(
                Vec2::new(track.min.x + track.width() * sample, track.center().y),
                3.0,
                1.6,
                0.0,
                std::f32::consts::TAU,
                palette.success,
            );
        }
        let bottom = Rect::from_min_max(
            Vec2::new(rect.min.x + 10.0, rect.min.y + 34.0),
            Vec2::new(rect.max.x - 10.0, rect.max.y - 2.0),
        );
        painter.text_in(
            bottom,
            &readout,
            TextStyle::new(caption, palette.text_secondary).truncating(),
            Align::Center,
        );
        painter.text_in(
            bottom,
            &count,
            TextStyle::new(micro, palette.text_tertiary).aligned(Align::End),
            Align::Center,
        );
    })
    .press(Message::ToggleComparisonDetail { group, key })
    .cursor(Cursor::Pointing)
    .focus_ring(Corners::all(8.0));
    let mut parts = vec![canvas];
    if open {
        parts.push(grana_ui::text(
            sample_line(row, metric),
            TextStyle::new(caption, palette.text_secondary).wrapping(),
        ));
        if !tests.is_empty() {
            parts.push(grana_ui::text(
                test_line(&row.group, &tests),
                TextStyle::new(caption, palette.text_secondary).wrapping(),
            ));
        }
    }
    grana_ui::column().gap(4.0).children(parts)
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
