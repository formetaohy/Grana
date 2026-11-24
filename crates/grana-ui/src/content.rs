use std::ops::RangeInclusive;

use crate::color::Color;
use crate::geometry::{Band, Corners};
use crate::icon::{Icon, IconStyle};
use crate::text::TextStyle;
use crate::theme::{Role, TypeStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContentKind {
    Empty,
    Text,
    Canvas,
    Icon,
    Button,
    IconButton,
    Toggle,
    Checkbox,
    Slider,
    RangeSlider,
    Segment,
    Tabs,
    DragValue,
    Field,
    MenuRow,
    ActionRow,
    Badge,
    Loading,
    Progress,
    GradientBar,
    Labelled,
    ColorSwatch,
    ColorPicker,
}

pub(crate) enum Content {
    Empty,
    Text(TextContent),
    Canvas,
    Icon(IconContent),
    Button(ButtonContent),
    IconButton(IconButtonContent),
    Toggle(ToggleContent),
    Checkbox(CheckboxContent),
    Slider(SliderContent),
    RangeSlider(RangeSliderContent),
    Segment(SegmentContent),
    Tabs(TabsContent),
    DragValue(DragValueContent),
    Field(FieldContent),
    MenuRow(MenuRowContent),
    ActionRow(ActionRowContent),
    Badge(BadgeContent),
    Loading(LoadingContent),
    Progress(ProgressContent),
    GradientBar(GradientBarContent),
    Labelled(LabelledContent),
    ColorSwatch(ColorSwatchContent),
    ColorPicker(ColorPickerContent),
}

impl Content {
    pub(crate) fn kind(&self) -> ContentKind {
        match self {
            Self::Empty => ContentKind::Empty,
            Self::Text(_) => ContentKind::Text,
            Self::Canvas => ContentKind::Canvas,
            Self::Icon(_) => ContentKind::Icon,
            Self::Button(_) => ContentKind::Button,
            Self::IconButton(_) => ContentKind::IconButton,
            Self::Toggle(_) => ContentKind::Toggle,
            Self::Checkbox(_) => ContentKind::Checkbox,
            Self::Slider(_) => ContentKind::Slider,
            Self::RangeSlider(_) => ContentKind::RangeSlider,
            Self::Segment(_) => ContentKind::Segment,
            Self::Tabs(_) => ContentKind::Tabs,
            Self::DragValue(_) => ContentKind::DragValue,
            Self::Field(_) => ContentKind::Field,
            Self::MenuRow(_) => ContentKind::MenuRow,
            Self::ActionRow(_) => ContentKind::ActionRow,
            Self::Badge(_) => ContentKind::Badge,
            Self::Loading(_) => ContentKind::Loading,
            Self::Progress(_) => ContentKind::Progress,
            Self::GradientBar(_) => ContentKind::GradientBar,
            Self::Labelled(_) => ContentKind::Labelled,
            Self::ColorSwatch(_) => ContentKind::ColorSwatch,
            Self::ColorPicker(_) => ContentKind::ColorPicker,
        }
    }

    pub(crate) fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) | (Self::Canvas, Self::Canvas) => true,
            (Self::Loading(a), Self::Loading(b)) => a == b,
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Icon(a), Self::Icon(b)) => a == b,
            (Self::Button(a), Self::Button(b)) => a == b,
            (Self::IconButton(a), Self::IconButton(b)) => a == b,
            (Self::Toggle(a), Self::Toggle(b)) => a == b,
            (Self::Checkbox(a), Self::Checkbox(b)) => a == b,
            (Self::Slider(a), Self::Slider(b)) => a == b,
            (Self::RangeSlider(a), Self::RangeSlider(b)) => a == b,
            (Self::Segment(a), Self::Segment(b)) => a == b,
            (Self::Tabs(a), Self::Tabs(b)) => a == b,
            (Self::DragValue(a), Self::DragValue(b)) => a == b,
            (Self::Field(a), Self::Field(b)) => a == b,
            (Self::MenuRow(a), Self::MenuRow(b)) => a == b,
            (Self::ActionRow(a), Self::ActionRow(b)) => a == b,
            (Self::Badge(a), Self::Badge(b)) => a == b,
            (Self::Progress(a), Self::Progress(b)) => a == b,
            (Self::GradientBar(a), Self::GradientBar(b)) => a == b,
            (Self::Labelled(a), Self::Labelled(b)) => a == b,
            (Self::ColorSwatch(a), Self::ColorSwatch(b)) => a == b,
            (Self::ColorPicker(a), Self::ColorPicker(b)) => a == b,
            _ => false,
        }
    }

    pub(crate) fn dirty_since(&self, previous: &Self) -> Option<crate::element::Dirty> {
        if self.same(previous) {
            return None;
        }
        let geometry = self.geometry() != previous.geometry();
        Some(crate::element::Dirty {
            geometry,
            paint: true,
            animate: true,
            hits: geometry,
            fields: matches!(self, Self::Field(_)),
            ..crate::element::Dirty::NONE
        })
    }

    pub(crate) fn geometry(&self) -> Geometry<'_> {
        match self {
            Self::Empty => Geometry::Fixed,
            Self::Canvas => Geometry::Fixed,
            Self::Text(text) => Geometry::Text(text),
            Self::Icon(icon) => Geometry::Size(icon.size),
            Self::Button(button) => Geometry::Button(button),
            Self::IconButton(icon) => Geometry::Size(icon.size),
            Self::Toggle(toggle) => Geometry::Row(toggle.row),
            Self::Checkbox(checkbox) => Geometry::Label(&checkbox.label, checkbox.font),
            Self::Slider(slider) => Geometry::Slider(
                slider.label.as_deref(),
                slider.readout.is_some(),
                slider.width,
            ),
            Self::RangeSlider(range) => Geometry::Slider(None, false, range.width),
            Self::Segment(segment) => {
                Geometry::Segment(&segment.options, segment.font, segment.width, segment.fill)
            }
            Self::Tabs(_) => Geometry::Fixed,
            Self::DragValue(drag) => Geometry::Width(drag.width),
            Self::Field(_) => Geometry::Fixed,
            Self::MenuRow(_) => Geometry::Fixed,
            Self::ActionRow(_) => Geometry::Fixed,
            Self::Badge(badge) => Geometry::Label(&badge.label, badge.font),
            Self::Loading(loading) => Geometry::Size(loading.size),
            Self::Progress(progress) => Geometry::Width(progress.width),
            Self::GradientBar(bar) => Geometry::Size(bar.height),
            Self::Labelled(_) => Geometry::Fixed,
            Self::ColorSwatch(swatch) => Geometry::Size(swatch.size),
            Self::ColorPicker(_) => Geometry::Fixed,
        }
    }
}

#[derive(PartialEq)]
pub(crate) enum Geometry<'a> {
    Fixed,
    Text(&'a TextContent),
    Size(f32),
    Button(&'a ButtonContent),
    Row(bool),
    Label(&'a str, TypeStyle),
    Slider(Option<&'a str>, bool, crate::element::Length),
    Segment(&'a [String], TypeStyle, crate::element::Length, bool),
    Width(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconContent {
    pub symbol: Icon,
    pub size: f32,
    pub color: Color,
    pub style: IconStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextContent {
    pub text: String,
    pub style: TextStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ButtonContent {
    pub label: String,
    pub role: Role,
    pub font: TypeStyle,
    pub symbol: Option<Icon>,
    pub inset: f32,
    pub width: crate::element::Length,
    pub height: f32,
    pub corners: Option<Corners>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IconButtonContent {
    pub symbol: Icon,
    pub size: f32,
    pub role: Role,
    pub corners: Option<Corners>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToggleContent {
    pub value: bool,
    pub label: Option<String>,
    pub font: TypeStyle,
    pub row: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckboxContent {
    pub value: bool,
    pub label: String,
    pub font: TypeStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SliderContent {
    pub value: f32,
    pub range: RangeInclusive<f32>,
    pub label: Option<String>,
    pub readout: Option<String>,
    pub logarithmic: bool,
    pub width: crate::element::Length,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RangeSliderContent {
    pub start: f32,
    pub end: f32,
    pub range: RangeInclusive<f32>,
    pub gradient: Vec<Color>,
    pub width: crate::element::Length,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentContent {
    pub options: Vec<String>,
    pub selected: usize,
    pub fill: bool,
    pub font: TypeStyle,
    pub width: crate::element::Length,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabsContent {
    pub options: Vec<String>,
    pub selected: usize,
    pub font: TypeStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DragValueContent {
    pub value: f32,
    pub decimals: usize,
    pub font: TypeStyle,
    pub width: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldContent {
    pub value: String,
    pub style: TextStyle,
    pub multiline: bool,
    pub radius: f32,
    pub editing: bool,
    pub select_all: bool,
    pub error: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuRowContent {
    pub label: String,
    pub checked: bool,
    pub font: TypeStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActionRowContent {
    pub symbol: Icon,
    pub label: String,
    pub shortcut: Option<String>,
    pub detail: Option<String>,
    pub font: TypeStyle,
    pub trailing: Option<Trailing>,
    pub highlight: bool,
    pub band: Band,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trailing {
    pub symbol: Icon,
    pub angle: f32,
    pub open: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BadgeContent {
    pub label: String,
    pub tint: Color,
    pub font: TypeStyle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoadingContent {
    pub size: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProgressContent {
    pub fraction: f32,
    pub width: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GradientBarContent {
    pub colors: Vec<Color>,
    pub height: f32,
    pub radius: Corners,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelledContent {
    pub label: String,
    pub value: String,
    pub label_font: TypeStyle,
    pub value_font: TypeStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColorSwatchContent {
    pub color: [f32; 3],
    pub size: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorPickerContent {
    pub color: [f32; 3],
}
