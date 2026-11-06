use crate::element::{Element, Key};

pub fn popup<'a, A>(
    anchor: Key,
    width: f32,
    children: impl IntoIterator<Item = Element<'a, A>>,
) -> Element<'a, A> {
    Element::new(crate::element::Layout {
        flow: crate::element::Flow::Column,
        size: crate::element::Size::new(crate::element::Length::Fill, crate::element::Length::Auto),
        ..crate::element::Layout::column()
    })
    .popup(anchor, width)
    .layer(crate::element::Layer::Popup)
    .glass(crate::glass::GlassStyle::popover())
    .children(children)
}

pub use crate::widget::{action_row, menu_row, separator};
