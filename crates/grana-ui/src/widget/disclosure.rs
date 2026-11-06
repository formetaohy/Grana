use crate::element::{Content, DisclosureContent, Element, Layout, Length, Size};
use crate::geometry::{Corners, Rect};
use crate::node::Runtime;
use crate::paint::Painter;
use crate::text::{Align, TextStyle};
use crate::theme::Theme;

use super::PaintInfo;

pub fn disclosure<'a, A>(theme: &Theme, title: &str, open: bool) -> Element<'a, A> {
    Element::new(Layout {
        flow: crate::element::Flow::Column,
        size: Size::new(Length::Fill, Length::Auto),
        gap: 6.0,
        ..Layout::column()
    })
    .with_content(Content::Disclosure(DisclosureContent {
        title: title.to_string(),
        open,
        font: theme.font(crate::theme::Level::Callout).medium(),
    }))
    .focus_ring(Corners::all(8.0))
    .cursor(crate::input::Cursor::Pointing)
}

pub(super) fn paint_disclosure(
    disclosure: &DisclosureContent,
    runtime: &Runtime,
    painter: &mut Painter,
    info: &PaintInfo<'_>,
) {
    let rect = info.rect;
    let header = Rect::from_min_size(rect.min, glam::Vec2::new(rect.width(), 26.0));
    let palette = info.theme.palette;
    if info.hover > 0.01 {
        painter.rect(
            header.expand(2.0),
            Corners::all(8.0),
            palette.material_hover.amplify(0.45 * info.hover),
        );
    }
    let style = TextStyle::new(
        disclosure.font,
        palette
            .text_secondary
            .lerp(palette.text_primary, info.hover),
    );
    painter.text_in(
        Rect::from_min_max(
            header.min,
            glam::Vec2::new(header.max.x - 18.0, header.max.y),
        ),
        &disclosure.title,
        style,
        Align::Center,
    );
    let centre = glam::Vec2::new(header.max.x - 6.0, header.center().y);
    let tint = palette
        .text_tertiary
        .lerp(palette.text_secondary, info.hover);
    crate::icon::chevron(
        painter,
        centre,
        8.0,
        std::f32::consts::FRAC_PI_2 * runtime.value.value,
        tint,
        1.5,
    );
}
