use glam::Vec2;

use crate::color::Color;
use crate::content::{Content, IconContent};
use crate::element::{Element, Layout, Length, Size};
use crate::icon::{self, Icon};
use crate::paint::Painter;

use super::PaintInfo;

pub fn icon<'a, A>(symbol: Icon, size: f32, color: Color, weight: f32) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(size), Length::Fixed(size)),
        ..Layout::row()
    })
    .with_content(Content::Icon(IconContent {
        symbol,
        size,
        color,
        weight,
    }))
}

pub(super) fn measure(content: &IconContent) -> Vec2 {
    Vec2::splat(content.size)
}

pub(super) fn paint(content: &IconContent, painter: &mut Painter, info: &PaintInfo<'_>) {
    icon::paint(
        content.symbol,
        painter,
        info.rect.center(),
        content.size,
        content.color,
        content.weight,
    );
}
