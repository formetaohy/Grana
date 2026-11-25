use glam::Vec2;

use crate::content::{Content, LoadingContent};
use crate::element::{Element, Layout, Length, Size};
use crate::node::Runtime;
use crate::paint::Painter;
use crate::widget::PaintInfo;

mod animator;
mod shape;

pub(crate) use animator::LoadingAnimator;

pub fn loading<'a, A>(size: f32) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(size), Length::Fixed(size)),
        ..Layout::row()
    })
    .with_content(Content::Loading(LoadingContent { size }))
}

pub(super) fn paint_loading(runtime: &Runtime, painter: &mut Painter, info: &PaintInfo<'_>) {
    let size = info.rect.width().min(info.rect.height());
    if size <= 0.0 {
        return;
    }
    let center = info.rect.center();
    let animator = &runtime.loading;
    let (sin, cos) = animator.rotation().to_radians().sin_cos();
    let radius = size * 0.5;
    let points = shape::morphed(animator.morph()).map(|point| {
        let turned = Vec2::new(point.x * cos - point.y * sin, point.x * sin + point.y * cos);
        center + turned * radius
    });
    painter.fill(center, &points, info.theme.palette.primary);
}
