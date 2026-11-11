use glam::Vec2;
use grana_session::Command;
use grana_ui::glass::GlassStyle;
use grana_ui::{
    Align, BuildContext, Color, Corners, Edges, Element, Icon, Key, Length, Level, Rect, TextStyle,
    icon,
};

use crate::message::Message;
use crate::panel::Layout;
use crate::state::View;
use crate::view::{menu, menu_separator};

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    vec![
        navigation(ctx, layout, state),
        actions(ctx, layout, state),
        analysis(ctx, layout, state),
    ]
}

fn navigation<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let group = Key::new("header.navigation.group");
    let hidden = state.workspace.sidebar_hidden;
    let language = ctx.language();
    let tooltip = if hidden {
        grana_i18n::header::show_sidebar(language)
    } else {
        grana_i18n::header::hide_sidebar(language)
    };
    let toggle = chrome_icon_button(
        ctx,
        "sidebar.toggle",
        Icon::Sidebar,
        36.0,
        tooltip,
        Message::ToggleSidebar,
        group,
    );
    let project = project_pill(ctx, state, group);
    let open = open_pill(ctx, group);
    let project_menu = project_menu(ctx);
    let open_menu = open_menu(ctx, state);
    grana_ui::row()
        .at(layout.toolbar_left)
        .named("header.navigation")
        .gap(8.0)
        .align(Align::Center)
        .height(Length::Fill)
        .children([toggle, project, open, project_menu, open_menu])
}

fn settings_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let key = Key::new("settings.menu");
    let width = 280.0;
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, width, std::iter::empty());
    }
    let language = ctx.language();
    let english = grana_i18n::Language::English;
    let chinese = grana_i18n::Language::Chinese;
    let contrast = grana_ui::widget::toggle_row(
        ctx.theme(),
        grana_i18n::appearance::increase_contrast(language),
        state.workspace.contrast,
    )
    .named("settings.contrast")
    .on_toggle(Message::SetContrast);
    let transparency = grana_ui::widget::toggle_row(
        ctx.theme(),
        grana_i18n::appearance::reduce_transparency(language),
        state.workspace.reduce_transparency,
    )
    .named("settings.transparency")
    .on_toggle(Message::SetReduceTransparency);
    grana_ui::popover::popup(
        key,
        width,
        [
            crate::view::menu_row(
                ctx,
                "settings.language.english",
                english.label(),
                language == english,
                Some(Message::SetLanguage(english)),
            ),
            crate::view::menu_row(
                ctx,
                "settings.language.chinese",
                chinese.label(),
                language == chinese,
                Some(Message::SetLanguage(chinese)),
            ),
            menu_separator(ctx),
            contrast,
            transparency,
        ],
    )
    .persistent()
    .padding(Edges::all(12.0))
    .gap(6.0)
}

fn project_pill<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    group: Key,
) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let project_font = ctx.theme().font(Level::Callout).medium();
    let project = state
        .io
        .project
        .as_deref()
        .unwrap_or(grana_i18n::common::untitled(ctx.language()))
        .to_string();
    let dirty = state.io.dirty;
    let full = ctx
        .measure(&project, TextStyle::new(project_font, Color::WHITE))
        .x;
    let measured = full.min(136.0);
    let width = 12.0 + measured + if dirty { 12.0 } else { 0.0 } + 22.0;
    let key = Key::new("project.menu");
    let open = ctx.popup_open(key);
    let title = project.clone();
    let tooltip = (full > measured).then_some(project.as_str());
    let element = grana_ui::canvas(Length::Fixed(width), 36.0, move |painter, info| {
        let frame = info.glass.unwrap_or(info.rect);
        let engaged = info.hover.max(if open { 1.0 } else { 0.0 });
        if engaged > 0.01 {
            painter.rect(
                frame,
                Corners::capsule(),
                palette.material_fill.amplify(0.6 * engaged),
            );
        }
        let title_left = frame.min.x + 12.0;
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(title_left, frame.min.y),
                Vec2::new(title_left + measured, frame.max.y),
            ),
            &title,
            TextStyle::new(
                project_font,
                palette.text_secondary.lerp(palette.text_primary, engaged),
            )
            .truncating(),
            Align::Center,
        );
        if dirty {
            painter.rect(
                Rect::from_center_size(
                    Vec2::new(title_left + measured + 7.0, frame.center().y),
                    Vec2::splat(5.0),
                ),
                Corners::capsule(),
                palette.warning,
            );
        }
        icon::paint(
            Icon::ChevronDown,
            painter,
            Vec2::new(frame.max.x - 11.0, frame.center().y),
            9.0,
            palette.text_tertiary,
            1.5,
        );
    })
    .key(key)
    .press(Message::Noop)
    .glass(GlassStyle::button().interactive())
    .glass_group(group)
    .focus_ring(Corners::capsule());
    match tooltip {
        Some(tooltip) => element.tooltip(tooltip),
        None => element,
    }
}

fn open_pill<'a>(ctx: &mut BuildContext<'_, Message>, group: Key) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let font = ctx.theme().font(Level::Callout).medium();
    let key = Key::new("open.menu");
    let open = ctx.popup_open(key);
    let label = grana_i18n::header::open(ctx.language());
    grana_ui::canvas(Length::Fixed(84.0), 36.0, move |painter, info| {
        let frame = info.glass.unwrap_or(info.rect);
        let engaged = info.hover.max(if open { 1.0 } else { 0.0 });
        if engaged > 0.01 {
            painter.rect(
                frame,
                Corners::capsule(),
                palette.material_fill.amplify(0.6 * engaged),
            );
        }
        let color = palette.text_secondary.lerp(
            palette.text_primary,
            info.hover.max(if open { 1.0 } else { 0.0 }),
        );
        icon::paint(
            Icon::Folder,
            painter,
            Vec2::new(frame.min.x + 15.0, frame.center().y),
            13.0,
            color,
            1.5,
        );
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(frame.min.x + 26.0, frame.min.y),
                Vec2::new(frame.max.x - 18.0, frame.max.y),
            ),
            label,
            TextStyle::new(font, color),
            Align::Center,
        );
        icon::paint(
            Icon::ChevronDown,
            painter,
            Vec2::new(frame.max.x - 11.0, frame.center().y),
            9.0,
            palette.text_tertiary,
            1.5,
        );
    })
    .key(key)
    .press(Message::Noop)
    .glass(GlassStyle::button().interactive())
    .glass_group(group)
    .focus_ring(Corners::capsule())
}

fn actions<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let group = Key::new("header.actions.group");
    let undo = state.history.undo;
    let redo = state.history.redo;
    let undo_tooltip = undo.map_or_else(
        || grana_i18n::header::nothing_to_undo(ctx.language()).to_string(),
        |action| {
            grana_i18n::header::undo_action(
                ctx.language(),
                crate::text::action(ctx.language(), action),
            )
        },
    );
    let redo_tooltip = redo.map_or_else(
        || grana_i18n::header::nothing_to_redo(ctx.language()).to_string(),
        |action| {
            grana_i18n::header::redo_action(
                ctx.language(),
                crate::text::action(ctx.language(), action),
            )
        },
    );
    let undo_element = if undo.is_some() {
        chrome_icon_button(
            ctx,
            "undo",
            Icon::Undo,
            36.0,
            &undo_tooltip,
            Message::Command(Command::Undo),
            group,
        )
    } else {
        chrome_icon_button_disabled(ctx, "undo", Icon::Undo, 36.0, group)
    };
    let redo_element = if redo.is_some() {
        chrome_icon_button(
            ctx,
            "redo",
            Icon::Redo,
            36.0,
            &redo_tooltip,
            Message::Command(Command::Redo),
            group,
        )
    } else {
        chrome_icon_button_disabled(ctx, "redo", Icon::Redo, 36.0, group)
    };
    let palette = ctx.theme().palette;
    let pending = state.io.pending > 0 || !state.jobs.is_empty();
    let fit = chrome_icon_button(
        ctx,
        "view.fit",
        Icon::Fit,
        36.0,
        grana_i18n::header::frame_volume(ctx.language()),
        Message::FitView,
        group,
    );
    let settings = chrome_icon_button(
        ctx,
        "settings.menu",
        Icon::Sliders,
        36.0,
        grana_i18n::header::settings(ctx.language()),
        Message::Noop,
        group,
    );
    let settings_popup = settings_menu(ctx, state);
    let mut children = vec![
        palette_button(ctx, group),
        undo_element,
        redo_element,
        fit,
        settings,
        settings_popup,
    ];
    if pending {
        let dot_rect = Rect::from_center_size(
            Vec2::new(
                layout.toolbar_right.max.x - 7.0,
                layout.toolbar_right.min.y + 7.0,
            ),
            Vec2::splat(5.0),
        );
        children.push(
            grana_ui::canvas(Length::Fixed(5.0), 5.0, move |painter, info| {
                painter.rect(info.rect, Corners::capsule(), palette.accent);
            })
            .named("header.pending")
            .at(dot_rect),
        );
    }
    grana_ui::row()
        .at(layout.toolbar_right)
        .named("header.actions")
        .gap(8.0)
        .align(Align::Center)
        .justify(Align::End)
        .width(Length::Fill)
        .height(Length::Fill)
        .children(children)
}

fn palette_button<'a>(ctx: &mut BuildContext<'_, Message>, group: Key) -> Element<'a, Message> {
    let shortcut = crate::command::spec("palette.open")
        .and_then(|spec| spec.shortcut)
        .map_or_else(String::new, |shortcut| shortcut.label());
    let tooltip = grana_i18n::header::search_commands(ctx.language(), &shortcut);
    chrome_icon_button(
        ctx,
        "header.palette",
        Icon::Search,
        36.0,
        &tooltip,
        Message::OpenPalette,
        group,
    )
}

fn chrome_icon_button<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    symbol: Icon,
    size: f32,
    tooltip: &str,
    message: Message,
    group: Key,
) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let glyph = (size * 0.5).clamp(11.0, 17.0);
    grana_ui::canvas(Length::Fixed(size), size, move |painter, info| {
        let color = palette
            .text_secondary
            .lerp(palette.text_primary, info.hover.max(info.press));
        icon::paint(symbol, painter, info.rect.center(), glyph, color, 1.6);
    })
    .named(key)
    .press(message)
    .glass(GlassStyle::button().interactive())
    .glass_group(group)
    .focus_ring(Corners::capsule())
    .tooltip(tooltip)
}

fn chrome_icon_button_disabled<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    symbol: Icon,
    size: f32,
    group: Key,
) -> Element<'a, Message> {
    let color = ctx.theme().palette.text_quaternary;
    let glyph = (size * 0.5).clamp(11.0, 17.0);
    grana_ui::canvas(Length::Fixed(size), size, move |painter, info| {
        icon::paint(symbol, painter, info.rect.center(), glyph, color, 1.6);
    })
    .named(key)
    .enabled(false)
    .glass(GlassStyle::button_disabled())
    .glass_group(group)
    .focus_ring(Corners::capsule())
}

fn analysis<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let font = ctx.theme().button_font();
    let open = state.workspace.panel.is_some();
    let style = if open {
        GlassStyle::primary_active()
    } else {
        GlassStyle::primary()
    };
    let message = if open {
        Message::SetPanel(None)
    } else {
        Message::SetPanel(Some(state.workspace.last_panel))
    };
    let label = grana_i18n::header::analysis(ctx.language());
    grana_ui::canvas(Length::Fill, 36.0, move |painter, info| {
        let frame = info.glass.unwrap_or(info.rect);
        let mark = Rect::from_center_size(
            Vec2::new(frame.min.x + 22.0, frame.center().y),
            Vec2::splat(13.0),
        );
        icon::paint(Icon::Table, painter, mark.center(), 13.0, Color::WHITE, 1.6);
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(mark.max.x + 7.0, frame.min.y),
                Vec2::new(frame.max.x - 14.0, frame.max.y),
            ),
            label,
            TextStyle::new(font, Color::WHITE).aligned(Align::Start),
            Align::Center,
        );
        if open {
            painter.rect(
                Rect::from_center_size(
                    Vec2::new(frame.max.x - 12.0, frame.center().y),
                    Vec2::splat(4.5),
                ),
                Corners::capsule(),
                palette.text_on_accent.with_alpha(0.85),
            );
        }
    })
    .at(layout.analysis)
    .named("header.analysis")
    .glass(style.interactive())
    .press(message)
    .focus_ring(Corners::capsule())
}

fn open_menu<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let key = Key::new("open.menu");
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, 236.0, std::iter::empty());
    }
    let mut rows = vec![
        command_row(ctx, "open.image"),
        command_row(ctx, "open.folder"),
        command_row(ctx, "open.remote"),
        menu_separator(ctx),
        command_row(ctx, "open.project"),
    ];
    if !state.recent.is_empty() {
        rows.push(menu_separator(ctx));
        rows.push(crate::view::section_header(
            ctx,
            grana_i18n::header::recent(ctx.language()),
            None,
        ));
        for (index, entry) in state.recent.iter().take(crate::RECENT_LIMIT).enumerate() {
            rows.push(crate::view::action_menu_message(
                ctx,
                &format!("open.recent.{index}"),
                match entry.kind() {
                    grana_session::RecentKind::Image => Icon::Cube,
                    grana_session::RecentKind::Project => Icon::Folder,
                    grana_session::RecentKind::Remote => Icon::Globe,
                },
                entry.name(),
                None,
                Message::Command(Command::OpenRecent(index)),
            ));
        }
    }
    menu(ctx, key, 236.0, rows)
}

fn project_menu<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let key = Key::new("project.menu");
    if !ctx.popup_open(key) {
        return grana_ui::popover::popup(key, 228.0, std::iter::empty());
    }
    let rows = vec![
        command_row(ctx, "save.project"),
        command_row(ctx, "save.as"),
        menu_separator(ctx),
        command_row(ctx, "snapshot"),
        command_row(ctx, "figure"),
        command_row(ctx, "labels"),
    ];
    menu(ctx, key, 228.0, rows)
}

fn command_row<'a>(ctx: &mut BuildContext<'_, Message>, id: &str) -> Element<'a, Message> {
    let spec = crate::command::spec(id).expect("a menu names a registered command");
    let shortcut = spec.shortcut.map(|shortcut| shortcut.label());
    crate::view::action_menu_message(
        ctx,
        id,
        spec.symbol,
        crate::text::command(ctx.language(), id),
        shortcut.as_deref(),
        spec.action.message(),
    )
}
