use glam::Vec2;
use grana_i18n::Language;
use grana_scene::AnnotationTag;
use grana_session::Command;
use grana_ui::{
    Align, BuildContext, Corners, Element, Icon, Key, Length, Level, Rect, Role, TextStyle, icon,
};

use crate::message::Message;
use crate::state::{AnnotationTool, View};
use crate::view;

const ROW_HEIGHT: f32 = 34.0;
const ROW_PAD: f32 = 4.0;

pub fn view<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let authoring = state.access.author();
    let mut children = Vec::new();
    if authoring {
        children.push(tools_card(ctx, state));
    }
    children.push(list_card(ctx, state));
    if authoring {
        if let Some(annotation) = selected_card(ctx, state) {
            children.push(annotation);
        }
        if let Some(profile) = profile_card(ctx, state) {
            children.push(profile);
        }
        children.push(region_card(ctx, state));
    }
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
    let count = AnnotationTool::ALL.len();
    for (index, tool) in AnnotationTool::ALL.into_iter().enumerate() {
        row.push(tool_button(ctx, index, count, tool, active == Some(tool)));
    }
    let group_gap = ctx.theme().metrics.group_gap;
    let buttons = grana_ui::row()
        .gap(group_gap)
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
    index: usize,
    count: usize,
    tool: AnnotationTool,
    active: bool,
) -> Element<'a, Message> {
    let inner = ctx.theme().metrics.corner_sm;
    let corners = Corners::group_horizontal(index, count, Corners::CAPSULE, inner, active);
    let role = if active {
        Role::Tonal
    } else {
        Role::SurfaceHigh
    };
    view::button_shaped(
        ctx,
        tool.name(ctx.language()),
        role,
        Length::Fixed(96.0),
        corners,
    )
    .key(Key::index("annotation.tool", tool.index()))
    .press(Message::SetAnnotationTool(if active {
        None
    } else {
        Some(tool)
    }))
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
    let count = scene.annotations.len();
    let mut rows = Vec::with_capacity(count);
    for index in 0..count {
        rows.push(row(ctx, state, index, count));
    }
    let group_gap = ctx.theme().metrics.group_gap;
    children.push(grana_ui::column().gap(group_gap).children(rows));
    if !state.access.author() {
        return view::card(ctx, 8.0, children);
    }
    children.push(view::divider(ctx));
    let selected = scene.annotations.selected();
    let inner = ctx.theme().metrics.corner_sm;
    let remove = view::button_shaped(
        ctx,
        grana_i18n::common::remove(language),
        Role::SurfaceHigh,
        Length::Fixed(132.0),
        Corners::split_leading(inner),
    )
    .named("annotation.remove.selected")
    .press(match selected {
        Some(index) => Message::Command(Command::RemoveAnnotation(index)),
        None => Message::Noop,
    });
    let clear = view::button_shaped(
        ctx,
        grana_i18n::annotation::remove_all(language),
        Role::Error,
        Length::Fixed(132.0),
        Corners::split_trailing(inner),
    )
    .named("annotation.clear")
    .press(Message::Command(Command::ClearAnnotations));
    children.push(
        grana_ui::row()
            .gap(group_gap)
            .justify(Align::End)
            .width(Length::Fill)
            .children([remove, clear]),
    );
    view::card(ctx, 8.0, children)
}

fn row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    index: usize,
    count: usize,
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
        theme.font(Level::Title)
    } else {
        theme.font(Level::Body).medium()
    };
    let corners = Corners::group_vertical(
        index,
        count,
        theme.metrics.corner_md,
        theme.metrics.corner_xs,
        selected,
    );
    let body = grana_ui::canvas(Length::Fill, ROW_HEIGHT, move |painter, info| {
        let rect = info.rect;
        let hovered = info.hover.max(info.press);
        let fill = if selected {
            palette.secondary_container
        } else {
            palette
                .surface_container_high
                .lerp(palette.surface_container_highest, hovered)
        };
        painter.rect(rect, corners, fill);
        let fg = if selected {
            palette.on_secondary_container
        } else {
            palette.on_surface
        };
        icon::paint(
            match kind.tag() {
                AnnotationTag::Point => Icon::Target,
                AnnotationTag::Distance => Icon::Ruler,
            },
            painter,
            Vec2::new(rect.min.x + 14.0, rect.center().y),
            14.0,
            if selected {
                palette.on_secondary_container
            } else {
                palette.primary
            },
            1.6,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 28.0, rect.min.y),
                Vec2::new(rect.max.x - 84.0, rect.max.y),
            ),
            &name,
            TextStyle::new(font, fg).truncating(),
            Align::Center,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.max.x - 190.0, rect.min.y),
                Vec2::new(rect.max.x - 10.0, rect.max.y),
            ),
            &detail,
            TextStyle::new(
                theme.font(Level::Caption),
                if selected {
                    palette.on_secondary_container.with_alpha(0.82)
                } else {
                    palette.on_surface_variant
                },
            )
            .aligned(Align::End)
            .truncating(),
            Align::Center,
        );
    })
    .key(Key::index("annotation.row", index))
    .press(Message::Command(Command::SelectAnnotation(Some(index))))
    .focus_ring(corners);
    let eye = view::icon_button(
        ctx,
        "annotation.visible",
        if visible { Icon::Eye } else { Icon::EyeOff },
        28.0,
        if visible { Role::Tonal } else { Role::Standard },
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
        let tints = state
            .scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.tint())
            .collect::<Vec<_>>();
        let heading = grana_chart::Heading::new(
            String::new(),
            grana_i18n::annotation::profile_axis(language),
            grana_i18n::annotation::profile_offset(language),
        )
        .with_summary(grana_i18n::annotation::profile_span(
            language,
            profile.length(),
            profile.spacing(),
        ))
        .with_language(language);
        let page = grana_chart::Page::Series(Box::new(grana_chart::SeriesPage::of_profile(
            profile,
            &tints,
            heading,
            grana_chart::DistributionStyle::Box,
        )));
        let theme = *ctx.theme();
        let style = grana_chart::Style::card(&theme, grana_chart::DistributionStyle::Box)
            .with_language(language);
        let chart = grana_ui::canvas(Length::Fill, 148.0, move |painter, info| {
            page.paint(painter, info.rect, &style, &grana_chart::Decor::default());
        });
        children.push(chart);
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
        view::button_with(ctx, label, Role::SurfaceHigh, Length::Fill)
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
            Role::SurfaceHigh,
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
