use glam::Vec2;
use grana_session::{RecentKind, SessionView};
use grana_ui::{
    Align, BuildContext, Corners, Edges, Element, Icon, Key, Layer, Length, Level, Rect, Role,
    State, TextStyle, Ui,
};

use crate::command::{Action, Group, Scope, Shortcut};
use crate::message::Message;
use crate::state::Workspace;

const WIDTH: f32 = 560.0;
const TOP: f32 = 84.0;
const PADDING: f32 = 12.0;
const FIELD: f32 = 40.0;
const ROW: f32 = 32.0;
const HEADER: f32 = 22.0;
const FOOTER: f32 = 20.0;
const DIVIDER: f32 = 8.0;
pub(crate) const VISIBLE: usize = 9;

pub struct Entry {
    pub title: String,
    pub group: Group,
    pub symbol: Icon,
    pub shortcut: Option<Shortcut>,
    pub message: Message,
}

pub struct PaletteView {
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub start: usize,
}

pub(crate) fn resolve(
    ui: &Ui<Message>,
    view: &SessionView<'_>,
    workspace: &mut Workspace,
) -> Option<PaletteView> {
    let state = workspace.palette.as_ref()?;
    let mut selected = state.selected;
    let query = ui.editing_text().unwrap_or_default().to_string();
    let entries = catalog(view, workspace, &query);
    if entries.is_empty() {
        selected = 0;
    } else {
        selected = selected.min(entries.len() - 1);
        let mut delta = 0i64;
        let mut edge = None;
        for event in ui.input().keys() {
            if !event.pressed || event.repeat {
                continue;
            }
            match event.key {
                Some(grana_ui::InputKey::Up) => delta -= 1,
                Some(grana_ui::InputKey::Down) => delta += 1,
                Some(grana_ui::InputKey::PageUp) => delta -= VISIBLE as i64,
                Some(grana_ui::InputKey::PageDown) => delta += VISIBLE as i64,
                Some(grana_ui::InputKey::Home) => edge = Some(0),
                Some(grana_ui::InputKey::End) => edge = Some(entries.len() - 1),
                _ => {}
            }
        }
        if let Some(target) = edge {
            selected = target;
        } else if delta != 0 {
            let count = entries.len() as i64;
            selected = (selected as i64 + delta).rem_euclid(count) as usize;
        }
    }
    let start = selected
        .saturating_sub(VISIBLE - 1)
        .min(entries.len().saturating_sub(VISIBLE));
    workspace.palette_actions = entries.iter().map(|entry| entry.message.clone()).collect();
    if let Some(state) = workspace.palette.as_mut() {
        state.selected = selected;
    }
    Some(PaletteView {
        entries,
        selected,
        start,
    })
}

fn catalog(view: &SessionView<'_>, workspace: &Workspace, query: &str) -> Vec<Entry> {
    let language = workspace.language;
    let scene = view.scene.is_some();
    let mut entries = Vec::new();
    for group in Group::ALL {
        for spec in crate::command::SPECS
            .iter()
            .filter(|spec| spec.group == group)
        {
            if spec.scope == Scope::Scene && !scene {
                continue;
            }
            entries.push(Entry {
                title: crate::text::command(language, spec.id).to_string(),
                group,
                symbol: spec.symbol,
                shortcut: spec.shortcut,
                message: spec.action.message(),
            });
        }
        for extra in dynamic(view, workspace, language, group, scene) {
            entries.push(extra);
        }
    }
    filter(entries, query, language)
}

fn dynamic(
    view: &SessionView<'_>,
    workspace: &Workspace,
    language: grana_i18n::Language,
    group: Group,
    scene: bool,
) -> Vec<Entry> {
    let mut entries = Vec::new();
    match group {
        Group::File => {
            for (index, recent) in view.recent.iter().enumerate() {
                entries.push(Entry {
                    title: grana_i18n::palette::open_recent(language, recent.name()),
                    group,
                    symbol: match recent.kind() {
                        RecentKind::Image => Icon::Cube,
                        RecentKind::Project => Icon::Folder,
                        RecentKind::Remote => Icon::Globe,
                    },
                    shortcut: None,
                    message: Action::OpenRecent(index).message(),
                });
            }
        }
        Group::View if scene => {
            for (index, named) in view.views.iter().enumerate() {
                entries.push(Entry {
                    title: grana_i18n::palette::apply_view(language, &named.name),
                    group,
                    symbol: Icon::Camera,
                    shortcut: None,
                    message: Action::ApplyView(index).message(),
                });
            }
        }
        Group::Time if scene => entries.push(Entry {
            title: if workspace.playback.playing {
                grana_i18n::palette::pause_playback(language).to_string()
            } else {
                grana_i18n::palette::play(language).to_string()
            },
            group,
            symbol: if workspace.playback.playing {
                Icon::Pause
            } else {
                Icon::Play
            },
            shortcut: None,
            message: if workspace.playback.playing {
                Action::Pause.message()
            } else {
                Action::Play.message()
            },
        }),
        _ => {}
    }
    entries
}

fn filter(entries: Vec<Entry>, query: &str, language: grana_i18n::Language) -> Vec<Entry> {
    let tokens = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return entries;
    }
    let mut kept = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        let title = entry.title.to_lowercase();
        let group = entry.group.title(language).to_lowercase();
        let mut score = 0i32;
        let mut matched = true;
        for token in &tokens {
            if title.starts_with(token.as_str()) {
                score += 4;
            } else if title.contains(token.as_str()) {
                score += 2;
            } else if group.contains(token.as_str()) {
                score += 1;
            } else {
                matched = false;
                break;
            }
        }
        if matched {
            kept.push((order(entry.group), -score, index, entry));
        }
    }
    kept.sort_by_key(|(group, score, index, _)| (*group, *score, *index));
    kept.into_iter().map(|(_, _, _, entry)| entry).collect()
}

fn order(group: Group) -> usize {
    Group::ALL
        .iter()
        .position(|candidate| *candidate == group)
        .unwrap_or(0)
}

pub(crate) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    full: Rect,
    palette: &PaletteView,
) -> Vec<Element<'a, Message>> {
    let theme = *ctx.theme();
    let palette_colors = theme.palette;
    let width = WIDTH.min(full.width() - 48.0).max(320.0);
    let listed = palette.entries.len().min(VISIBLE);
    let window = &palette.entries[palette.start..palette.start + listed];
    let mut headers = 0usize;
    let mut previous = None;
    for entry in window {
        if previous != Some(entry.group) {
            previous = Some(entry.group);
            headers += 1;
        }
    }
    let height = 2.0 * PADDING
        + FIELD
        + DIVIDER
        + ROW * (listed.max(1)) as f32
        + HEADER * headers as f32
        + FOOTER;
    let rect = Rect::from_min_size(
        Vec2::new(full.center().x - 0.5 * width, full.min.y + TOP),
        Vec2::new(width, height),
    );
    let backdrop = palette_colors.scrim;
    let mut elements = vec![
        grana_ui::canvas(Length::Fill, full.height(), move |painter, info| {
            painter.rect(info.rect, Corners::ZERO, backdrop);
        })
        .named("palette.backdrop")
        .at(full)
        .layer(Layer::Popup)
        .press(Message::ClosePalette),
    ];
    let field_style = TextStyle::new(theme.font(Level::Body), theme.palette.on_surface);
    let field = grana_ui::widget::text_field("", field_style, width - 2.0 * PADDING - 38.0, false)
        .key(Key::new("palette.field"))
        .editing(true)
        .on_commit(|_| Message::RunSelected)
        .on_cancel(|| Message::ClosePalette);
    let search = grana_ui::canvas(Length::Fixed(26.0), 26.0, move |painter, info| {
        grana_ui::icon::paint(
            Icon::Search,
            painter,
            info.rect.center(),
            14.5,
            theme.palette.primary,
            1.6,
        );
    });
    let mut children = vec![
        grana_ui::row()
            .gap(6.0)
            .align(Align::Center)
            .height(Length::Fixed(FIELD))
            .padding(Edges::symmetric(6.0, 0.0))
            .children([search, field]),
        divider(ctx),
    ];
    let mut previous = None;
    for (offset, entry) in palette.entries[palette.start..palette.start + listed]
        .iter()
        .enumerate()
    {
        let index = palette.start + offset;
        if previous != Some(entry.group) {
            previous = Some(entry.group);
            children.push(group_header(ctx, entry.group));
        }
        children.push(row(ctx, entry, index, index == palette.selected));
    }
    if palette.entries.is_empty() {
        let empty = grana_i18n::palette::no_matches(ctx.language());
        children.push(
            grana_ui::canvas(Length::Fill, ROW, move |painter, info| {
                painter.text_in(
                    info.rect,
                    empty,
                    TextStyle::new(theme.font(Level::Body), theme.palette.outline),
                    Align::Center,
                );
            })
            .named("palette.empty"),
        );
    }
    children.push(footer(ctx));
    let surface = theme
        .surface(Role::Menu, State::Rest)
        .with_corners(Corners::all(theme.metrics.corner_xxl));
    elements.push(
        grana_ui::column()
            .at(rect)
            .named("palette")
            .padding(Edges::all(PADDING))
            .gap(0.0)
            .background(surface)
            .layer(Layer::Popup)
            .children(children),
    );
    elements
}

fn divider<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let color = ctx.theme().palette.outline_variant.with_alpha(0.5);
    grana_ui::canvas(Length::Fill, DIVIDER, move |painter, info| {
        painter.rect(
            Rect::from_min_size(
                Vec2::new(info.rect.min.x, info.rect.center().y),
                Vec2::new(info.rect.width(), 1.0),
            ),
            Corners::ZERO,
            color,
        );
    })
}

fn group_header<'a>(ctx: &mut BuildContext<'_, Message>, group: Group) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let font = theme.font(Level::Overline);
    let title = group.title(ctx.language()).to_uppercase();
    grana_ui::canvas(Length::Fill, HEADER, move |painter, info| {
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(info.rect.min.x + 10.0, info.rect.min.y),
                info.rect.max,
            ),
            &title,
            TextStyle::new(font, palette.primary),
            Align::Center,
        );
    })
}

fn row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    entry: &Entry,
    index: usize,
    selected: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let symbol = entry.symbol;
    let title = entry.title.clone();
    let shortcut = entry.shortcut.map(Shortcut::label);
    let title_style = TextStyle::new(
        theme.font(Level::Label),
        if selected {
            palette.on_secondary_container
        } else {
            palette.on_surface
        },
    );
    let shortcut_style = TextStyle::new(theme.font(Level::Monospace), palette.outline);
    grana_ui::canvas(Length::Fill, ROW, move |painter, info| {
        let rect = info.rect;
        let engaged = if selected { 1.0 } else { info.hover };
        if selected {
            painter.rect(rect, Corners::capsule(), palette.secondary_container);
        } else if engaged > 0.01 {
            painter.rect(
                rect,
                Corners::all(theme.metrics.corner_sm),
                palette.on_surface.with_alpha(0.08 * engaged),
            );
        }
        grana_ui::icon::paint(
            symbol,
            painter,
            Vec2::new(rect.min.x + 15.0, rect.center().y),
            13.5,
            if selected {
                palette.on_secondary_container
            } else {
                palette.on_surface_variant.lerp(palette.on_surface, engaged)
            },
            1.6,
        );
        let shortcut_width = shortcut.as_ref().map_or(10.0, |label| {
            24.0 + painter.measure(label, shortcut_style).x
        });
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 30.0, rect.min.y),
                Vec2::new(rect.max.x - shortcut_width, rect.max.y),
            ),
            &title,
            title_style,
            Align::Center,
        );
        if let Some(label) = &shortcut {
            painter.text_in(
                rect.shrink2(Vec2::new(10.0, 0.0)),
                label,
                shortcut_style.aligned(Align::End),
                Align::Center,
            );
        }
    })
    .key(Key::index("palette.row", index))
    .press(Message::RunPaletteIndex(index))
    .focus_ring(Corners::capsule())
}

fn footer<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let font = ctx.theme().font(Level::Caption);
    let hint = grana_i18n::palette::footer_hint(ctx.language());
    grana_ui::canvas(Length::Fill, FOOTER, move |painter, info| {
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(info.rect.min.x + 10.0, info.rect.min.y),
                info.rect.max,
            ),
            hint,
            TextStyle::new(font, palette.outline),
            Align::Center,
        );
    })
}
