use glam::Vec2;
use grana_ui::{Align, BuildContext, Edges, Element, Icon, Rect, Role, State};

use crate::message::Message;
use crate::panel::{MARGIN, TOOLBAR_HEIGHT};

pub const CONTROLS_WIDTH: f32 = CONTROL * 3.0 + GAP * 2.0 + PADDING * 2.0;
pub const TITLE_HEIGHT: f32 = MARGIN + TOOLBAR_HEIGHT;

const CONTROL: f32 = 30.0;
const PADDING: f32 = 6.0;
const GAP: f32 = 2.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChromeView {
    pub maximized: bool,
    pub controls: bool,
    pub leading: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowAction {
    Minimize,
    Maximize,
    Close,
}

pub fn title_band(size: Vec2) -> Rect {
    Rect::from_min_size(Vec2::ZERO, Vec2::new(size.x, TITLE_HEIGHT))
}

pub fn controls_rect(full: Rect) -> Rect {
    Rect::from_min_size(
        Vec2::new(full.max.x - MARGIN - CONTROLS_WIDTH, full.min.y + MARGIN),
        Vec2::new(CONTROLS_WIDTH, TOOLBAR_HEIGHT),
    )
}

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    full: Rect,
    chrome: ChromeView,
) -> Vec<Element<'a, Message>> {
    let mut elements = vec![
        Element::column()
            .at(Rect::from_min_size(
                full.min,
                Vec2::new(full.width(), TITLE_HEIGHT),
            ))
            .named("window.title")
            .blocks_pointer(),
    ];
    if chrome.controls {
        elements.push(controls(ctx, full, chrome.maximized));
    }
    elements
}

fn controls<'a>(
    ctx: &mut BuildContext<'_, Message>,
    full: Rect,
    maximized: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let language = ctx.language();
    let symbol = if maximized {
        Icon::WindowRestore
    } else {
        Icon::WindowMaximize
    };
    let maximize = if maximized {
        grana_i18n::header::restore_window(language)
    } else {
        grana_i18n::header::maximize_window(language)
    };
    let surface = theme.surface(Role::FloatingToolbar, State::Rest);
    grana_ui::row()
        .at(controls_rect(full))
        .padding(Edges::all(PADDING))
        .gap(GAP)
        .align(Align::Center)
        .background(surface)
        .children([
            grana_ui::widget::icon_button(
                Icon::WindowMinimize,
                CONTROL,
                Role::Standard,
                grana_i18n::header::minimize_window(language),
            )
            .named("window.minimize")
            .press(Message::Window(WindowAction::Minimize)),
            grana_ui::widget::icon_button(symbol, CONTROL, Role::Standard, maximize)
                .named("window.maximize")
                .press(Message::Window(WindowAction::Maximize)),
            grana_ui::widget::icon_button(
                Icon::Close,
                CONTROL,
                Role::Destructive,
                grana_i18n::header::close_window(language),
            )
            .named("window.close")
            .press(Message::Window(WindowAction::Close)),
        ])
        .named("window.controls")
}
