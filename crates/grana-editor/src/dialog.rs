use glam::Vec2;
use grana_ui::{
    BuildContext, Corners, Element, Key, Layer, Length, Motion, Rect, Role, State, Surface,
};

use crate::message::Message;

const CLEARANCE: f32 = 24.0;
const SCRIM: f32 = 0.32;

pub(crate) struct Sheet {
    pub rect: Rect,
    pub surface: Surface,
}

pub(crate) fn sheet(
    ctx: &mut BuildContext<'_, Message>,
    pane: Rect,
    key: Key,
    open: bool,
    size: Vec2,
) -> Option<Sheet> {
    let spatial = ctx.animate(
        key.then("rise").id(),
        if open { 1.0 } else { 0.0 },
        Motion::spatial_default(),
    );
    if !open && spatial <= 0.005 {
        return None;
    }
    let theme = *ctx.theme();
    let travel = 0.5 * (pane.height() + size.y) + CLEARANCE;
    let center = pane.center() + Vec2::new(0.0, (1.0 - spatial) * travel);
    let surface = theme.surface(Role::Dialog, State::Rest);
    Some(Sheet {
        rect: Rect::from_center_size(center, size),
        surface,
    })
}

pub(crate) fn scrim(
    ctx: &BuildContext<'_, Message>,
    pane: Rect,
    name: &str,
    open: bool,
    layer: Layer,
    dismiss: Option<Message>,
) -> Element<'static, Message> {
    let color = ctx.theme().palette.scrim.with_alpha(SCRIM);
    let element = grana_ui::canvas(Length::Fill, pane.height())
        .view_static(move |painter, info| {
            painter.rect(info.rect, Corners::ZERO, color);
        })
        .named(name)
        .at(pane)
        .layer(layer);
    match (open, dismiss) {
        (true, Some(message)) => element.press(message),
        (true, None) => element.hit_target(),
        (false, _) => element,
    }
}
