use glam::Vec2;
use grana_i18n::Language;
use grana_scene::AnnotationTag;
use grana_session::Command;
use grana_ui::{
    Align, BuildContext, Color, Corners, Element, Icon, Key, Length, Rect, Role, TextStyle, icon,
};

use crate::message::Message;
use crate::state::{AnnotationTool, View};
use crate::view;

const ROW_HEIGHT: f32 = 30.0;
const ROW_PAD: f32 = 2.0;

pub fn view<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let mut children = vec![tools_card(ctx, state), list_card(ctx, state)];
    if let Some(annotation) = selected_card(ctx, state) {
        children.push(annotation);
    }
    if let Some(profile) = profile_card(ctx, state) {
        children.push(profile);
    }
    children.push(region_card(ctx, state));
    grana_ui::column()
        .gap(8.0)
        .width(Length::Fill)
        .children(children)
}

fn tools_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let language = ctx.language();
    let active = state.workspace.annotation_tool;
    let placed = grana_i18n::annotation::placed(language, state.scene.annotations.len());
    let title = view::card_title(
        ctx,
        Icon::Target,
        grana_i18n::annotation::measure(language),
        Some(&placed),
    );
    let mut row = Vec::new();
    for tool in AnnotationTool::ALL {
        row.push(tool_button(ctx, tool, active == Some(tool)));
    }
    let buttons = grana_ui::row()
        .gap(6.0)
        .align(Align::Center)
        .width(Length::Fill)
        .children(row);
    let detail = match active {
        Some(AnnotationTool::Point) => grana_i18n::annotation::click_point(language),
        Some(AnnotationTool::Distance) => grana_i18n::annotation::click_distance(language),
        Some(AnnotationTool::Roi) => grana_i18n::annotation::click_roi(language),
        None => grana_i18n::annotation::choose_tool(language),
    };
    let caption = view::weak(ctx, detail);
    view::card(ctx, 8.0, vec![title, buttons, caption])
}

fn tool_button<'a>(
    ctx: &mut BuildContext<'_, Message>,
    tool: AnnotationTool,
    active: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let font = theme.button_font();
    let label = tool.name(ctx.language());
    grana_ui::canvas(Length::Fixed(84.0), 30.0, move |painter, info| {
        let rect = info.rect;
        let engaged = info.hover.max(info.press);
        if active {
            painter.outlined(
                rect,
                Corners::all(8.0),
                grana_ui::Fill::Solid(palette.accent.with_alpha(0.24)),
                grana_ui::Border {
                    width: 1.0,
                    color: palette.accent.with_alpha(0.62),
                },
                grana_ui::Shadow::NONE,
            );
        } else {
            painter.rect(
                rect,
                Corners::all(8.0),
                palette.material_fill.amplify(0.55 + 0.45 * engaged),
            );
        }
        let color = if active {
            palette.text_primary
        } else {
            palette.text_secondary.lerp(palette.text_primary, engaged)
        };
        painter.text_in(
            rect,
            label,
            TextStyle::new(font, color).aligned(Align::Center),
            Align::Center,
        );
    })
    .key(Key::index("annotation.tool", tool.index()))
    .press(Message::SetAnnotationTool(if active {
        None
    } else {
        Some(tool)
    }))
    .focus_ring(Corners::all(8.0))
}

fn list_card<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let scene = state.scene;
    let language = ctx.language();
    let detail = grana_i18n::annotation::placed(language, scene.annotations.len());
    let title = view::card_title(
        ctx,
        Icon::Grid,
        grana_i18n::annotation::annotations(language),
        Some(&detail),
    );
    let mut children = vec![title];
    if scene.annotations.is_empty() {
        children.push(view::weak(
            ctx,
            grana_i18n::annotation::nothing_measured(language),
        ));
        return view::card(ctx, 8.0, children);
    }
    for index in 0..scene.annotations.len() {
        children.push(row(ctx, state, index));
    }
    children.push(view::divider(ctx));
    let selected = scene.annotations.selected();
    let remove = view::button_with(
        ctx,
        grana_i18n::common::remove(language),
        Role::Quiet,
        Length::Fixed(132.0),
    )
    .named("annotation.remove.selected")
    .press(match selected {
        Some(index) => Message::Command(Command::RemoveAnnotation(index)),
        None => Message::Noop,
    });
    let clear = view::button_with(
        ctx,
        grana_i18n::annotation::remove_all(language),
        Role::Quiet,
        Length::Fixed(132.0),
    )
    .named("annotation.clear")
    .press(Message::Command(Command::ClearAnnotations));
    children.push(
        grana_ui::row()
            .gap(6.0)
            .justify(Align::End)
            .width(Length::Fill)
            .children([remove, clear]),
    );
    view::card(ctx, 6.0, children)
}

fn row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let annotation = state
        .scene
        .annotations
        .get(index)
        .expect("the annotation row names an annotation");
    let selected = state.scene.annotations.selected() == Some(index);
    let visible = annotation.visible();
    let name = annotation.name().to_string();
    let kind = annotation.kind();
    let placement = scope(state, language, annotation.frame());
    let detail = match kind.tag() {
        AnnotationTag::Point => grana_i18n::annotation::point_detail(language, &placement),
        AnnotationTag::Distance => grana_i18n::annotation::distance_detail(
            language,
            &grana_figure::value(
                kind.length().unwrap_or(0.0),
                grana_measure::Unit::Micrometer,
                2,
            ),
            &placement,
        ),
    };
    let font = if selected {
        theme.font(grana_ui::Level::Body).medium()
    } else {
        theme.font(grana_ui::Level::Body)
    };
    let body = grana_ui::canvas(Length::Fill, ROW_HEIGHT, move |painter, info| {
        let rect = info.rect;
        let hovered = info.hover.max(info.press);
        if selected {
            painter.outlined(
                rect,
                Corners::all(7.0),
                grana_ui::Fill::Solid(palette.accent.with_alpha(0.16)),
                grana_ui::Border {
                    width: 1.0,
                    color: palette.accent.with_alpha(0.34),
                },
                grana_ui::Shadow::NONE,
            );
        } else if hovered > 0.01 {
            painter.rect(
                rect,
                Corners::all(7.0),
                palette.material_hover.amplify(0.5 * hovered),
            );
        }
        icon::paint(
            match kind.tag() {
                AnnotationTag::Point => Icon::Target,
                AnnotationTag::Distance => Icon::Ruler,
            },
            painter,
            Vec2::new(rect.min.x + 14.0, rect.center().y),
            14.0,
            palette.text_secondary,
            1.5,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 28.0, rect.min.y),
                Vec2::new(rect.max.x - 74.0, rect.max.y),
            ),
            &name,
            TextStyle::new(font, palette.text_primary).truncating(),
            Align::Center,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.max.x - 190.0, rect.min.y),
                Vec2::new(rect.max.x - 4.0, rect.max.y),
            ),
            &detail,
            TextStyle::new(theme.font(grana_ui::Level::Caption), palette.text_tertiary)
                .aligned(Align::End)
                .truncating(),
            Align::Center,
        );
    })
    .key(Key::index("annotation.row", index))
    .press(Message::Command(Command::SelectAnnotation(Some(index))))
    .focus_ring(Corners::all(7.0));
    let eye = view::icon_button(
        ctx,
        "annotation.visible",
        if visible { Icon::Eye } else { Icon::EyeOff },
        26.0,
        Role::Quiet,
        if visible {
            grana_i18n::annotation::hide(language)
        } else {
            grana_i18n::annotation::show(language)
        },
        Message::Command(Command::SetAnnotationVisible {
            index,
            visible: !visible,
        }),
    )
    .key(Key::index("annotation.visible", index));
    grana_ui::row()
        .gap(ROW_PAD)
        .align(Align::Center)
        .width(Length::Fill)
        .children([eye, body])
}

fn scope(state: &View<'_>, language: Language, frame: Option<u32>) -> String {
    if state.scene.frame_count() <= 1 {
        return grana_i18n::annotation::single_frame_scope(language).to_string();
    }
    match frame {
        Some(frame) => {
            grana_i18n::annotation::frame_scope(language, frame + 1, state.scene.frame_count())
        }
        None => grana_i18n::annotation::every_frame_scope(language).to_string(),
    }
}

fn selected_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Option<Element<'a, Message>> {
    let index = state.scene.annotations.selected()?;
    let annotation = state.scene.annotations.get(index)?;
    let language = ctx.language();
    let time = state.scene.time();
    let every = annotation.frame().is_none();
    let mut children = Vec::new();
    let detail = if state.scene.frame_count() <= 1 {
        grana_i18n::annotation::single_frame_detail(language).to_string()
    } else {
        match annotation.frame() {
            Some(frame) => {
                grana_i18n::annotation::placed_frame(language, frame + 1, state.scene.frame_count())
            }
            None => grana_i18n::annotation::shown_every_frame(language).to_string(),
        }
    };
    let title = view::card_title(
        ctx,
        Icon::Clock,
        grana_i18n::common::time(language),
        Some(annotation.name()),
    );
    children.push(title);
    children.push(view::weak(ctx, &detail));
    if state.scene.frame_count() > 1 {
        children.push(view::toggle_row(
            ctx,
            "annotation.every_frame",
            grana_i18n::annotation::on_every_frame(language),
            every,
            move |every| Command::SetAnnotationFrame {
                index,
                frame: (!every).then_some(time),
            },
        ));
    }
    Some(view::card(ctx, 8.0, children))
}

fn profile_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Option<Element<'a, Message>> {
    let index = state.scene.annotations.selected()?;
    let annotation = state.scene.annotations.get(index)?;
    if annotation.kind().tag() != AnnotationTag::Distance {
        return None;
    }
    let language = ctx.language();
    let profile = annotation.profile();
    let detail = profile.map(|profile| {
        grana_i18n::annotation::profile_detail(
            language,
            profile.samples().len(),
            profile.names().len(),
        )
    });
    let title = view::card_title(
        ctx,
        Icon::Channel,
        grana_i18n::annotation::intensity_profile(language),
        detail.as_deref(),
    );
    let mut children = vec![title];
    if let Some(profile) = profile {
        let names = profile.names().to_vec();
        let tints = state
            .scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.tint())
            .collect::<Vec<_>>();
        let samples = profile
            .samples()
            .iter()
            .map(|sample| (sample.offset(), sample.values().to_vec()))
            .collect::<Vec<_>>();
        let span = profile.length().max(1.0e-6);
        let spacing = profile.spacing();
        let legend = names.join(" · ");
        let theme = *ctx.theme();
        let chart = grana_ui::canvas(Length::Fill, 148.0, move |painter, info| {
            let palette = theme.palette;
            let rect = info.rect.shrink2(Vec2::new(4.0, 4.0));
            painter.rect(rect, Corners::all(8.0), palette.material_fill.amplify(0.4));
            let plot = Rect::from_min_max(
                Vec2::new(rect.left() + 10.0, rect.top() + 10.0),
                Vec2::new(rect.right() - 10.0, rect.bottom() - 22.0),
            );
            painter.segment(
                Vec2::new(plot.left(), plot.bottom()),
                Vec2::new(plot.right(), plot.bottom()),
                1.0,
                palette.separator,
            );
            let stride = (samples.len() / 256).max(1);
            for (channel, _) in names.iter().enumerate() {
                let mut low = f32::INFINITY;
                let mut high = f32::NEG_INFINITY;
                for sample in samples.iter().step_by(stride) {
                    if let Some(value) = sample.1.get(channel).copied()
                        && value.is_finite()
                    {
                        low = low.min(value);
                        high = high.max(value);
                    }
                }
                if low > high {
                    continue;
                }
                let range = (high - low).max(1.0e-6);
                let tint = tints.get(channel).copied().unwrap_or([1.0, 1.0, 1.0]);
                let color = Color::from_rgb_f32(tint);
                let mut previous: Option<Vec2> = None;
                for sample in samples.iter().step_by(stride) {
                    let Some(value) = sample.1.get(channel).copied() else {
                        previous = None;
                        continue;
                    };
                    if !value.is_finite() {
                        previous = None;
                        continue;
                    }
                    let x = plot.left() + (sample.0 / span).clamp(0.0, 1.0) * plot.width();
                    let y = plot.bottom() - ((value - low) / range) * plot.height();
                    let point = Vec2::new(x, y);
                    if let Some(previous) = previous {
                        painter.segment(previous, point, 1.4, color);
                    }
                    previous = Some(point);
                }
            }
            let caption = grana_i18n::annotation::profile_span(language, span, spacing);
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(rect.left() + 10.0, plot.bottom() + 4.0),
                    Vec2::new(rect.right() - 10.0, rect.bottom() - 2.0),
                ),
                &caption,
                TextStyle::new(theme.font(grana_ui::Level::Caption), palette.text_tertiary),
                Align::Center,
            );
        });
        children.push(chart);
        children.push(view::weak(ctx, &legend));
    } else {
        children.push(view::weak(
            ctx,
            grana_i18n::annotation::empty_profile(language),
        ));
    }
    let label = if profile.is_some() {
        grana_i18n::annotation::resample(language)
    } else {
        grana_i18n::annotation::sample_profile(language)
    };
    children.push(
        view::button_with(ctx, label, Role::Quiet, Length::Fill)
            .named("annotation.profile.sample")
            .press(Message::Command(Command::SampleProfile { index })),
    );
    Some(view::card(ctx, 8.0, children))
}

fn region_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let region = state.scene.region;
    let language = ctx.language();
    let enabled = region.enabled();
    let size = region.size();
    let detail = if enabled {
        format!(
            "{} × {} × {} µm",
            size.x.round(),
            size.y.round(),
            size.z.round()
        )
    } else {
        grana_i18n::annotation::whole_volume(language).to_string()
    };
    let title = view::card_title(
        ctx,
        Icon::Filter,
        grana_i18n::annotation::region_of_interest(language),
        Some(detail.as_str()),
    );
    let current = region;
    let mut children = vec![title];
    children.push(view::weak(
        ctx,
        grana_i18n::annotation::region_detail(language),
    ));
    children.push(view::toggle_row(
        ctx,
        "annotation.region",
        grana_i18n::annotation::restrict_region(language),
        enabled,
        move |on| {
            let mut region = current;
            region.set_enabled(on);
            Command::SetRegion(region)
        },
    ));
    if enabled {
        let half = 0.5 * state.scene.dataset.world_size();
        let whole = grana_volume::Region::full(-half, half);
        let clear = view::button_with(
            ctx,
            grana_i18n::common::clear(language),
            Role::Quiet,
            Length::Fixed(132.0),
        )
        .named("annotation.region.clear")
        .press(Message::Command(Command::SetRegion(whole)));
        children.push(
            grana_ui::row()
                .justify(Align::End)
                .width(Length::Fill)
                .children([clear]),
        );
    }
    view::card(ctx, 8.0, children)
}
