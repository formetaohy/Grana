mod button;
mod disclosure;
mod field;
mod label;
mod segment;
mod slider;
mod toggle;

pub use button::{
    action_button, button, button_with, button_with_enabled, glass_action_button, glass_button,
    glass_icon_button, glass_icon_button_response, icon_button, icon_button_enabled,
    icon_button_response, text_button,
};
pub use disclosure::disclosure;
pub use field::{text_area, text_field};
pub use label::{
    badge, body, body_wrapped, caption, color_swatch, field_label, headline, labelled,
    section_title, spinner, title, weak,
};
pub use segment::{segmented, segmented_fill, tab_bar, tabs};
pub use slider::{
    SliderText, drag_value, gradient, gradient_bar, gradient_in, progress, range_slider, slider,
    slider_compact, slider_readout, slider_sized,
};
pub use toggle::{checkbox, toggle, toggle_row};

use crate::color::Color;
use crate::style::Shadow;
use crate::theme::TypeStyle;
use crate::ui::Ui;

pub(crate) fn padding(ui: &Ui) -> f32 {
    (ui.theme().metrics.control_height * 0.5).round() - 3.0
}

pub(crate) fn button_font(ui: &Ui) -> TypeStyle {
    ui.theme().button_font()
}

pub(crate) fn knob_shadow() -> Shadow {
    Shadow::new(Color::black_alpha(0.38), 9.0, 2.0)
}

pub(crate) fn pressed_inset(pressed: f32) -> f32 {
    0.8 * pressed
}
