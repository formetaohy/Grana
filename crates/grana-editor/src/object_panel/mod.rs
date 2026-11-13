use glam::Vec2;
use grana_i18n::Language;
use grana_measure::{ChildField, ColocMetric, Group, Measurement};
use grana_scene::{
    IntensitySource, Item, Job, ObjectColor, ObjectFrame, ObjectKind, ObjectSet, ObjectSetId,
    ParamField, ParamScale, Provenance, RelationField, SEGMENT_FIELDS, Scene,
};
use grana_session::{Command, MeshFormat};
use grana_ui::{
    Align, BuildContext, Color, Corners, Element, Icon, Key, Length, Level, Motion, Rect, Role,
    TextStyle, icon,
};

use crate::message::Message;
use crate::state::View;
use crate::view;

mod appearance;
mod card;
mod classifier;
mod detail;
mod metrics;
mod relations;

use appearance::{appearance, appearance_fields};
use card::header_row;
use classifier::classifier;
use detail::{attached, curated, detect, importing, origin, params, seeds, summary};
use metrics::{child_metrics, children_section, coloc_metrics, label_metrics, measure};
use relations::relations;

const HEADER: f32 = 36.0;
const PAD: f32 = 12.0;

fn width() -> f32 {
    crate::inspector::panel_width() - PAD * 2.0
}

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let channel = state.scene.selected;
    let channel_name = state.scene.channel(channel).name().to_string();
    let count = state.scene.objects.len();
    let language = ctx.language();
    let loading = state.io.loading.is_some();
    let authoring = state.access.author();
    let mut elements = if authoring {
        add_menu(ctx, state)
    } else {
        Vec::new()
    };
    elements.insert(0, header(ctx, count, loading, authoring));
    if !state.import.is_empty() {
        elements.insert(1, notes(ctx, state.import));
    }
    if count == 0 {
        elements.push(view::empty_state(
            ctx,
            Icon::Layers,
            grana_i18n::object::no_sets(language),
            &grana_i18n::object::no_sets_detail(language, &channel_name),
        ));
        return elements;
    }
    let ids = state
        .scene
        .objects
        .iter()
        .map(ObjectSet::id)
        .collect::<Vec<_>>();
    let sets = ids.len();
    let mut cards = Vec::with_capacity(sets);
    for (position, id) in ids.into_iter().enumerate() {
        cards.push(object(ctx, state, id, position, sets));
    }
    let group_gap = ctx.theme().metrics.group_gap;
    elements.push(grana_ui::column().gap(group_gap).children(cards));
    elements
}

fn notes<'a>(
    ctx: &mut BuildContext<'_, Message>,
    notes: &[grana_session::ImportNote],
) -> Element<'a, Message> {
    let shown = notes.len().min(4);
    let language = ctx.language();
    let detail = grana_i18n::object::unread(language, notes.len());
    let mut children = vec![view::card_title(
        ctx,
        Icon::Warning,
        grana_i18n::object::import_title(language),
        Some(&detail),
    )];
    for note in &notes[..shown] {
        children.push(view::labelled(ctx, note.name(), note.detail()));
    }
    if notes.len() > shown {
        children.push(view::labelled(
            ctx,
            "…",
            &grana_i18n::object::more_notes(language, notes.len() - shown),
        ));
    }
    view::card(ctx, 6.0, children)
}

fn header<'a>(
    ctx: &mut BuildContext<'_, Message>,
    count: usize,
    loading: bool,
    authoring: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let total = crate::inspector::panel_width();
    let spots_label = grana_i18n::object::spots(language);
    let inset = (theme.metrics.control_height * 0.44).round() + 1.0;
    let lead_w =
        ctx.measure(
            spots_label,
            TextStyle::new(theme.button_font(), Color::WHITE),
        )
        .x + inset * 2.0
            + 20.0;
    let split_w = if authoring {
        lead_w + theme.metrics.group_gap + theme.metrics.control_height
    } else {
        0.0
    };
    let title_w = (total - split_w - 8.0).max(60.0);
    let label_style = TextStyle::new(theme.font(Level::Overline), palette.on_surface_variant);
    let badge_style = TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant)
        .aligned(Align::Center);
    let count_text = count.to_string();
    let title = grana_ui::canvas(Length::Fixed(title_w), 30.0, move |painter, info| {
        let rect = info.rect;
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 4.0, rect.min.y),
                Vec2::new(rect.max.x - 32.0, rect.max.y),
            ),
            grana_i18n::object::object_sets(language),
            label_style,
            Align::Center,
        );
        if count > 0 {
            let pill = Rect::from_min_size(
                Vec2::new(rect.max.x - 28.0, rect.center().y - 9.5),
                Vec2::new(24.0, 19.0),
            );
            painter.rect(pill, Corners::capsule(), palette.surface_container_high);
            painter.text_in(pill, &count_text, badge_style, Align::Center);
        }
    });
    let lead_msg = if loading {
        Message::Noop
    } else {
        Message::Command(Command::AddObject(ObjectKind::Spots))
    };
    let split = view::split_button(
        ctx,
        Some(Icon::Plus),
        spots_label,
        Role::Filled,
        ("objects.add.spots", lead_msg),
        (Key::new("objects.add"), Message::Noop),
    );
    let mut children = vec![title];
    if authoring {
        children.push(split);
    }
    grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(30.0))
        .gap(8.0)
        .align(Align::Center)
        .children(children)
}

fn add_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
) -> Vec<Element<'a, Message>> {
    let loading = state.io.loading.is_some();
    let language = ctx.language();
    let mut rows = Vec::new();
    for (key, symbol, kind) in [
        ("add.spots", Icon::Target, ObjectKind::Spots),
        ("add.surfaces", Icon::Cube, ObjectKind::Surfaces),
        ("add.filaments", Icon::Channel, ObjectKind::Filaments),
        ("add.cells", Icon::Cube, ObjectKind::Cells),
    ] {
        let message = if loading {
            Message::Noop
        } else {
            Message::Command(Command::AddObject(kind))
        };
        rows.push(action_row(
            ctx,
            key,
            symbol,
            kind_name(language, kind),
            message,
        ));
    }
    let message = if loading {
        Message::Noop
    } else {
        Message::Command(Command::AddSegment)
    };
    let licensed = grana_scene::SegmentModel::preferred();
    let (label, message) = match licensed {
        Some(model) => (
            grana_i18n::object::segments(language, model.license().name()),
            message,
        ),
        None => (
            grana_i18n::object::segments_unlicensed(language).to_string(),
            Message::Noop,
        ),
    };
    rows.push(action_row(
        ctx,
        "add.segments",
        Icon::Sparkles,
        &label,
        message,
    ));
    if !state.labels.is_empty() {
        rows.push(view::menu_separator(ctx));
        rows.push(view::caption(
            ctx,
            grana_i18n::object::import_labels(language),
        ));
        for (index, label) in state.labels.iter().enumerate() {
            let message = if loading {
                Message::Noop
            } else {
                Message::Command(Command::ImportLabels {
                    label: label.clone(),
                })
            };
            rows.push(action_row(
                ctx,
                &format!("add.labels.{index}"),
                Icon::Layers,
                label,
                message,
            ));
        }
    }
    rows.push(view::menu_separator(ctx));
    rows.push(view::caption(
        ctx,
        grana_i18n::object::import_tracks(language),
    ));
    let message = if loading {
        Message::Noop
    } else {
        Message::Command(Command::ImportTracks)
    };
    rows.push(action_row(
        ctx,
        "add.tracks",
        Icon::Target,
        grana_i18n::object::import_geff(language),
        message,
    ));
    let message = if loading {
        Message::Noop
    } else {
        Message::Command(Command::ImportLocalizations)
    };
    rows.push(action_row(
        ctx,
        "add.localizations",
        Icon::Layers,
        grana_i18n::object::import_localizations(language),
        message,
    ));
    vec![view::menu(ctx, Key::new("objects.add"), 192.0, rows)]
}

fn action_row<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    symbol: Icon,
    label: &str,
    message: Message,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    grana_ui::popover::action_row(&theme, symbol, label, None)
        .named(key)
        .press(message)
}

fn kind_icon(kind: ObjectKind) -> Icon {
    match kind {
        ObjectKind::Spots => Icon::Target,
        ObjectKind::Surfaces => Icon::Cube,
        ObjectKind::Filaments => Icon::Channel,
        ObjectKind::Cells => Icon::Cube,
    }
}

fn kind_name(language: Language, kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Spots => grana_i18n::object::spots(language),
        ObjectKind::Surfaces => grana_i18n::object::surfaces(language),
        ObjectKind::Filaments => grana_i18n::object::filaments(language),
        ObjectKind::Cells => grana_i18n::object::cells(language),
    }
}

fn count_noun(language: Language, kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Spots => grana_i18n::object::spots_noun(language),
        ObjectKind::Surfaces => grana_i18n::object::objects_noun(language),
        ObjectKind::Filaments => grana_i18n::object::filaments_noun(language),
        ObjectKind::Cells => grana_i18n::object::cells_noun(language),
    }
}

fn description(language: Language, kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Spots => grana_i18n::object::spots_description(language),
        ObjectKind::Surfaces => grana_i18n::object::surfaces_description(language),
        ObjectKind::Filaments => grana_i18n::object::filaments_description(language),
        ObjectKind::Cells => grana_i18n::object::cells_description(language),
    }
}

fn object<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    position: usize,
    sets: usize,
) -> Element<'a, Message> {
    let Some(set) = state.scene.objects.get(id) else {
        return crate::inspector::hidden();
    };
    let width = width();
    let language = ctx.language();
    let kind = set.kind();
    let merged = set.provenance().is_merged();
    let imported = set.provenance().is_external();
    let imported_root = state
        .scene
        .detection_root(id)
        .and_then(|root| state.scene.objects.get(root))
        .is_some_and(|root| root.provenance().is_external());
    let frames = state.scene.frame_count();
    let time = state.scene.time();
    let detected = set.detected();
    let current = set
        .frame(time)
        .map(|frame| (frame.object_count(), frame.level()));
    let mean = set
        .frame(time)
        .and_then(ObjectFrame::spots)
        .and_then(|spots| spots.mean_quality());
    let expanded = state.workspace.expanded(id);
    let mut children = Vec::new();
    children.push(header_row(ctx, state, id, position, sets));
    if expanded {
        children.push(view::divider(ctx));
        children.extend(appearance_fields(ctx, state, id, width));
        if let Some(element) = origin(ctx, state, id) {
            children.push(element);
        }
        if let Some(element) = attached(ctx, state, id) {
            children.push(element);
        }
        if let Some(element) = curated(ctx, state, id) {
            children.push(element);
        }
        if imported {
            children.extend(importing(ctx, state, id, kind));
        } else if !merged && !imported_root && state.access.author() {
            children.extend(params(ctx, state, id));
            if matches!(kind, ObjectKind::Surfaces | ObjectKind::Cells) {
                children.extend(seeds(ctx, state, id));
            }
            children.push(detect(ctx, state, id, kind, frames));
        }
        children.push(summary(ctx, kind, current, mean, detected, frames, time));
        if let Some(export) = set.label_export() {
            children.push(view::weak(
                ctx,
                &grana_i18n::object::labels_written(
                    language,
                    export.path(),
                    export.labels(),
                    export.frames(),
                ),
            ));
        }
        if state.access.author() {
            children.extend(classifier(ctx, state, id, width));
        }
        children.extend(appearance(ctx, state, id, width));
        if state.access.author() {
            children.extend(relations(ctx, state, id));
        }
        children.extend(children_section(ctx, state, id));
        if state
            .scene
            .objects
            .get(id)
            .is_some_and(|set| set.kind().trackable())
        {
            children.push(crate::track_panel::entry(ctx, state, id));
        }
        children.extend(measure(ctx, state, id, time));
        if detected > 0 && state.access.author() {
            children.push(view::divider(ctx));
            children.push(
                view::button_with(
                    ctx,
                    grana_i18n::object::clear_objects(language),
                    Role::Error,
                    Length::Fill,
                )
                .press(Message::Command(Command::ClearObject(id))),
            );
        }
    }
    view::group_card(ctx, position, sets, expanded, 8.0, children)
}
