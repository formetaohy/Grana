use glam::{Vec2, Vec3};
use grana_display::{TransferFunction, VolumeMode};
use grana_optics::{Psf, PsfParams};
use grana_scene::{MaskFilter, RelationField};
use grana_session::Command;
use grana_ui::{
    Align, Border, BuildContext, Color, Corners, Edges, Element, Fill, Icon, Key, Length, Level,
    Motion, Rect, Role, Shadow, State, TextStyle, icon,
};
use grana_volume::{Clip, Intensity};

use crate::message::Message;
use crate::state::View;
use crate::view;

mod channel;
mod display;
mod render;
mod scene;

use channel::channels;
use display::display;
use render::{clip, rendering, slice};
use scene::{drift, mask, region};

pub(crate) fn panel_width() -> f32 {
    crate::panel::SIDEBAR_WIDTH - crate::panel::SIDEBAR_PAD * 2.0
}

pub(crate) fn card_width() -> f32 {
    panel_width() - 24.0
}

pub fn source<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let has_series = state.series.entries.len() >= 2;
    let loading = state.io.loading;
    let document = state.io.document;
    let labels = state.io.label;
    if !has_series && loading.is_none() && document.is_none() && labels.is_none() {
        return hidden();
    }
    let mut children = Vec::new();
    if has_series {
        children.push(series(ctx, state));
    }
    if let Some(name) = loading {
        children.push(progress_row(ctx, name));
    }
    if let Some((label, done, total)) = document {
        let text = done
            .checked_mul(100)
            .and_then(|percent| percent.checked_div(total))
            .map_or_else(
                || label.to_string(),
                |percent| format!("{label} · {percent}%"),
            );
        children.push(progress_row(ctx, &text));
    }
    if let Some((label, done, total)) = labels {
        let text = done
            .checked_mul(100)
            .and_then(|percent| percent.checked_div(total))
            .map_or_else(
                || label.to_string(),
                |percent| format!("{label} · {percent}%"),
            );
        children.push(progress_row(ctx, &text));
    }
    view::card(ctx, 6.0, children)
}

pub fn view_body<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let mut elements = Vec::new();
    if state.plate.is_some() {
        elements.push(plate(ctx, state));
    }
    elements.extend([
        channels(ctx, state),
        views(ctx, state),
        display(ctx, state),
        camera(ctx, state),
        slice(ctx, state),
        clip(ctx, state),
        rendering(ctx, state),
    ]);
    elements
}

fn camera<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let camera = state.camera();
    let projection = camera.projection();
    let mut children = vec![view::card_title(
        ctx,
        Icon::Cube,
        "Camera",
        Some(projection.label()),
    )];
    let modes = grana_display::CameraProjection::ALL.map(grana_display::CameraProjection::label);
    if camera.slice().is_some() {
        children.push(view::caption(
            ctx,
            "A slice view samples one plane orthographically.",
        ));
    } else {
        let selector = view::segmented_fill(
            ctx,
            "camera.projection",
            &modes,
            projection.index(),
            Message::SetCameraProjection,
        );
        children.push(view::form_row(ctx, "Projection", selector));
    }
    if projection.orthographic() {
        children.push(view::caption(
            ctx,
            "Parallel rays hold one scale across depth.",
        ));
    } else {
        let world = state.scene.dataset.world_size();
        let aperture = camera.aperture();
        let limit = (0.08 * world.length()).max(1.0e-3);
        let readout = if aperture > 0.0 {
            format!("{aperture:.2} µm")
        } else {
            "off".to_string()
        };
        children.push(view::slider_event(
            ctx,
            "camera.aperture",
            Some("Aperture"),
            aperture.clamp(0.0, limit),
            0.0..=limit,
            &readout,
            Message::SetAperture,
        ));
        let focus = camera.focus();
        let span = camera.distance();
        let range = (0.1 * span)..=(6.0 * span);
        let readout = format!("{focus:.2} µm");
        children.push(view::slider_event(
            ctx,
            "camera.focus",
            Some("Focus"),
            focus.clamp(*range.start(), *range.end()),
            range,
            &readout,
            Message::SetFocus,
        ));
        if aperture > 0.0 {
            children.push(
                view::button(ctx, "Focus Pivot", Role::Quiet)
                    .named("camera.focus_pivot")
                    .press(Message::FocusCamera),
            );
        }
    }
    view::card(ctx, 8.0, children)
}

fn views<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let count = state.views.len();
    let detail = match count {
        1 => Some("1 saved".to_string()),
        0 => None,
        count => Some(format!("{count} saved")),
    };
    let mut children = vec![view::card_title(
        ctx,
        Icon::Camera,
        "Views",
        detail.as_deref(),
    )];
    children.push(
        view::button(ctx, "Save Current View", Role::Control)
            .named("view.save")
            .press(Message::Command(Command::SaveView)),
    );
    for (index, named) in state.views.iter().enumerate() {
        let name = named.name.clone();
        let apply = view::button(ctx, &name, Role::Quiet)
            .named(&format!("view.apply.{index}"))
            .press(Message::Command(Command::ApplyView(index)));
        let remove = view::icon_button(
            ctx,
            &format!("view.remove.{index}"),
            Icon::Close,
            26.0,
            Role::Quiet,
            "Remove view",
            Message::Command(Command::RemoveView(index)),
        );
        children.push(
            grana_ui::row()
                .gap(6.0)
                .align(Align::Center)
                .children([apply, remove]),
        );
    }
    if count == 0 {
        children.push(view::caption(ctx, "Keep this camera and timeline."));
    }
    view::card(ctx, 6.0, children)
}

fn plate<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let Some(view) = state.plate else {
        return hidden();
    };
    let source = view.plate;
    let detail = format!(
        "{} × {} wells · {} fields",
        source.rows(),
        source.columns(),
        source.len()
    );
    let title = view::card_title(ctx, Icon::Grid, source.name(), Some(&detail));
    let current = view.selected_well().map_or_else(
        || "no field of view".to_string(),
        |well| {
            format!(
                "Well {} · {} field{}",
                well.label(),
                well.fields().len(),
                if well.fields().len() == 1 { "" } else { "s" }
            )
        },
    );
    let caption = view::caption(ctx, &current);
    let browse = view::button(ctx, "Browse Wells", Role::Control)
        .named("plate.browse")
        .press(Message::SetPanel(Some(crate::panel::Panel::Plate)));
    view::card(ctx, 6.0, vec![title, caption, browse])
}

pub fn objects_body<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let mut elements = vec![region(ctx, state), drift(ctx, state), mask(ctx, state)];
    elements.extend(crate::object_panel::view(ctx, state));
    elements
}

pub(crate) fn hidden<'a>() -> Element<'a, Message> {
    grana_ui::canvas(Length::Fixed(0.0), 0.0, |_painter, _info| {}).at(Rect::ZERO)
}

pub(crate) fn centered<'a>(height: f32, child: Element<'a, Message>) -> Element<'a, Message> {
    grana_ui::column()
        .width(Length::Auto)
        .height(Length::Fixed(height))
        .justify(Align::Center)
        .children([child])
}

fn series<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let unit = match state.scene.dataset.origin() {
        grana_scene::Origin::Czi => "Scene",
        _ => "Series",
    };
    let options = state
        .series
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let label = entry
                .name()
                .map_or_else(|| format!("{unit} {}", index + 1), |name| name.to_string());
            (index, label)
        })
        .collect::<Vec<_>>();
    let selected = state.series.selected;
    form_combo(
        ctx,
        "dataset.series",
        unit,
        &options,
        Some(selected),
        card_width() - crate::panel::FORM_LABEL,
        move |index| Message::Command(Command::SelectSeries(index)),
    )
}

fn progress_row<'a>(ctx: &mut BuildContext<'_, Message>, text: &str) -> Element<'a, Message> {
    let spinner = view::spinner(ctx, 12.0);
    let caption = view::caption(ctx, text);
    grana_ui::row()
        .gap(8.0)
        .align(Align::Center)
        .children([spinner, caption])
}

pub(crate) fn disclosure<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    title: &str,
    open: bool,
    children: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let font = theme.font(Level::Callout).medium();
    let token = Key::new(key);
    let arrow = ctx.animate(
        token.then("arrow").id(),
        if open { 1.0 } else { 0.0 },
        Motion::response(0.32, 0.18),
    );
    let label = title.to_string();
    let header = grana_ui::canvas(Length::Fill, 26.0, move |painter, info| {
        let rect = info.rect;
        if info.hover > 0.01 {
            painter.rect(
                rect.expand(2.0),
                Corners::all(8.0),
                palette.material_hover.amplify(0.45 * info.hover),
            );
        }
        painter.text_in(
            Rect::from_min_max(rect.min, Vec2::new(rect.max.x - 18.0, rect.max.y)),
            &label,
            TextStyle::new(
                font,
                palette
                    .text_secondary
                    .lerp(palette.text_primary, info.hover),
            ),
            Align::Center,
        );
        icon::chevron(
            painter,
            Vec2::new(rect.max.x - 6.0, rect.center().y),
            8.0,
            std::f32::consts::FRAC_PI_2 * arrow,
            palette
                .text_tertiary
                .lerp(palette.text_secondary, info.hover),
            1.5,
        );
    })
    .key(token)
    .press(Message::ToggleOpen(token))
    .focus_ring(Corners::all(8.0))
    .cursor(grana_ui::Cursor::Pointing);
    let mut items = vec![header];
    if open {
        items.push(grana_ui::column().gap(6.0).children(children));
    }
    grana_ui::column().gap(0.0).children(items)
}

pub(crate) fn combo<'a, T: Clone + PartialEq + 'static>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    width: Length,
    popup_width: f32,
    items: &[(T, String)],
    selected: Option<T>,
    select: impl Fn(T) -> Message + 'static,
) -> Element<'a, Message> {
    let labels = items
        .iter()
        .map(|(_, label)| label.clone())
        .collect::<Vec<_>>();
    let index =
        selected.and_then(|selected| items.iter().position(|(value, _)| value == &selected));
    let shown = index.map_or_else(String::new, |index| labels[index].clone());
    let values = items
        .iter()
        .map(|(value, _)| value.clone())
        .collect::<Vec<_>>();
    let rows = view::popup_rows(ctx, key, &labels, index.unwrap_or(0), move |index| {
        select(values[index].clone())
    });
    let anchor = combo_anchor(ctx, Key::new(key), shown, width);
    let menu = view::menu(ctx, Key::new(key), popup_width, rows);
    grana_ui::column().gap(0.0).children([anchor, menu])
}

fn combo_anchor<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: Key,
    value: String,
    width: Length,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let font = theme.font(Level::Callout);
    let height = theme.metrics.control_height;
    grana_ui::canvas(width, height, move |painter, info| {
        let rect = info.rect;
        let look = theme
            .surface(Role::Field, State::Rest)
            .lerp(theme.surface(Role::Field, State::Hover), info.hover);
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 9.0, rect.min.y),
                Vec2::new(rect.max.x - 22.0, rect.max.y),
            ),
            &value,
            TextStyle::new(font, look.text).truncating(),
            Align::Center,
        );
        icon::paint(
            Icon::ChevronDown,
            painter,
            Vec2::new(rect.max.x - 10.0, rect.center().y),
            9.0,
            palette.text_tertiary,
            1.5,
        );
    })
    .key(key)
    .press(Message::Noop)
    .focus_ring(Corners::all(10.0))
    .cursor(grana_ui::Cursor::Pointing)
}

pub(crate) fn form_combo<'a, T: Clone + PartialEq + 'static>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: &str,
    items: &[(T, String)],
    selected: Option<T>,
    popup_width: f32,
    select: impl Fn(T) -> Message + 'static,
) -> Element<'a, Message> {
    let content = combo(ctx, key, Length::Fill, popup_width, items, selected, select);
    view::form_row(ctx, label, content)
}
