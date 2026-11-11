use glam::Vec2;
use grana_i18n::Language;
use grana_measure::{ChildField, ColocMetric, Group, Measurement};
use grana_scene::{
    IntensitySource, Item, Job, ObjectColor, ObjectFrame, ObjectKind, ObjectSet, ObjectSetId,
    ParamField, ParamScale, Provenance, RelationField, SEGMENT_FIELDS, Scene,
};
use grana_session::{Command, MeshFormat};
use grana_ui::{
    Align, Border, BuildContext, Color, Corners, Edges, Element, Fill, Icon, Key, Length, Level,
    Motion, Rect, Role, Shadow, TextStyle, TypeStyle, icon,
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

const PAD: f32 = 11.0;

fn section_font() -> TypeStyle {
    TypeStyle {
        size: 10.0,
        weight: 600.0,
        tracking: 0.7,
        leading: 14.0,
    }
}

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
    let mut elements = add_menu(ctx, state);
    elements.insert(0, header(ctx, count));
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
    for (position, id) in ids.into_iter().enumerate() {
        elements.push(object(ctx, state, id, position, sets));
    }
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

fn header<'a>(ctx: &mut BuildContext<'_, Message>, count: usize) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let total = crate::inspector::panel_width();
    let label_style = TextStyle::new(section_font(), palette.text_tertiary);
    let detail_style =
        TextStyle::new(theme.font(Level::Caption), palette.text_tertiary).aligned(Align::End);
    let detail = count.to_string();
    let language = ctx.language();
    let canvas = grana_ui::canvas(Length::Fixed(total - 24.0), 26.0, move |painter, info| {
        let rect = info.rect;
        painter.text_in(
            rect,
            grana_i18n::object::object_sets(language),
            label_style,
            Align::Center,
        );
        painter.text_in(
            Rect::from_min_max(rect.min, Vec2::new(rect.max.x - 8.0, rect.max.y)),
            &detail,
            detail_style,
            Align::Center,
        );
    });
    let add = view::icon_button(
        ctx,
        "objects.add",
        Icon::Plus,
        24.0,
        Role::Quiet,
        grana_i18n::object::add_object_set(language),
        Message::Noop,
    );
    grana_ui::row()
        .width(Length::Fill)
        .height(Length::Fixed(26.0))
        .align(Align::Center)
        .children([canvas, add])
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
    vec![view::menu(ctx, Key::new("objects.add"), 184.0, rows)]
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
    }
}

fn kind_name(language: Language, kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Spots => grana_i18n::object::spots(language),
        ObjectKind::Surfaces => grana_i18n::object::surfaces(language),
        ObjectKind::Filaments => grana_i18n::object::filaments(language),
    }
}

fn count_noun(language: Language, kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Spots => grana_i18n::object::spots_noun(language),
        ObjectKind::Surfaces => grana_i18n::object::objects_noun(language),
        ObjectKind::Filaments => grana_i18n::object::filaments_noun(language),
    }
}

fn description(language: Language, kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Spots => grana_i18n::object::spots_description(language),
        ObjectKind::Surfaces => grana_i18n::object::surfaces_description(language),
        ObjectKind::Filaments => grana_i18n::object::filaments_description(language),
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
    let theme = *ctx.theme();
    let look = theme.surface(Role::Card, grana_ui::State::Rest);
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
        } else if !merged && !imported_root {
            children.extend(params(ctx, state, id));
            if kind == ObjectKind::Surfaces {
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
        children.extend(classifier(ctx, state, id, width));
        children.extend(appearance(ctx, state, id, width));
        children.extend(relations(ctx, state, id));
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
        if detected > 0 {
            children.push(view::divider(ctx));
            children.push(
                view::button_with(
                    ctx,
                    grana_i18n::object::clear_objects(language),
                    Role::Danger,
                    Length::Fill,
                )
                .press(Message::Command(Command::ClearObject(id))),
            );
        }
    }
    grana_ui::column()
        .padding(Edges::all(PAD))
        .gap(7.0)
        .background(look)
        .children(children)
}
