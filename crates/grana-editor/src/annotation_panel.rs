use glam::Vec2;
use grana_scene::AnnotationTag;
use grana_session::Command;
use grana_ui::{
    Align, BuildContext, Corners, Element, Icon, Key, Length, Rect, Role, TextStyle, icon,
};

use crate::message::Message;
use crate::state::{AnnotationTool, View};
use crate::view;

const ROW_HEIGHT: f32 = 30.0;
const ROW_PAD: f32 = 2.0;

pub fn view<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let mut children = vec![tools_card(ctx, state), list_card(ctx, state)];
    if let Some(statistics) = statistics_card(ctx, state) {
        children.push(statistics);
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
    let active = state.workspace.annotation_tool;
    let title = view::card_title(
        ctx,
        Icon::Target,
        "Measure",
        Some(&format!("{} placed", state.scene.annotations.len())),
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
        Some(AnnotationTool::Point) => "Click a surface to drop a measurement point.",
        Some(AnnotationTool::Distance) => "Click two surfaces to measure their distance.",
        Some(AnnotationTool::Roi) => "Click a surface to place a statistics region.",
        None => "Choose a tool, then click the volume or the section.",
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
    let label = tool.name();
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
    let detail = format!("{} placed", scene.annotations.len());
    let title = view::card_title(ctx, Icon::Grid, "Annotations", Some(&detail));
    let mut children = vec![title];
    if scene.annotations.is_empty() {
        children.push(view::weak(
            ctx,
            "Nothing is measured yet. Points, distances and regions stay with the project and the protocol.",
        ));
        return view::card(ctx, 8.0, children);
    }
    for index in 0..scene.annotations.len() {
        children.push(row(ctx, state, index));
    }
    children.push(view::divider(ctx));
    let selected = scene.annotations.selected();
    let remove = view::button_with(ctx, "Remove", Role::Quiet, Length::Fixed(132.0))
        .named("annotation.remove.selected")
        .press(match selected {
            Some(index) => Message::Command(Command::RemoveAnnotation(index)),
            None => Message::Noop,
        });
    let clear = view::button_with(ctx, "Remove All", Role::Quiet, Length::Fixed(132.0))
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
    let annotation = state
        .scene
        .annotations
        .get(index)
        .expect("the annotation row names an annotation");
    let selected = state.scene.annotations.selected() == Some(index);
    let visible = annotation.visible();
    let scoped = state.scene.annotations.roi() == Some(index);
    let name = annotation.name().to_string();
    let kind = annotation.kind();
    let detail = match kind.tag() {
        AnnotationTag::Point => "point".to_string(),
        AnnotationTag::Distance => format!(
            "distance · {}",
            grana_figure::value(
                kind.length().unwrap_or(0.0),
                grana_measure::Unit::Micrometer,
                2
            )
        ),
        AnnotationTag::Roi => format!(
            "region · r {}",
            grana_figure::value(
                kind.radius().unwrap_or(0.0),
                grana_measure::Unit::Micrometer,
                2
            )
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
                AnnotationTag::Roi => Icon::Sphere,
            },
            painter,
            Vec2::new(rect.min.x + 14.0, rect.center().y),
            14.0,
            if scoped {
                palette.accent
            } else {
                palette.text_secondary
            },
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
                Vec2::new(rect.max.x - 160.0, rect.min.y),
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
        if visible { "Hide" } else { "Show" },
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

fn statistics_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Option<Element<'a, Message>> {
    let index = state.scene.annotations.selected()?;
    let annotation = state.scene.annotations.get(index)?;
    if annotation.kind().tag() != AnnotationTag::Roi {
        return None;
    }
    let radius = annotation
        .kind()
        .radius()
        .expect("a region keeps its radius");
    let scoped = state.scene.annotations.roi() == Some(index);
    let world = state.scene.dataset.world_size().min_element();
    let maximum = (radius * 2.0).max(world * 0.5).max(radius + 1.0e-3);
    let slider = view::slider_readout(
        ctx,
        "annotation.radius",
        Some("Radius"),
        radius,
        0.05..=maximum,
        false,
        &grana_figure::value(radius, grana_measure::Unit::Micrometer, 2),
        move |radius| Command::SetAnnotationRadius { index, radius },
    );
    let toggle = view::toggle_row(
        ctx,
        "annotation.roi",
        "Scope statistics",
        scoped,
        move |on| Command::SetAnnotationRoi(if on { Some(index) } else { None }),
    );
    let count = format!(
        "{} of {} objects in range",
        state.measure.rows().len(),
        state.measure.total()
    );
    let title = view::card_title(ctx, Icon::Filter, "Statistics Region", Some(&count));
    Some(view::card(ctx, 8.0, vec![title, slider, toggle]))
}
