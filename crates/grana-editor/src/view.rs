use grana_session::Command;
use grana_ui::popover::separator as separator_widget;
use grana_ui::{BuildContext, Corners, Edges, Element, Icon, Key, Length, Motion, Role};

use crate::message::Message;

pub type View<'a> = Element<'a, Message>;

pub fn card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    gap: f32,
    children: Vec<View<'a>>,
) -> View<'a> {
    ctx.card(Role::Card, Edges::all(12.0), gap, children)
}

pub fn group_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    index: usize,
    count: usize,
    active: bool,
    gap: f32,
    children: Vec<View<'a>>,
) -> View<'a> {
    ctx.group_card((index, count), active, Edges::all(12.0), gap, children)
}

pub fn card_title<'a>(
    ctx: &mut BuildContext<'_, Message>,
    symbol: Icon,
    title: &str,
    detail: Option<&str>,
) -> View<'a> {
    ctx.card_title(symbol, title, detail)
}

pub fn divider<'a>(ctx: &mut BuildContext<'_, Message>) -> View<'a> {
    ctx.divider()
}

pub fn section_header<'a>(
    ctx: &mut BuildContext<'_, Message>,
    title: &str,
    detail: Option<&str>,
) -> View<'a> {
    ctx.section_header(title, detail)
}

pub fn disclosure_group_card<'a>(
    ctx: &mut BuildContext<'_, Message>,
    slot: (usize, usize),
    key: Key,
    head: (Icon, &str, Option<&str>),
    children: Vec<View<'a>>,
    open: bool,
) -> View<'a> {
    let header = disclosure_header(ctx, key, head.0, head.1, head.2, open);
    let mut items = vec![header];
    if open && !children.is_empty() {
        items.extend(children);
    }
    ctx.group_card(slot, open, Edges::all(12.0), 8.0, items)
}

fn disclosure_header<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: Key,
    symbol: Icon,
    title: &str,
    detail: Option<&str>,
    open: bool,
) -> View<'a> {
    let angle = ctx.animate(
        key.then("chevron").id(),
        if open { 1.0 } else { 0.0 },
        Motion::spatial_fast(),
    );
    ctx.disclosure_row(symbol, title, detail.unwrap_or(""), open, angle)
        .key(key)
        .press(Message::ToggleOpen(key))
        .focus_ring(Corners::capsule())
}

pub fn form_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    label: &str,
    content: View<'a>,
) -> View<'a> {
    ctx.form_row(crate::panel::FORM_LABEL, label, content)
}

pub fn menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    anchor: Key,
    width: f32,
    rows: Vec<View<'a>>,
) -> View<'a> {
    let _ = ctx;
    grana_ui::popover::popup(anchor, width, rows)
}

pub fn action_menu_message<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    symbol: Icon,
    label: &str,
    shortcut: Option<&str>,
    message: Message,
) -> View<'a> {
    ctx.action_row(symbol, label, shortcut)
        .named(key)
        .press(message)
}

pub fn menu_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: &str,
    checked: bool,
    message: Option<Message>,
) -> View<'a> {
    let row = ctx.menu_row(label, checked).named(key);
    match message {
        Some(message) => row.press(message),
        None => row,
    }
}

pub fn menu_separator<'a>(ctx: &mut BuildContext<'_, Message>) -> View<'a> {
    let _ = ctx;
    separator_widget()
}

pub fn labelled<'a>(ctx: &mut BuildContext<'_, Message>, label: &str, value: &str) -> View<'a> {
    ctx.labelled(label, value)
}

pub fn caption<'a>(ctx: &mut BuildContext<'_, Message>, text: &str) -> View<'a> {
    ctx.caption(text)
}

pub fn weak<'a>(ctx: &mut BuildContext<'_, Message>, text: &str) -> View<'a> {
    ctx.weak(text)
}

pub fn spinner<'a>(ctx: &mut BuildContext<'_, Message>, size: f32) -> View<'a> {
    ctx.spinner(size)
}

pub fn button<'a>(ctx: &mut BuildContext<'_, Message>, label: &str, role: Role) -> View<'a> {
    ctx.button(label, role)
}

pub fn button_with<'a>(
    ctx: &mut BuildContext<'_, Message>,
    label: &str,
    role: Role,
    width: Length,
) -> View<'a> {
    ctx.button_width(label, role, width)
}

pub fn button_shaped<'a>(
    ctx: &mut BuildContext<'_, Message>,
    label: &str,
    role: Role,
    width: Length,
    corners: Corners,
) -> View<'a> {
    ctx.button_width(label, role, width).shaped(corners)
}

pub fn icon_button<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    symbol: Icon,
    size: f32,
    role: Role,
    tooltip: &str,
    message: Message,
) -> View<'a> {
    ctx.icon_button(symbol, size, role, tooltip)
        .named(key)
        .press(message)
}

pub fn split_button<'a>(
    ctx: &mut BuildContext<'_, Message>,
    symbol: Option<Icon>,
    label: &str,
    role: Role,
    leading: (&str, Message),
    trailing: (Key, Message),
) -> View<'a> {
    let open = ctx.popup_open(trailing.0);
    let spec = grana_ui::widget::Split {
        symbol,
        label,
        role,
        leading_key: Key::new(leading.0),
        leading_action: leading.1,
        trailing_key: trailing.0,
        trailing_action: trailing.1,
        open,
    };
    ctx.split_button(spec)
}

pub fn slider_readout<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: Option<&str>,
    value: f32,
    range: (std::ops::RangeInclusive<f32>, bool),
    readout: &str,
    command: impl Fn(f32) -> Command + 'static,
) -> View<'a> {
    ctx.slider(label, value, range.0, range.1, readout)
        .named(key)
        .on_slide(move |value| Message::Command(command(value)))
}

pub fn slider_event<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: Option<&str>,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    readout: &str,
    message: impl Fn(f32) -> Message + 'static,
) -> View<'a> {
    ctx.slider(label, value, range, false, readout)
        .named(key)
        .on_slide(message)
}

pub fn toggle_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: &str,
    value: bool,
    command: impl Fn(bool) -> Command + 'static,
) -> View<'a> {
    ctx.toggle_row(label, value)
        .named(key)
        .on_toggle(move |value| Message::Command(command(value)))
}

pub fn segmented<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    options: &[&str],
    selected: usize,
    message: impl Fn(usize) -> Message + 'static,
) -> View<'a> {
    ctx.segmented(options, selected)
        .named(key)
        .on_segment(message)
}

pub fn segmented_fill<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    options: &[&str],
    selected: usize,
    message: impl Fn(usize) -> Message + 'static,
) -> View<'a> {
    ctx.segmented_fill(options, selected)
        .named(key)
        .on_segment(message)
}

pub fn tabs<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    options: &[&str],
    selected: usize,
    message: impl Fn(usize) -> Message + 'static,
) -> View<'a> {
    ctx.tabs(options, selected).named(key).on_segment(message)
}

pub fn empty_state<'a>(
    ctx: &mut BuildContext<'_, Message>,
    symbol: Icon,
    title: &str,
    detail: &str,
) -> View<'a> {
    ctx.empty_state(symbol, title, detail)
}

pub fn column_label(language: grana_i18n::Language, column: &grana_measure::Column) -> String {
    let raw = crate::text::column(language, column.label());
    let mut chars = raw.chars();
    let label = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    let unit = column.unit().suffix();
    if unit.is_empty() {
        label
    } else {
        format!("{label} ({unit})")
    }
}

pub fn progress(
    analysis: &[grana_scene::Progress],
    job: grana_scene::Job,
) -> Option<grana_scene::Progress> {
    analysis
        .iter()
        .copied()
        .find(|progress| progress.job == job)
}

pub fn progress_caption(
    language: grana_i18n::Language,
    kind: grana_scene::ObjectKind,
    job: grana_scene::Job,
    progress: grana_scene::Progress,
    frames: u32,
) -> String {
    let unit = match job {
        grana_scene::Job::Detect(_) => match kind {
            grana_scene::ObjectKind::Spots => grana_i18n::common::blocks(language),
            grana_scene::ObjectKind::Surfaces => grana_i18n::common::tiles(language),
            grana_scene::ObjectKind::Filaments => grana_i18n::common::frames(language),
            grana_scene::ObjectKind::Cells => grana_i18n::common::frames(language),
        },
        grana_scene::Job::Import(_) => grana_i18n::common::tiles(language),
        grana_scene::Job::Localizations(_) => grana_i18n::common::steps(language),
        grana_scene::Job::Clusters(_) => grana_i18n::common::frames(language),
        grana_scene::Job::Track(_)
        | grana_scene::Job::Recut(_)
        | grana_scene::Job::Relation { .. }
        | grana_scene::Job::Coloc
        | grana_scene::Job::ObjectColoc(_)
        | grana_scene::Job::Drift
        | grana_scene::Job::Register => grana_i18n::common::frames(language),
        grana_scene::Job::Calibrate => grana_i18n::common::beads(language),
    };
    let prefix = match job {
        grana_scene::Job::Detect(_) | grana_scene::Job::Import(_) if frames > 1 => {
            grana_i18n::common::frame_prefix(language, progress.time + 1)
        }
        grana_scene::Job::Detect(_)
        | grana_scene::Job::Import(_)
        | grana_scene::Job::Recut(_)
        | grana_scene::Job::Track(_)
        | grana_scene::Job::Relation { .. }
        | grana_scene::Job::Coloc
        | grana_scene::Job::ObjectColoc(_)
        | grana_scene::Job::Localizations(_)
        | grana_scene::Job::Clusters(_)
        | grana_scene::Job::Drift
        | grana_scene::Job::Register
        | grana_scene::Job::Calibrate => String::new(),
    };
    if progress.total > 0 {
        format!(
            "{prefix}{}",
            grana_i18n::common::progress(language, progress.done, progress.total, unit)
        )
    } else {
        format!("{prefix}{}", grana_i18n::common::preparing(language))
    }
}
