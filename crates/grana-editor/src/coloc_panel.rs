use glam::Vec2;
use grana_i18n::Language;
use grana_measure::{
    COLOC_BLOCK_MAX, COLOC_PERMUTATIONS_MAX, ColocPair, ColocPrecision, ColocSeries, ColocSettings,
    ColocStatistics, ColocThresholds,
};
use grana_scene::{Job, ObjectSetId, Progress};
use grana_session::Command;
use grana_ui::{
    Align, BuildContext, Color, Corners, Cursor, Element, Icon, Key, Length, Level, Rect, Role,
    State, TextStyle, TypeStyle, icon,
};

use crate::message::Message;
use crate::state::View;
use crate::view;

const PLOT_CELLS: usize = 160;
const PLOT_SIDE: f32 = 186.0;

pub fn view<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let language = ctx.language();
    let Some(pair) = state.scene.coloc.pair() else {
        return view::empty_state(
            ctx,
            Icon::Grid,
            grana_i18n::coloc::two_channels_title(language),
            grana_i18n::coloc::two_channels_detail(language),
        );
    };
    let running = view::progress(state.jobs, Job::Coloc);
    let header = header(ctx, state, pair, running);
    let content = if state.scene.coloc.result().is_none() {
        view::empty_state(
            ctx,
            Icon::Grid,
            grana_i18n::coloc::no_coloc_title(language),
            if running.is_some() {
                grana_i18n::coloc::no_coloc_streaming(language)
            } else {
                grana_i18n::coloc::no_coloc_detail(language)
            },
        )
    } else {
        let thresholds = thresholds(ctx, state);
        let side = diagnostics(ctx, state);
        let scatter = scatter(ctx, state);
        let first = grana_ui::column()
            .width(Length::Fixed(340.0))
            .gap(8.0)
            .children([thresholds]);
        let second = grana_ui::column()
            .width(Length::Fixed(380.0))
            .gap(8.0)
            .children(side);
        let third = grana_ui::column()
            .width(Length::Fill)
            .gap(8.0)
            .children([scatter]);
        grana_ui::row()
            .gap(12.0)
            .align(Align::Start)
            .children([first, second, third])
    };
    grana_ui::column().gap(8.0).children([header, content])
}

fn header<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    pair: ColocPair,
    running: Option<Progress>,
) -> Element<'a, Message> {
    let language = ctx.language();
    let names = state
        .scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>();
    let a = pair.a();
    let b = pair.b();
    let options_a = names
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != b)
        .map(|(_index, name)| name.clone())
        .collect::<Vec<_>>();
    let indices_a = names
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != b)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let options_b = names
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != a)
        .map(|(_index, name)| name.clone())
        .collect::<Vec<_>>();
    let indices_b = names
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != a)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let selected_a = indices_a.iter().position(|index| *index == a).unwrap_or(0);
    let selected_b = indices_b.iter().position(|index| *index == b).unwrap_or(0);
    let mut items = vec![
        field_label(ctx, grana_i18n::coloc::pair(language)),
        combo(
            ctx,
            "coloc.a",
            124.0,
            selected_a,
            &options_a,
            move |index| match indices_a.get(index) {
                Some(value) if *value != a => {
                    Message::Command(Command::SetColocPair(ColocPair::new(*value, b)))
                }
                _ => Message::Noop,
            },
        ),
        view::caption(ctx, "×"),
        combo(
            ctx,
            "coloc.b",
            124.0,
            selected_b,
            &options_b,
            move |index| match indices_b.get(index) {
                Some(value) if *value != b => {
                    Message::Command(Command::SetColocPair(ColocPair::new(a, *value)))
                }
                _ => Message::Noop,
            },
        ),
    ];
    let busy = running.is_some() || state.io.loading.is_some();
    match running {
        Some(running) => {
            items.push(view::spinner(ctx, 13.0));
            items.push(view::caption(
                ctx,
                &grana_i18n::coloc::samples(language, running.done, running.total),
            ));
            items.push(view::icon_button(
                ctx,
                "coloc.cancel",
                Icon::Close,
                28.0,
                Role::Quiet,
                grana_i18n::common::cancel(language),
                Message::Command(Command::CancelColoc),
            ));
        }
        None => {
            let theme = *ctx.theme();
            let compute = grana_ui::widget::action_button(
                &theme,
                Icon::Target,
                grana_i18n::coloc::compute(language),
                Role::Prominent,
            )
            .named("coloc.compute");
            items.push(if busy {
                compute.enabled(false)
            } else {
                compute.press(Message::Command(Command::ComputeColoc))
            });
        }
    }
    if state.scene.coloc.result().is_some() {
        items.push(view::icon_button(
            ctx,
            "coloc.export",
            Icon::Export,
            28.0,
            Role::Quiet,
            grana_i18n::coloc::export_csv(language),
            Message::Command(Command::ExportColoc),
        ));
    }
    let results = state.scene.coloc.results();
    for (index, result) in results.iter().enumerate() {
        let stored = result.pair();
        let label = format!("{} × {}", names[stored.a()], names[stored.b()]);
        let role = if stored == pair {
            Role::Tinted
        } else {
            Role::Quiet
        };
        items.push(
            view::button(ctx, &label, role)
                .key(Key::index("coloc.saved", index))
                .press(Message::Command(Command::SetColocPair(stored))),
        );
    }
    if state.scene.coloc.result().is_some() {
        items.push(view::icon_button(
            ctx,
            "coloc.remove",
            Icon::Trash,
            28.0,
            Role::Quiet,
            grana_i18n::coloc::remove_result(language),
            Message::Command(Command::RemoveColocResult(pair)),
        ));
    }
    grana_ui::row()
        .gap(8.0)
        .align(Align::Center)
        .children(items)
}

fn thresholds<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let Some(result) = state.scene.coloc.result() else {
        return grana_ui::column();
    };
    let pair = result.pair();
    let language = ctx.language();
    let thresholds = result.thresholds();
    let series = result.series();
    let tints = [
        state.scene.dataset.channels()[pair.a()].tint(),
        state.scene.dataset.channels()[pair.b()].tint(),
    ];
    let a = thresholds.a();
    let b = thresholds.b();
    let mut rows = Vec::new();
    for (index, tint) in tints.iter().enumerate() {
        let value = if index == 0 { a } else { b };
        let label = if index == 0 { "M1" } else { "M2" };
        let current = if index == 0 { b } else { a };
        let theme = *ctx.theme();
        let style = TextStyle::new(theme.font(Level::Monospace), theme.palette.text_primary)
            .aligned(Align::End);
        let readout = grana_ui::canvas(Length::Fixed(54.0), 28.0, move |painter, info| {
            painter.text_in(info.rect, &format!("{value:.3}"), style, Align::Center);
        });
        let slider = grana_ui::widget::slider_sized(
            &theme,
            None,
            value,
            0.0..=1.0,
            false,
            Length::Fixed(170.0),
        )
        .named(if index == 0 {
            "coloc.threshold.0"
        } else {
            "coloc.threshold.1"
        })
        .on_slide(move |next| {
            let thresholds = if index == 0 {
                ColocThresholds::new(next, current)
            } else {
                ColocThresholds::new(current, next)
            };
            Message::Command(Command::SetColocThresholds(thresholds))
        });
        let dot = grana_ui::widget::color_swatch(*tint, 8.5);
        rows.push(grana_ui::row().gap(6.0).align(Align::Center).children([
            dot,
            grana_ui::text(
                label,
                TextStyle::new(
                    theme.font(Level::Caption).medium(),
                    theme.palette.text_secondary,
                ),
            ),
            slider,
            readout,
        ]));
    }
    let otsu = series.otsu();
    let costes = series.costes();
    let channels = state.scene.dataset.channels();
    let mut buttons = vec![
        view::button(ctx, "Otsu", Role::Control)
            .press(Message::Command(Command::SetColocThresholds(otsu))),
    ];
    if let Some(range) = costes {
        buttons.push(
            view::button(ctx, "Costes", Role::Control)
                .press(Message::Command(Command::SetColocThresholds(range.high()))),
        );
    }
    let raw_a = channels[pair.a()].intensity().value(thresholds.a());
    let raw_b = channels[pair.b()].intensity().value(thresholds.b());
    buttons.push(view::caption(
        ctx,
        &grana_i18n::coloc::raw(language, raw_a, raw_b),
    ));
    rows.push(
        grana_ui::row()
            .gap(6.0)
            .align(Align::Center)
            .children(buttons),
    );
    if let Some(range) = costes {
        let low = range.low();
        let high = range.high();
        rows.push(view::caption(
            ctx,
            &format!(
                "Costes M1 {:.3}–{:.3} · M2 {:.3}–{:.3}",
                low.a(),
                high.a(),
                low.b(),
                high.b()
            ),
        ));
    }
    let title = view::card_title(
        ctx,
        Icon::Sliders,
        grana_i18n::coloc::thresholds(language),
        None,
    );
    let mut children = vec![title];
    children.extend(rows);
    view::card(ctx, 7.0, children)
}

fn diagnostics<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let mut cards = Vec::new();
    let language = ctx.language();
    if let Some(series) = state.scene.coloc.series_statistics() {
        let current = state.scene.coloc.statistics(state.scene.time());
        let theme = *ctx.theme();
        let palette = theme.palette;
        let caption_font = theme.font(Level::Caption);
        let mono_font = theme.font(Level::Monospace);
        let micro_font = theme.font(Level::Micro);
        let header_style = TextStyle::new(micro_font, palette.text_tertiary);
        let caption_style = TextStyle::new(caption_font, palette.text_tertiary);
        let value_style = TextStyle::new(mono_font, palette.text_secondary).aligned(Align::End);
        let rows = 1 + usize::from(current.is_some());
        let height = 18.0 + 9.0 + rows as f32 * 20.0;
        let series_values = metrics_values(series);
        let current_values = current.map(metrics_values);
        let time = state.scene.time();
        let headers = [
            grana_i18n::coloc::scope(language),
            "Pearson",
            grana_i18n::coloc::overlap(language),
            "M1",
            "M2",
            grana_i18n::coloc::voxels(language),
            "p",
        ];
        let series_label = grana_i18n::coloc::series(language);
        let frame_label = grana_i18n::coloc::frame_row(language, time + 1);
        let canvas = grana_ui::canvas(Length::Fill, height, move |painter, info| {
            let widths = statistic_widths(info.rect.width());
            let mut left = info.rect.min.x;
            for (index, (label, width)) in headers.iter().zip(widths.iter()).enumerate() {
                painter.text_in(
                    Rect::from_min_size(Vec2::new(left, info.rect.min.y), Vec2::new(*width, 18.0)),
                    label,
                    header_style.aligned(if index == 0 { Align::Start } else { Align::End }),
                    Align::Center,
                );
                left += width;
            }
            let line_y = info.rect.min.y + 18.0 + 4.0;
            painter.rect(
                Rect::from_min_size(
                    Vec2::new(info.rect.min.x, line_y),
                    Vec2::new(info.rect.width(), 1.0),
                ),
                Corners::ZERO,
                palette.separator,
            );
            let mut y = info.rect.min.y + 18.0 + 9.0;
            painter.text_in(
                Rect::from_min_size(Vec2::new(info.rect.min.x, y), Vec2::new(widths[0], 20.0)),
                series_label,
                caption_style,
                Align::Center,
            );
            metric_row(
                painter,
                Vec2::new(info.rect.min.x, y),
                &series_values,
                &widths,
                value_style,
            );
            y += 20.0;
            if let Some(values) = &current_values {
                painter.text_in(
                    Rect::from_min_size(Vec2::new(info.rect.min.x, y), Vec2::new(widths[0], 20.0)),
                    &frame_label,
                    caption_style,
                    Align::Center,
                );
                metric_row(
                    painter,
                    Vec2::new(info.rect.min.x, y),
                    values,
                    &widths,
                    value_style,
                );
            }
        });
        let title = view::card_title(
            ctx,
            Icon::Table,
            grana_i18n::panel::statistics(language),
            None,
        );
        cards.push(view::card(ctx, 4.0, vec![title, canvas]));
    }
    if let Some(card) = method(ctx, state) {
        cards.push(card);
    }
    cards
}

fn statistic_widths(available: f32) -> [f32; 7] {
    let base: [f32; 7] = [48.0, 50.0, 50.0, 38.0, 38.0, 46.0, 44.0];
    let sum = base.iter().sum::<f32>();
    let scale = (available / sum).max(0.85);
    base.map(|width| width * scale)
}

fn metrics_values(statistics: ColocStatistics) -> [String; 6] {
    let manders = statistics.manders();
    [
        number(statistics.pearson()),
        number(statistics.overlap()),
        number(manders[0]),
        number(manders[1]),
        statistics.voxels().to_string(),
        statistics.significance().map_or_else(
            || "\u{2014}".to_string(),
            |significance| format!("{:.4}", significance.p()),
        ),
    ]
}

fn metric_row(
    painter: &mut grana_ui::Painter<'_>,
    min: Vec2,
    values: &[String; 6],
    widths: &[f32; 7],
    style: TextStyle,
) {
    let mut left = min.x + widths[0];
    for (index, value) in values.iter().enumerate() {
        let width = widths[index + 1];
        painter.text_in(
            Rect::from_min_size(Vec2::new(left, min.y), Vec2::new(width, 20.0)),
            value,
            style,
            Align::Center,
        );
        left += width;
    }
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(|| "\u{2014}".to_string(), |value| format!("{value:.3}"))
}

fn method<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Option<Element<'a, Message>> {
    let language = ctx.language();
    let result = state.scene.coloc.result()?;
    let settings = state.scene.coloc.settings();
    let cost = result.cost();
    let level = result.level();
    let stride = result.stride();
    let frames = result.series().frames().len();
    let mask = result
        .mask()
        .and_then(ObjectSetId::from_raw)
        .and_then(|id| state.scene.objects.get(id))
        .map_or_else(
            || grana_i18n::common::whole_volume(language).to_string(),
            |set| set.name().to_string(),
        );
    let permutations = settings.permutations() as f32;
    let block = settings.block() as f32;
    let max_permutations = COLOC_PERMUTATIONS_MAX as f32;
    let max_block = COLOC_BLOCK_MAX as f32;
    let theme = *ctx.theme();
    let sample = grana_i18n::coloc::sampling_detail(
        language,
        100.0 * result.coverage(),
        cost.voxels(),
        frames,
        cost.blocks(),
        stride,
    );
    let precision = settings.precision();
    let labels = ColocPrecision::ALL.map(|value| precision_label(language, value));
    let precision_row = view::segmented(
        ctx,
        "coloc.precision",
        &labels,
        precision.index() as usize,
        move |index| match ColocPrecision::from_index(index as u8) {
            Some(next) if next != precision => {
                Message::Command(Command::SetColocSettings(settings.with_precision(next)))
            }
            _ => Message::Noop,
        },
    );
    let permutations_value = grana_ui::widget::drag_value(&theme, permutations, 0)
        .named("coloc.permutations")
        .on_drag_value(0.5, move |value| {
            let next = value.round().clamp(1.0, max_permutations) as u32;
            if next != settings.permutations() {
                Message::Command(Command::SetColocSettings(ColocSettings::new(
                    next,
                    settings.block(),
                )))
            } else {
                Message::Noop
            }
        });
    let block_value = grana_ui::widget::drag_value(&theme, block, 0)
        .named("coloc.block")
        .on_drag_value(0.1, move |value| {
            let next = value.round().clamp(1.0, max_block) as u32;
            if next != settings.block() {
                Message::Command(Command::SetColocSettings(ColocSettings::new(
                    settings.permutations(),
                    next,
                )))
            } else {
                Message::Noop
            }
        });
    let shuffles = grana_ui::row().gap(8.0).align(Align::Center).children([
        permutations_value,
        field_label(ctx, "PSF"),
        block_value,
    ]);
    let detail = format!("L{level} · s{stride} · {mask}");
    let title = view::card_title(
        ctx,
        Icon::Sparkles,
        grana_i18n::coloc::method(language),
        Some(&detail),
    );
    let sampling = view::labelled(ctx, grana_i18n::coloc::sampling(language), &sample);
    let shuffles = view::form_row(ctx, grana_i18n::coloc::shuffles(language), shuffles);
    let precision = view::form_row(ctx, grana_i18n::coloc::precision(language), precision_row);
    Some(view::card(
        ctx,
        4.0,
        vec![title, sampling, precision, shuffles],
    ))
}

fn precision_label(language: Language, precision: ColocPrecision) -> &'static str {
    match precision {
        ColocPrecision::Full => grana_i18n::coloc::precision_full(language),
        ColocPrecision::Balanced => grana_i18n::coloc::precision_balanced(language),
        ColocPrecision::Fast => grana_i18n::coloc::precision_fast(language),
    }
}

fn scatter<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let language = ctx.language();
    let Some(pair) = state.scene.coloc.pair() else {
        return grana_ui::column();
    };
    let Some(result) = state.scene.coloc.result() else {
        return grana_ui::column();
    };
    let series = result.series();
    let thresholds = result.thresholds();
    let channels = state.scene.dataset.channels();
    let tints = [channels[pair.a()].tint(), channels[pair.b()].tint()];
    let names = [channels[pair.a()].name(), channels[pair.b()].name()];
    let bins = series.bins();
    let theme = *ctx.theme();
    let subtitle = grana_i18n::coloc::all_frames_bins(language, bins);
    let background = theme.surface(Role::Field, State::Rest);
    let grid = Color::white_alpha(0.05);
    let line = Color::rgba(0.95, 0.96, 1.0, 0.55);
    let costes = series.costes();
    let footer_font = TypeStyle::new(10.0, 500.0, 13.0);
    let y_label = format!("Y: {}", names[1]);
    let x_label = format!("X: {}", names[0]);
    let y_tint = Color::from_rgb_f32(tints[1]);
    let x_tint = Color::from_rgb_f32(tints[0]);
    let canvas = grana_ui::canvas(Length::Fill, PLOT_SIDE + 18.0, move |painter, info| {
        let full = info.rect;
        let side = PLOT_SIDE.min(full.width() - 4.0).max(120.0);
        let x = full.min.x + (full.width() - side) * 0.5;
        let plot = Rect::from_min_size(Vec2::new(x, full.min.y), Vec2::splat(side));
        painter.outlined(
            plot,
            Corners::all(8.0),
            background.fill,
            background.border,
            grana_ui::Shadow::NONE,
        );
        for index in 1..4 {
            let fraction = index as f32 / 4.0;
            let x = plot.min.x + plot.width() * fraction;
            let y = plot.min.y + plot.height() * fraction;
            painter.segment(
                Vec2::new(x, plot.min.y),
                Vec2::new(x, plot.max.y),
                0.5,
                grid,
            );
            painter.segment(
                Vec2::new(plot.min.x, y),
                Vec2::new(plot.max.x, y),
                0.5,
                grid,
            );
        }
        for (rect, color) in scatter_positions(series, tints, side) {
            painter.rect(
                rect.translate(plot.min),
                Corners::ZERO,
                Color::rgb(color[0], color[1], color[2]),
            );
        }
        let vertical = plot.min.x + thresholds.a() * plot.width();
        let horizontal = plot.max.y - thresholds.b() * plot.height();
        painter.segment(
            Vec2::new(vertical, plot.min.y),
            Vec2::new(vertical, plot.max.y),
            0.8,
            line,
        );
        painter.segment(
            Vec2::new(plot.min.x, horizontal),
            Vec2::new(plot.max.x, horizontal),
            0.8,
            line,
        );
        if let Some(range) = costes {
            let low = range.low();
            let high = range.high();
            let low_x = plot.min.x + low.a() * plot.width();
            let high_x = plot.min.x + high.a() * plot.width();
            painter.segment(
                Vec2::new(low_x, plot.max.y - 6.0),
                Vec2::new(high_x, plot.max.y - 6.0),
                2.0,
                Color::rgba(1.0, 0.85, 0.45, 0.85),
            );
        }
        let footer = Rect::from_min_size(
            Vec2::new(plot.min.x, plot.max.y + 4.0),
            Vec2::new(plot.width(), 14.0),
        );
        painter.text_in(
            footer,
            &y_label,
            TextStyle::new(footer_font, y_tint),
            Align::Center,
        );
        painter.text_in(
            footer,
            &x_label,
            TextStyle::new(footer_font, x_tint).aligned(Align::End),
            Align::Center,
        );
    });
    let title = view::card_title(
        ctx,
        Icon::Grid,
        grana_i18n::coloc::joint_histogram(language),
        Some(&subtitle),
    );
    view::card(ctx, 6.0, vec![title, canvas])
}

fn scatter_positions(
    series: &ColocSeries,
    tints: [[f32; 3]; 2],
    side: f32,
) -> Vec<(Rect, [f32; 3])> {
    let bins = series.bins();
    let cells = bins.min(PLOT_CELLS);
    if cells == 0 {
        return Vec::new();
    }
    let block = bins.div_ceil(cells);
    let maximum = series.joint().iter().copied().max().unwrap_or(1).max(1);
    let ceiling = (maximum as f32 + 1.0).ln();
    let cell = side / cells as f32;
    let mut positions = Vec::new();
    for row in 0..cells {
        for column in 0..cells {
            let mut count = 0u64;
            for inner_row in 0..block {
                for inner_column in 0..block {
                    let bucket_a = column * block + inner_column;
                    let bucket_b = row * block + inner_row;
                    if bucket_a < bins && bucket_b < bins {
                        count = count.max(series.joint_at(bucket_a, bucket_b));
                    }
                }
            }
            if count == 0 {
                continue;
            }
            let weight = ((count as f32 + 1.0).ln() / ceiling).clamp(0.0, 1.0);
            let blend = (column + row) as f32 / (2.0 * (cells - 1).max(1) as f32);
            let tint =
                [0, 1, 2].map(|axis| tints[0][axis] + (tints[1][axis] - tints[0][axis]) * blend);
            let brightness = 0.22 + 0.78 * weight;
            positions.push((
                Rect::from_min_size(
                    Vec2::new(column as f32 * cell, (cells - row - 1) as f32 * cell),
                    Vec2::splat(cell + 0.4),
                ),
                [
                    tint[0] * brightness,
                    tint[1] * brightness,
                    tint[2] * brightness,
                ],
            ));
        }
    }
    positions
}

fn field_label<'a>(ctx: &mut BuildContext<'_, Message>, text: &str) -> Element<'a, Message> {
    let theme = *ctx.theme();
    grana_ui::text(
        text,
        TextStyle::new(theme.label_font(), theme.palette.text_tertiary),
    )
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
