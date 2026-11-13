use grana_chart::{Axis, Band, BandPoint, Curve, CurvePoint, SeriesChart};
use grana_display::linear_to_srgb;
use grana_i18n::Language;
use grana_object::ClusterSpec;
use grana_scene::{Job, ObjectKind, ObjectSet, ObjectSetId};
use grana_session::{
    Command, Groups, LocalizationFilter, LocalizationReport, LocalizationSpec, NeNa,
};
use grana_ui::{Align, BuildContext, Color, Corners, Element, Icon, Length, Rect, Role, Vec2};

use crate::message::Message;
use crate::state::View;
use crate::view;

const IMAGE_MINIMUM: f32 = 96.0;

const IMAGE_MAXIMUM: f32 = 260.0;

const CHART_HEIGHT: f32 = 148.0;

const GROUP_ROWS: usize = 4;

pub(super) fn card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    set: &'a ObjectSet,
) -> Element<'a, Message> {
    let language = ctx.language();
    let id = set.id();
    let frames = state.scene.frame_count();
    let spec = set
        .localization()
        .map_or_else(
            || LocalizationSpec::cover(frames),
            grana_scene::LocalizationSource::spec,
        )
        .fitted(frames);
    let projected = set
        .localization()
        .is_some_and(grana_scene::LocalizationSource::projected);
    let theme = *ctx.theme();
    let job = Job::Localizations(id);
    let running = view::progress(state.jobs, job);
    let report = state.localizations.report(id);
    let bitmap = state.localizations.bitmap(id);
    let detail = report.map_or_else(
        || grana_i18n::localization::span(language, spec.frames()[0], spec.frames()[1]),
        |report| grana_i18n::localization::summary(language, report.pixel()),
    );
    let mut children = vec![view::card_title(
        ctx,
        Icon::Target,
        grana_i18n::localization::title(language),
        Some(&detail),
    )];
    if let (Some(bitmap), Some(report)) = (bitmap, report) {
        let bitmap = bitmap.clone();
        let size = bitmap.size();
        let height = (super::width() * size[1] as f32 / size[0].max(1) as f32)
            .clamp(IMAGE_MINIMUM, IMAGE_MAXIMUM);
        children.push(
            grana_ui::canvas(Length::Fill, height).paint(move |painter, info| {
                let target = fit(info.rect, size);
                painter.rect(info.rect, Corners::all(6.0), Color::BLACK);
                painter.bitmap(target, Corners::all(4.0), &bitmap);
            }),
        );
        let depth = report.depth();
        if depth.layers() > 1 {
            let layers = depth.layers();
            let selected = state.localizations.layer(id).min(layers as usize - 1);
            let field = grana_ui::widget::drag_value(&theme, (selected + 1) as f32, 0)
                .named("localization.layer")
                .on_drag_value(0.2, move |value| {
                    let layer = (value.round() as i64 - 1).clamp(0, i64::from(layers) - 1) as usize;
                    Message::Command(Command::SetLocalizationLayer { set: id, layer })
                });
            children.push(view::form_row(
                ctx,
                grana_i18n::localization::slice(language),
                single(field),
            ));
            children.push(view::caption(
                ctx,
                &grana_i18n::localization::slice_range(
                    language,
                    depth.lower(selected as u32) * 1000.0,
                    depth.upper(selected as u32) * 1000.0,
                ),
            ));
        }
        children.push(match report.nena() {
            Some(nena) => view::labelled(
                ctx,
                grana_i18n::localization::precision(language),
                &grana_i18n::localization::nanometres(language, nena.precision()),
            ),
            None => view::caption(ctx, grana_i18n::localization::precision_missing(language)),
        });
        let resolution = report.frc().resolution().map_or_else(
            || grana_i18n::localization::resolution_missing(language).to_string(),
            |value| grana_i18n::localization::nanometres(language, value),
        );
        children.push(view::labelled(
            ctx,
            grana_i18n::localization::resolution(language),
            &resolution,
        ));
        if report.spec() != spec || report.region() != state.scene.region_scope() {
            children.push(view::caption(
                ctx,
                grana_i18n::localization::stale(language),
            ));
        }
    } else if let Some(progress) = running {
        children.push(view::caption(
            ctx,
            &view::progress_caption(language, ObjectKind::Spots, job, progress, frames),
        ));
    } else {
        let count = set
            .frames()
            .map(|(_, frame)| frame.object_count() as u64)
            .sum();
        children.push(view::weak(
            ctx,
            &grana_i18n::localization::localizations(language, count),
        ));
    }
    children.push(fields(ctx, &theme, id, spec, frames, set));
    children.push(view::toggle_row(
        ctx,
        "localization.projected",
        grana_i18n::localization::projected(language),
        projected,
        move |value| Command::SetLocalizationProjected {
            set: id,
            projected: value,
        },
    ));
    let group_colored = set
        .localization()
        .is_some_and(grana_scene::LocalizationSource::group_colored);
    children.push(view::toggle_row(
        ctx,
        "localization.group_colored",
        grana_i18n::localization::color_by_group(language),
        group_colored,
        move |value| Command::SetLocalizationGroupColored {
            set: id,
            group_colored: value,
        },
    ));
    let mut actions = Vec::new();
    if running.is_some() {
        actions.push(
            view::button_with(
                ctx,
                grana_i18n::common::cancel(language),
                Role::SurfaceHigh,
                Length::Fill,
            )
            .press(Message::Command(Command::CancelLocalizations(id))),
        );
    } else {
        actions.push(
            view::button_with(
                ctx,
                grana_i18n::localization::render(language),
                Role::Filled,
                Length::Fill,
            )
            .press(Message::Command(Command::AnalyzeLocalizations(id))),
        );
    }
    children.push(grana_ui::row().gap(8.0).children(actions));
    let mut tools = Vec::new();
    if bitmap.is_some() {
        tools.push(
            view::button_with(
                ctx,
                grana_i18n::localization::export_image(language),
                Role::SurfaceHigh,
                Length::Fill,
            )
            .press(Message::Command(Command::ExportLocalizationImage(id))),
        );
        tools.push(
            view::button_with(
                ctx,
                grana_i18n::localization::clear_image(language),
                Role::Error,
                Length::Fill,
            )
            .press(Message::Command(Command::ClearLocalizations(id))),
        );
    }
    if bitmap.is_some() && state.localizations.rendered() > 1 {
        tools.push(
            view::button_with(
                ctx,
                grana_i18n::localization::export_composite(language),
                Role::SurfaceHigh,
                Length::Fill,
            )
            .press(Message::Command(Command::ExportLocalizationComposite)),
        );
    }
    if !tools.is_empty() {
        children.push(grana_ui::column().gap(8.0).children(tools));
    }
    if let Some(report) = report {
        let groups = report.groups();
        if !groups.stats().is_empty() {
            children.push(view::divider(ctx));
            let held = grana_i18n::localization::group_count(language, groups.stats().len());
            children.push(view::section_header(
                ctx,
                grana_i18n::localization::groups(language),
                Some(&held),
            ));
            children.push(group_rows(ctx, language, groups));
        }
        if let Some(nena) = report.nena() {
            children.push(view::divider(ctx));
            let pairs = grana_i18n::localization::neighbour_pairs(language, nena.pairs());
            children.push(view::section_header(
                ctx,
                grana_i18n::localization::neighbours(language),
                Some(&pairs),
            ));
            children.push(neighbours_chart(ctx, language, nena));
        }
        children.push(view::divider(ctx));
        children.push(view::section_header(
            ctx,
            grana_i18n::localization::correlation(language),
            None,
        ));
        children.push(correlation_chart(ctx, language, report));
        if report.frc().scoped() {
            children.push(view::caption(
                ctx,
                grana_i18n::localization::region_scoped(language),
            ));
        }
    }
    children.push(view::divider(ctx));
    let molecules = set.tracks().map_or(0, |tracks| tracks.set().len());
    let detail =
        (molecules > 0).then(|| grana_i18n::localization::molecule_count(language, molecules));
    children.push(view::section_header(
        ctx,
        grana_i18n::localization::molecules(language),
        detail.as_deref(),
    ));
    let clusters = set.localization().map_or_else(
        grana_object::ClusterSpec::default,
        grana_scene::LocalizationSource::clusters,
    );
    children.push(cluster_fields(ctx, &theme, id, clusters));
    let clustering = view::progress(state.jobs, Job::Clusters(id));
    let mut cluster_actions = Vec::new();
    if clustering.is_some() {
        cluster_actions.push(
            view::button_with(
                ctx,
                grana_i18n::common::cancel(language),
                Role::SurfaceHigh,
                Length::Fill,
            )
            .press(Message::Command(Command::CancelClusters(id))),
        );
    } else {
        cluster_actions.push(
            view::button_with(
                ctx,
                grana_i18n::localization::cluster(language),
                Role::PrimaryContainer,
                Length::Fill,
            )
            .press(Message::Command(Command::ClusterLocalizations(id))),
        );
    }
    if report.is_some() && clustering.is_none() {
        cluster_actions.push(
            view::button_with(
                ctx,
                grana_i18n::localization::estimate(language),
                Role::SurfaceHigh,
                Length::Fill,
            )
            .press(Message::Command(Command::EstimateLocalizationClusters(id))),
        );
    }
    children.push(grana_ui::row().gap(8.0).children(cluster_actions));
    view::card(ctx, 8.0, children)
}

fn group_rows<'a>(
    ctx: &mut BuildContext<'_, Message>,
    language: Language,
    groups: &'a Groups,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let rows = groups
        .stats()
        .iter()
        .take(GROUP_ROWS)
        .map(|stat| {
            let name = match stat.group() {
                Some(group) => grana_i18n::localization::group(language, group),
                None => grana_i18n::localization::ungrouped(language).to_string(),
            };
            let detail = grana_i18n::localization::group_detail(
                language,
                stat.localizations(),
                stat.precision(),
            );
            grana_ui::row().gap(8.0).align(Align::Center).children([
                grana_ui::widget::color_swatch(stat.tint().map(linear_to_srgb), 10.0),
                grana_ui::widget::labelled(&theme, &name, &detail),
            ])
        })
        .collect::<Vec<_>>();
    grana_ui::column().gap(2.0).children(rows)
}

fn cluster_fields<'a>(
    ctx: &mut BuildContext<'_, Message>,
    theme: &grana_ui::Theme,
    id: ObjectSetId,
    clusters: grana_object::ClusterSpec,
) -> Element<'a, Message> {
    let language = ctx.language();
    let radius = grana_ui::widget::drag_value(theme, clusters.radius() * 1000.0, 0)
        .named("cluster.radius")
        .on_drag_value(0.2, move |value| {
            let radius = (value.clamp(1.0, 10_000.0) * 0.001)
                .clamp(ClusterSpec::RADIUS_MINIMUM, ClusterSpec::RADIUS_MAXIMUM);
            Message::Command(Command::SetLocalizationClusters {
                set: id,
                clusters: clusters.at_radius(radius),
            })
        });
    let dark = grana_ui::widget::drag_value(theme, clusters.dark() as f32, 0)
        .named("cluster.dark")
        .on_drag_value(0.2, move |value| {
            let dark = value.round().clamp(0.0, ClusterSpec::DARK_MAXIMUM as f32) as u32;
            Message::Command(Command::SetLocalizationClusters {
                set: id,
                clusters: clusters.at_dark(dark),
            })
        });
    let minimum = grana_ui::widget::drag_value(theme, clusters.minimum() as f32, 0)
        .named("cluster.minimum")
        .on_drag_value(0.2, move |value| {
            let minimum = value
                .round()
                .clamp(1.0, ClusterSpec::MINIMUM_MAXIMUM as f32) as u32;
            Message::Command(Command::SetLocalizationClusters {
                set: id,
                clusters: clusters.at_minimum(minimum),
            })
        });
    grana_ui::column().gap(6.0).children([
        view::form_row(
            ctx,
            grana_i18n::localization::radius(language),
            single(radius),
        ),
        view::form_row(ctx, grana_i18n::localization::dark(language), single(dark)),
        view::form_row(
            ctx,
            grana_i18n::localization::minimum(language),
            single(minimum),
        ),
    ])
}

fn single<'a>(field: Element<'a, Message>) -> Element<'a, Message> {
    grana_ui::row()
        .gap(6.0)
        .align(Align::Center)
        .children([field])
}

fn fields<'a>(
    ctx: &mut BuildContext<'_, Message>,
    theme: &grana_ui::Theme,
    id: ObjectSetId,
    spec: LocalizationSpec,
    frames: u32,
    set: &'a ObjectSet,
) -> Element<'a, Message> {
    let language = ctx.language();
    let last = frames.saturating_sub(1);
    let [start, end] = spec.frames();
    let start_field = grana_ui::widget::drag_value(theme, start as f32, 0)
        .named("localization.start")
        .on_drag_value(0.2, move |value| {
            let start = value.round().clamp(0.0, last as f32) as u32;
            Message::Command(Command::SetLocalizationSpec {
                set: id,
                spec: spec.placed([start.min(end), end]),
            })
        });
    let end_field = grana_ui::widget::drag_value(theme, end as f32, 0)
        .named("localization.end")
        .on_drag_value(0.2, move |value| {
            let end = value.round().clamp(0.0, last as f32) as u32;
            Message::Command(Command::SetLocalizationSpec {
                set: id,
                spec: spec.placed([start, end.max(start)]),
            })
        });
    let span_row = grana_ui::row()
        .gap(6.0)
        .align(Align::Center)
        .children([start_field, end_field]);
    let pixel_field = grana_ui::widget::drag_value(theme, spec.pixel(), 1)
        .named("localization.pixel")
        .on_drag_value(0.1, move |value| {
            let pixel = value.clamp(
                LocalizationSpec::PIXEL_MINIMUM,
                LocalizationSpec::PIXEL_MAXIMUM,
            );
            Message::Command(Command::SetLocalizationSpec {
                set: id,
                spec: spec.at_pixel(pixel),
            })
        });
    let blur_field = grana_ui::widget::drag_value(theme, spec.blur(), 1)
        .named("localization.blur")
        .on_drag_value(0.2, move |value| {
            let blur = value.clamp(0.0, LocalizationSpec::BLUR_MAXIMUM);
            Message::Command(Command::SetLocalizationSpec {
                set: id,
                spec: spec.at_blur(blur),
            })
        });
    let filter = spec.filter();
    let precision_field = grana_ui::widget::drag_value(theme, filter.precision(), 1)
        .named("localization.precision_limit")
        .on_drag_value(0.5, move |value| {
            let precision = value.clamp(0.0, LocalizationFilter::PRECISION_MAXIMUM);
            Message::Command(Command::SetLocalizationSpec {
                set: id,
                spec: spec.at_filter(filter.at_precision(precision)),
            })
        });
    let pixel_row = grana_ui::row()
        .gap(6.0)
        .align(Align::Center)
        .children([pixel_field]);
    let blur_row = grana_ui::row()
        .gap(6.0)
        .align(Align::Center)
        .children([blur_field]);
    let depth_field = grana_ui::widget::drag_value(theme, spec.depth(), 0)
        .named("localization.depth")
        .on_drag_value(1.0, move |value| {
            let depth = value.clamp(0.0, LocalizationSpec::DEPTH_MAXIMUM);
            Message::Command(Command::SetLocalizationSpec {
                set: id,
                spec: spec.at_depth(depth),
            })
        });
    let depth_row = grana_ui::row()
        .gap(6.0)
        .align(Align::Center)
        .children([depth_field]);
    let precision_row = grana_ui::row()
        .gap(6.0)
        .align(Align::Center)
        .children([precision_field]);
    let mut rows = vec![
        view::form_row(ctx, grana_i18n::common::frames(language), span_row),
        view::form_row(
            ctx,
            grana_i18n::localization::pixel_size(language),
            pixel_row,
        ),
        view::form_row(
            ctx,
            grana_i18n::localization::blur_width(language),
            blur_row,
        ),
        view::form_row(
            ctx,
            grana_i18n::localization::slice_depth(language),
            depth_row,
        ),
        view::form_row(
            ctx,
            grana_i18n::localization::precision_limit(language),
            precision_row,
        ),
    ];
    if spec.depth() == 0.0 {
        rows.push(view::caption(
            ctx,
            grana_i18n::localization::slice_projection(language),
        ));
    }
    if set
        .localization()
        .is_some_and(grana_scene::LocalizationSource::photons)
    {
        let photons_field = grana_ui::widget::drag_value(theme, filter.photons(), 0)
            .named("localization.photons")
            .on_drag_value(5.0, move |value| {
                let photons = value.clamp(0.0, LocalizationFilter::PHOTONS_MAXIMUM);
                Message::Command(Command::SetLocalizationSpec {
                    set: id,
                    spec: spec.at_filter(filter.at_photons(photons)),
                })
            });
        let photons_row = grana_ui::row()
            .gap(6.0)
            .align(Align::Center)
            .children([photons_field]);
        rows.push(view::form_row(
            ctx,
            grana_i18n::localization::photons(language),
            photons_row,
        ));
    }
    grana_ui::column().gap(6.0).children(rows)
}

fn neighbours_chart<'a>(
    ctx: &mut BuildContext<'_, Message>,
    language: Language,
    nena: &'a NeNa,
) -> Element<'a, Message> {
    let high = nena.bins().last().copied().unwrap_or(1.0).max(1.0);
    let peak = nena
        .counts()
        .iter()
        .chain(nena.fit())
        .copied()
        .fold(0.0f32, f32::max)
        .max(f32::MIN_POSITIVE);
    let chart = SeriesChart {
        x: Axis::new(0.0, high, 5),
        y: Axis::new(0.0, peak, 4),
        curves: vec![
            Curve::new(
                grana_i18n::localization::curve_data(language),
                points(nena.bins(), nena.counts()),
            ),
            Curve::new(
                grana_i18n::localization::curve_fit(language),
                points(nena.bins(), nena.fit()),
            ),
        ],
        bands: Vec::new(),
        notes: Vec::new(),
        outside: Vec::new(),
    };
    chart_canvas(ctx, language, chart)
}

fn correlation_chart<'a>(
    ctx: &mut BuildContext<'_, Message>,
    language: Language,
    report: &'a LocalizationReport,
) -> Element<'a, Message> {
    let frc = report.frc();
    let frequencies = frc.frequencies();
    let high = frequencies
        .last()
        .copied()
        .unwrap_or(1.0)
        .max(f32::MIN_POSITIVE);
    let threshold = Band::new(
        grana_i18n::localization::threshold(language),
        vec![
            BandPoint {
                x: 0.0,
                lower: grana_session::Frc::THRESHOLD,
                upper: grana_session::Frc::THRESHOLD,
            },
            BandPoint {
                x: high,
                lower: grana_session::Frc::THRESHOLD,
                upper: grana_session::Frc::THRESHOLD,
            },
        ],
    );
    let outside = frc
        .resolution()
        .filter(|value| *value > 0.0)
        .map(|value| vec![(1.0 / value, grana_session::Frc::THRESHOLD)])
        .unwrap_or_default();
    let chart = SeriesChart {
        x: Axis::new(0.0, high, 3),
        y: Axis::new(-0.2, 1.0, 3),
        curves: vec![
            Curve::new(
                grana_i18n::localization::curve_raw(language),
                points(frequencies, frc.curve()),
            )
            .with_tint([0.62, 0.62, 0.66]),
            Curve::new(
                grana_i18n::localization::curve_smooth(language),
                points(frequencies, frc.smooth()),
            ),
        ],
        bands: vec![threshold],
        notes: Vec::new(),
        outside,
    };
    chart_canvas(ctx, language, chart)
}

fn points(values: &[f32], held: &[f32]) -> Vec<CurvePoint> {
    values
        .iter()
        .zip(held)
        .map(|(x, value)| CurvePoint {
            x: *x,
            value: *value,
            lower: *value,
            upper: *value,
        })
        .collect()
}

fn chart_canvas<'a>(
    ctx: &mut BuildContext<'_, Message>,
    language: Language,
    chart: SeriesChart,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let style = grana_chart::Style::card(&theme, grana_chart::DistributionStyle::Box)
        .with_language(language);
    grana_ui::canvas(Length::Fill, CHART_HEIGHT).paint(move |painter, info| {
        grana_chart::series(
            painter,
            info.rect,
            &chart,
            &style,
            &grana_chart::Decor::default(),
            None,
        );
    })
}

fn fit(rect: Rect, size: [u32; 2]) -> Rect {
    let scale = (rect.width() / size[0].max(1) as f32)
        .min(rect.height() / size[1].max(1) as f32)
        .max(0.0);
    let points = Vec2::new(size[0] as f32 * scale, size[1] as f32 * scale);
    Rect::from_center_size(rect.center(), points)
}
