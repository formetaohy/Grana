use crate::element::{Element, Key};
use crate::geometry::Edges;

pub fn popup<'a, A>(
    anchor: Key,
    width: f32,
    children: impl IntoIterator<Item = Element<'a, A>>,
) -> Element<'a, A> {
    Element::new(crate::element::Layout {
        flow: crate::element::Flow::Column,
        size: crate::element::Size::new(crate::element::Length::Fill, crate::element::Length::Auto),
        padding: Edges::all(6.0),
        gap: 2.0,
        ..crate::element::Layout::column()
    })
    .popup(anchor, width)
    .layer(crate::element::Layer::Popup)
    .children(children)
}

pub use crate::widget::{action_row, menu_row};
