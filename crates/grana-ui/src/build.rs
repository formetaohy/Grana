use std::ops::RangeInclusive;

use crate::color::Color;
use crate::element::{Element, Length, text};
use crate::geometry::Edges;
use crate::icon::Icon;
use crate::style::Surface;
use crate::text::TextStyle;
use crate::theme::{Role, Theme, TypeStyle};
use crate::ui::BuildContext;
use crate::widget;

impl<'a, A> BuildContext<'a, A> {
    fn copy(&self) -> Theme {
        *self.theme()
    }

    pub fn text<'e>(&self, value: impl Into<String>, style: TextStyle) -> Element<'e, A> {
        text(value, style)
    }

    pub fn icon<'e>(&self, symbol: Icon, size: f32, color: Color, weight: f32) -> Element<'e, A> {
        widget::icon(symbol, size, color, weight)
    }

    pub fn surface<'e>(&self, surface: Surface) -> Element<'e, A> {
        Element::column().background(surface)
    }

    pub fn button<'e>(&self, label: &str, role: Role) -> Element<'e, A> {
        widget::button(&self.copy(), label, role)
    }

    pub fn button_width<'e>(&self, label: &str, role: Role, width: Length) -> Element<'e, A> {
        widget::button_width(&self.copy(), label, role, width)
    }

    pub fn button_enabled<'e>(
        &self,
        label: &str,
        role: Role,
        width: Length,
        enabled: bool,
    ) -> Element<'e, A> {
        widget::button_enabled(&self.copy(), label, role, width, enabled)
    }

    pub fn action_button<'e>(&self, symbol: Icon, label: &str, role: Role) -> Element<'e, A> {
        widget::action_button(&self.copy(), symbol, label, role)
    }

    pub fn icon_button<'e>(
        &self,
        symbol: Icon,
        size: f32,
        role: Role,
        tooltip: &str,
    ) -> Element<'e, A> {
        widget::icon_button(symbol, size, role, tooltip)
    }

    pub fn split_button<'e>(&self, spec: widget::Split<'_, A>) -> Element<'e, A>
    where
        A: Clone,
    {
        widget::split_button(&self.copy(), spec)
    }

    pub fn field<'e>(
        &self,
        value: &str,
        style: TextStyle,
        width: f32,
        multiline: bool,
    ) -> Element<'e, A> {
        widget::text_field(value, style, width, multiline)
    }

    pub fn slider<'e>(
        &self,
        label: Option<&str>,
        value: f32,
        range: RangeInclusive<f32>,
        logarithmic: bool,
        readout: &str,
    ) -> Element<'e, A> {
        widget::slider_readout(&self.copy(), label, value, range, logarithmic, readout)
    }

    pub fn slider_sized<'e>(
        &self,
        label: Option<&str>,
        value: f32,
        range: RangeInclusive<f32>,
        logarithmic: bool,
        width: Length,
    ) -> Element<'e, A> {
        widget::slider_sized(&self.copy(), label, value, range, logarithmic, width)
    }

    pub fn range_slider<'e>(
        &self,
        start: f32,
        end: f32,
        range: RangeInclusive<f32>,
        gradient: &[Color],
    ) -> Element<'e, A> {
        widget::range_slider(start, end, range, gradient)
    }

    pub fn drag_value<'e>(&self, value: f32, decimals: usize) -> Element<'e, A> {
        widget::drag_value(&self.copy(), value, decimals)
    }

    pub fn progress<'e>(&self, fraction: f32, width: f32) -> Element<'e, A> {
        widget::progress(fraction, width)
    }

    pub fn gradient_bar<'e>(&self, colors: &[Color], height: f32) -> Element<'e, A> {
        widget::gradient_bar(colors, height)
    }

    pub fn toggle<'e>(&self, value: bool) -> Element<'e, A> {
        widget::toggle(&self.copy(), value)
    }

    pub fn toggle_row<'e>(&self, label: &str, value: bool) -> Element<'e, A> {
        widget::toggle_row(&self.copy(), label, value)
    }

    pub fn checkbox<'e>(&self, label: &str, value: bool) -> Element<'e, A> {
        widget::checkbox(&self.copy(), label, value)
    }

    pub fn segmented<'e>(&self, options: &[&str], selected: usize) -> Element<'e, A> {
        widget::segmented(&self.copy(), &labels(options), selected)
    }

    pub fn segmented_fill<'e>(&self, options: &[&str], selected: usize) -> Element<'e, A> {
        widget::segmented_fill(&self.copy(), &labels(options), selected)
    }

    pub fn tabs<'e>(&self, options: &[&str], selected: usize) -> Element<'e, A> {
        widget::tabs(&self.copy(), &labels(options), selected)
    }

    pub fn caption<'e>(&self, text: &str) -> Element<'e, A> {
        widget::caption(&self.copy(), text)
    }

    pub fn weak<'e>(&self, text: &str) -> Element<'e, A> {
        widget::weak(&self.copy(), text)
    }

    pub fn badge<'e>(&self, label: &str, tint: Color) -> Element<'e, A> {
        widget::badge(label, tint)
    }

    pub fn labelled<'e>(&self, label: &str, value: &str) -> Element<'e, A> {
        widget::labelled(&self.copy(), label, value)
    }

    pub fn spinner<'e>(&self, size: f32) -> Element<'e, A> {
        widget::spinner(size)
    }

    pub fn color_swatch<'e>(&self, color: [f32; 3], size: f32) -> Element<'e, A> {
        widget::color_swatch(color, size)
    }

    pub fn color_picker<'e>(&self, color: [f32; 3]) -> Element<'e, A> {
        widget::color_picker(color)
    }

    pub fn label_style(&self) -> TextStyle {
        TextStyle::new(
            self.copy().label_font(),
            self.copy().palette.on_surface_variant,
        )
    }

    pub fn font(&self, level: crate::theme::Level) -> TypeStyle {
        self.copy().font(level)
    }

    pub fn action_row<'e>(
        &self,
        symbol: Icon,
        label: &str,
        shortcut: Option<&str>,
    ) -> Element<'e, A> {
        widget::action_row(&self.copy(), symbol, label, shortcut)
    }

    pub fn disclosure_row<'e>(
        &self,
        symbol: Icon,
        label: &str,
        detail: &str,
        open: bool,
        angle: f32,
    ) -> Element<'e, A> {
        widget::disclosure_row(&self.copy(), symbol, label, detail, open, angle)
    }

    pub fn menu_row<'e>(&self, label: &str, checked: bool) -> Element<'e, A> {
        widget::menu_row(&self.copy(), label, checked)
    }

    pub fn card<'e>(
        &self,
        role: Role,
        padding: Edges,
        gap: f32,
        children: impl IntoIterator<Item = Element<'e, A>>,
    ) -> Element<'e, A> {
        widget::card(&self.copy(), role, padding, gap, children)
    }

    pub fn group_card<'e>(
        &self,
        slot: (usize, usize),
        active: bool,
        padding: Edges,
        gap: f32,
        children: impl IntoIterator<Item = Element<'e, A>>,
    ) -> Element<'e, A> {
        widget::group_card(&self.copy(), slot, active, padding, gap, children)
    }

    pub fn card_title<'e>(
        &self,
        symbol: Icon,
        title: &str,
        detail: Option<&str>,
    ) -> Element<'e, A> {
        widget::card_title(&self.copy(), symbol, title, detail)
    }

    pub fn section_header<'e>(&self, title: &str, detail: Option<&str>) -> Element<'e, A> {
        widget::section_header(&self.copy(), title, detail)
    }

    pub fn form_row<'e>(&self, width: f32, label: &str, content: Element<'e, A>) -> Element<'e, A> {
        widget::form_row(&self.copy(), width, label, content)
    }

    pub fn empty_state<'e>(&self, symbol: Icon, title: &str, detail: &str) -> Element<'e, A> {
        widget::empty_state(&self.copy(), symbol, title, detail)
    }
}

fn labels(options: &[&str]) -> Vec<String> {
    options.iter().map(|option| (*option).to_string()).collect()
}
