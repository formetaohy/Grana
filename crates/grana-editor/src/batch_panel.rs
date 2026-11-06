use glam::Vec2;
use grana_session::Command;
use grana_ui::{
    Align, Axis, BuildContext, Color, Corners, Edges, Element, Icon, Length, Level, Rect, Role,
    State, TextStyle, icon,
};

use crate::message::Message;
use crate::state::View;
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
        for dataset in state.batch_datasets() {
            let tint = state_tint(&dataset.state, &theme);
            let name = dataset.name.clone();
            let label = state_label(&dataset.state).to_string();
            let detail = dataset.detail.clone();
            let row = grana_ui::canvas(Length::Fill, 30.0, move |painter, info| {
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
        children.push(
            grana_ui::widget::action_button(&theme, Icon::Folder, "Choose Folder…", Role::Control)
                .named("batch.output")
                .press(Message::Command(Command::ChooseBatchOutput)),
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

fn comparison<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let rows = state.batch_comparison();
    let mut children = vec![view::card_title(
        ctx,
        Icon::Table,
        "Comparison",
        (!rows.is_empty())
            .then(|| format!("{} groups", rows.len()))
            .as_deref(),
    )];
    if rows.is_empty() {
        children.push(view::weak(
            ctx,
            "Run the batch to compare reported groups in comparison.csv.",
        ));
        return view::card(ctx, 6.0, children);
    }
    let theme = *ctx.theme();
    let palette = theme.palette;
    let callout = theme.font(Level::Callout);
    let caption = theme.font(Level::Caption);
    let mut entries = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let head = grana_ui::canvas(Length::Fill, 18.0, {
            let group = row.group.clone();
            let name = row.name.clone();
            let kind = row.kind.clone();
            move |painter, info| {
                painter.text_in(
                    info.rect,
                    &format!("{group} · {name}"),
                    TextStyle::new(callout, palette.text_primary).truncating(),
                    Align::Center,
                );
                painter.text_in(
                    info.rect,
                    &kind,
                    TextStyle::new(caption, palette.text_tertiary).aligned(Align::End),
                    Align::Center,
                );
            }
        });
        let mut parts = vec![format!(
            "{} dataset{}",
            row.datasets,
            if row.datasets == 1 { "" } else { "s" }
        )];
        if let Some(objects) = row.objects {
            parts.push(format!("{objects:.1} objects"));
        }
        if let Some(volume) = row.volume {
            parts.push(format!(
                "Volume {} µm³",
                spread(volume, row.volume_deviation)
            ));
        }
        if let Some(area) = row.area {
            parts.push(format!("Area {} µm²", spread(area, row.area_deviation)));
        }
        if let Some(size) = row.size {
            parts.push(format!("Size {} µm", spread(size, row.size_deviation)));
        }
        if let Some(intensity) = row.intensity {
            parts.push(format!(
                "Intensity {}",
                spread(intensity, row.intensity_deviation)
            ));
        }
        let detail = grana_ui::text(
            parts.join(" · "),
            TextStyle::new(caption, palette.text_secondary).wrapping(),
        );
        entries.push(grana_ui::column().gap(2.0).children([head, detail]));
        if index + 1 < rows.len() {
            entries.push(view::divider(ctx));
        }
    }
    children.push(
        Element::new(grana_ui::Layout::column())
            .scroll(Axis::Column, Length::Fixed(180.0), None)
            .clipped()
            .children(entries),
    );
    view::card(ctx, 6.0, children)
}

fn spread(mean: f64, deviation: Option<f64>) -> String {
    match deviation {
        Some(deviation) => format!("{mean:.3} ±{deviation:.3}"),
        None => format!("{mean:.3}"),
    }
}

fn state_tint(state: &str, theme: &grana_ui::Theme) -> Color {
    match state {
        "complete" | "done" | "resumed" => theme.palette.success,
        "failed" => theme.palette.danger,
        "running" => theme.palette.warning,
        _ => theme.palette.text_secondary,
    }
}

fn state_label(state: &str) -> &str {
    match state {
        "complete" | "done" => "Complete",
        "resumed" => "Resumed",
        "failed" => "Failed",
        "running" => "Running",
        "pending" => "Pending",
        "queued" => "Queued",
        other => other,
    }
}
