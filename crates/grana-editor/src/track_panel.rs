use glam::Vec2;
use grana_display::Palette;
use grana_i18n::Language;
use grana_measure::{Aggregate, TrackMeasurement};
use grana_object::{Link, TrackSet};
use grana_scene::{Job, ObjectSet, ObjectSetId, RelationField, TrackColor};
use grana_session::Command;
use grana_ui::{
    Align, Axis, BuildContext, Color, Corners, Cursor, Element, Icon, Key, Length, Level, Rect,
    Role, State, TextStyle, TypeStyle, icon,
};

use crate::message::Message;
use crate::panel::Panel;
use crate::state::View;
use crate::view;

const ROW: f32 = 13.0;
const AXIS: f32 = 18.0;
const SIDE: f32 = 10.0;
const INSPECTOR_WIDTH: f32 = 316.0;

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    rect: Rect,
) -> Vec<Element<'a, Message>> {
    let language = ctx.language();
    let scene = state.scene;
    let sets = scene
        .objects
        .iter()
        .filter(|set| set.kind().trackable())
        .map(ObjectSet::id)
        .collect::<Vec<_>>();
    if sets.is_empty() {
        return vec![
            view::empty_state(
                ctx,
                Icon::Channel,
                grana_i18n::track::no_trackable_objects(language),
                grana_i18n::track::no_trackable_objects_detail(language),
            )
            .at(rect),
        ];
    }
    if scene.frame_count() < 2 {
        return vec![
            view::empty_state(
                ctx,
                Icon::Clock,
                grana_i18n::track::single_time_frame(language),
                grana_i18n::track::single_time_frame_detail(language),
            )
            .at(rect),
        ];
    }
    let id = select(state, &sets);
    let Some(object) = scene.objects.get(id) else {
        return Vec::new();
    };
    let selected = scene.selected_track(id);
    let header_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 32.0));
    let mut elements = header(ctx, state, header_rect, &sets, id, object, selected);
    let full = Rect::from_min_max(Vec2::new(rect.min.x, header_rect.max.y + 8.0), rect.max);
    let gap = 12.0;
    let left_width = (full.width() - INSPECTOR_WIDTH - gap).max(260.0);
    let left = Rect::from_min_size(full.min, Vec2::new(left_width, full.height()));
    let right = Rect::from_min_size(
        Vec2::new(left.max.x + gap, full.min.y),
        Vec2::new(
            INSPECTOR_WIDTH
                .min(rect.max.x - left.max.x - gap)
                .max(120.0),
            full.height(),
        ),
    );
    match object.tracks().map(|layer| layer.set()) {
        Some(set) => {
            elements.push(lineage(ctx, state, left, id, object, set, selected));
        }
        None => elements.push(
            view::empty_state(
                ctx,
                Icon::Channel,
                grana_i18n::track::no_trajectories(language),
                grana_i18n::track::no_trajectories_detail(language),
            )
            .at(left),
        ),
    }
    elements.push(inspector(ctx, state, right, id, object, selected));
    elements
}

fn select(state: &View<'_>, sets: &[ObjectSetId]) -> ObjectSetId {
    state
        .workspace
        .track_set
        .filter(|id| sets.contains(id))
        .or_else(|| {
            state
                .scene
                .selection()
                .map(|selection| selection.set)
                .filter(|id| sets.contains(id))
        })
        .unwrap_or(sets[0])
}

pub fn entry<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    set: ObjectSetId,
) -> Element<'a, Message> {
    let language = ctx.language();
    let Some(object) = state.scene.objects.get(set) else {
        return grana_ui::column();
    };
    if !object.kind().trackable() {
        return grana_ui::column();
    }
    let counts = object.tracks().map(|layer| {
        (
            layer.set().len(),
            layer.set().divisions(),
            layer.set().merges(),
        )
    });
    let detail = match counts {
        Some((tracks, divisions, merges)) => {
            grana_i18n::track::track_counts(language, tracks, divisions, merges)
        }
        None => grana_i18n::track::not_tracked_yet(language).to_string(),
    };
    let selected = state.scene.selected_track(set);
    let mut controls = vec![lineage_button(ctx, set)];
    controls.extend(tracking_controls(
        ctx,
        state,
        set,
        object,
        selected,
        &format!("tracks.entry.{}", set.raw()),
    ));
    let mut children = vec![
        view::section_header(ctx, grana_i18n::track::tracking(language), Some(&detail)),
        grana_ui::row()
            .width(Length::Fill)
            .gap(6.0)
            .align(Align::Center)
            .wrap()
            .children(controls),
    ];
    children.extend(parameter_rows(
        ctx,
        state,
        set,
        object,
        &format!("tracks.entry.params.{}", set.raw()),
    ));
    grana_ui::column().gap(6.0).children(children)
}

fn lineage_button<'a>(
    ctx: &mut BuildContext<'_, Message>,
    set: ObjectSetId,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let label = grana_i18n::track::lineage(ctx.language());
    let style = TextStyle::new(theme.button_font(), palette.on_surface);
    let rest = theme.surface(Role::SurfaceHigh, State::Rest);
    let hover = theme.surface(Role::SurfaceHigh, State::Hover);
    grana_ui::canvas(Length::Fixed(78.0), 30.0, move |painter, info| {
        let surface = if info.hover > 0.5 { hover } else { rest };
        painter.surface(info.rect, surface);
        painter.text_in(
            info.rect,
            label,
            style.aligned(Align::Center),
            Align::Center,
        );
    })
    .named(&format!("tracks.entry.lineage.{}", set.raw()))
    .cursor(Cursor::Pointing)
    .on_pointer(move |pointer| match pointer.phase {
        grana_ui::Phase::Down => Some(Message::SetTrackSet(Some(set))),
        grana_ui::Phase::Up => Some(Message::SetPanel(Some(Panel::Tracks))),
        grana_ui::Phase::Move => None,
    })
}

fn tracking_controls<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &View<'_>,
    id: ObjectSetId,
    object: &ObjectSet,
    selected: Option<usize>,
    prefix: &str,
) -> Vec<Element<'a, Message>> {
    let scene = state.scene;
    let tracking = view::progress(state.jobs, Job::Track(id));
    let detected = object.detected();
    let busy = state.io.loading.is_some();
    let time = scene.time();
    let cuts = object.tracks().map(|layer| layer.set()).and_then(|set| {
        let index = selected?;
        let track = set.track(index)?;
        let last = track.nodes().last()?;
        let node = track.nodes().iter().find(|node| node.time() == time)?;
        let division = set.children(index).next().is_some();
        (node.time() != last.time() || division).then_some((node.time(), node.object()))
    });
    let joins = object.tracks().map(|layer| layer.set()).and_then(|set| {
        let index = selected?;
        let track = set.track(index)?;
        let last = track.nodes().last()?;
        (set.children(index).next().is_none() && last.time() + 1 < scene.frame_count())
            .then_some((last.time(), last.object()))
    });
    let joining = state.workspace.join.is_some();
    let language = ctx.language();
    let join = if joining {
        grana_i18n::track::cancel_join(language)
    } else {
        grana_i18n::track::join(language)
    };
    let mut buttons = Vec::new();
    if object.tracks().is_some() {
        buttons.push(enabled_button(
            ctx,
            &format!("{prefix}.cut"),
            grana_i18n::track::cut(language),
            Role::SurfaceHigh,
            cuts.is_some(),
            cuts.map(|(time, object)| {
                Message::Command(Command::CutTrack {
                    set: id,
                    time,
                    object,
                })
            }),
        ));
        buttons.push(enabled_button(
            ctx,
            &format!("{prefix}.join"),
            join,
            if joining {
                Role::Tonal
            } else {
                Role::SurfaceHigh
            },
            joining || joins.is_some(),
            Some(Message::SetJoin(if joining { None } else { joins })),
        ));
        buttons.push(enabled_button(
            ctx,
            &format!("{prefix}.export"),
            grana_i18n::track::export(language),
            Role::SurfaceHigh,
            !busy,
            Some(Message::ExportLineage(id)),
        ));
        buttons.push(enabled_button(
            ctx,
            &format!("{prefix}.recompute"),
            grana_i18n::track::recompute(language),
            Role::SurfaceHigh,
            !busy,
            Some(Message::Command(Command::Track(id))),
        ));
    }
    match tracking {
        Some(tracking) => {
            let fraction = if tracking.total > 0 {
                tracking.done as f32 / tracking.total as f32
            } else {
                0.0
            };
            buttons.push(grana_ui::widget::progress(fraction, 110.0));
            buttons.push(view::spinner(ctx, 16.0));
            buttons.push(enabled_button(
                ctx,
                &format!("{prefix}.cancel"),
                grana_i18n::common::cancel(language),
                Role::SurfaceHigh,
                true,
                Some(Message::Command(Command::CancelTrack(id))),
            ));
        }
        None => buttons.push(enabled_button(
            ctx,
            &format!("{prefix}.track"),
            grana_i18n::track::track(language),
            Role::Filled,
            detected > 0 && !busy,
            Some(Message::Command(Command::Track(id))),
        )),
    }
    buttons
}

fn header<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    rect: Rect,
    sets: &[ObjectSetId],
    id: ObjectSetId,
    object: &ObjectSet,
    selected: Option<usize>,
) -> Vec<Element<'a, Message>> {
    let scene = state.scene;
    let names = sets
        .iter()
        .map(|set| {
            scene
                .objects
                .get(*set)
                .map_or_else(String::new, |set| set.name().to_string())
        })
        .collect::<Vec<_>>();
    let chosen = sets.iter().position(|set| *set == id).unwrap_or(0);
    let tracking = view::progress(state.jobs, Job::Track(id));
    let detected = object.detected();
    let divisions = object.tracks().map_or(0, |layer| layer.set().divisions());
    let merges = object.tracks().map_or(0, |layer| layer.set().merges());
    let tracks = object.tracks().map_or(0, |layer| layer.set().len());
    let joining = state.workspace.join.is_some();
    let language = ctx.language();
    let join = if joining {
        grana_i18n::track::cancel_join(language)
    } else {
        grana_i18n::track::join(language)
    };
    let mut widths = Vec::new();
    if object.tracks().is_some() {
        widths.push(button_width(ctx, grana_i18n::track::cut(language)));
        widths.push(button_width(ctx, join));
        widths.push(button_width(ctx, grana_i18n::track::export(language)));
        widths.push(button_width(ctx, grana_i18n::track::recompute(language)));
    }
    match tracking {
        Some(_) => {
            widths.push(110.0);
            widths.push(16.0);
            widths.push(button_width(ctx, grana_i18n::common::cancel(language)));
        }
        None => widths.push(button_width(ctx, grana_i18n::track::track(language))),
    }
    let right_width = widths.iter().sum::<f32>() + (widths.len() - 1) as f32 * 6.0;
    let right_rect = Rect::from_min_size(
        Vec2::new((rect.max.x - right_width).max(rect.min.x), rect.min.y),
        Vec2::new(right_width.min(rect.width()), rect.height()),
    );
    let left_rect = Rect::from_min_max(
        rect.min,
        Vec2::new((right_rect.min.x - 8.0).max(rect.min.x), rect.max.y),
    );
    let labels = names.clone();
    let set_ids = sets.to_vec();
    let set_combo = combo(
        ctx,
        "tracks.set",
        160.0,
        chosen,
        &labels,
        move |index| match set_ids.get(index) {
            Some(set) if Some(*set) != Some(id) => Message::SetTrackSet(Some(*set)),
            _ => Message::Noop,
        },
    );
    let mut left_items = vec![
        field_label(ctx, grana_i18n::track::set(language)),
        set_combo,
        view::caption(
            ctx,
            &grana_i18n::track::track_summary(language, tracks, divisions, merges, detected),
        ),
    ];
    let badge_tint = ctx.theme().palette.tertiary;
    if joining {
        left_items.push(grana_ui::widget::badge(
            grana_i18n::track::select_join_target(language),
            badge_tint,
        ));
    }
    if state.workspace.divide.is_some() {
        left_items.push(grana_ui::widget::badge(
            grana_i18n::track::select_daughters(language),
            badge_tint,
        ));
    }
    if state.workspace.merge.is_some() {
        let prompt = if state.workspace.merge_parents.is_empty() {
            grana_i18n::track::select_merge_parents(language)
        } else {
            grana_i18n::track::select_merge_parent(language)
        };
        left_items.push(grana_ui::widget::badge(prompt, badge_tint));
    }
    if state.workspace.reparent.is_some() {
        left_items.push(grana_ui::widget::badge(
            grana_i18n::track::select_parent(language),
            badge_tint,
        ));
    }
    let left_row = grana_ui::row()
        .gap(8.0)
        .align(Align::Center)
        .at(left_rect)
        .children(left_items);
    let right_items = tracking_controls(
        ctx,
        state,
        id,
        object,
        selected,
        &format!("tracks.header.{}", id.raw()),
    );
    let right_row = grana_ui::row()
        .gap(6.0)
        .align(Align::Center)
        .justify(Align::End)
        .at(right_rect)
        .children(right_items);
    vec![left_row, right_row]
}

fn enabled_button<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    label: &str,
    role: Role,
    enabled: bool,
    message: Option<Message>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let element =
        grana_ui::widget::button_enabled(&theme, label, role, Length::Auto, enabled).named(key);
    match (enabled, message) {
        (true, Some(message)) => element.press(message),
        _ => element,
    }
}

fn button_width(ctx: &mut BuildContext<'_, Message>, label: &str) -> f32 {
    let theme = *ctx.theme();
    let style = TextStyle::new(theme.button_font(), Color::WHITE);
    ctx.measure(label, style).x + 26.0
}

fn lineage<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    rect: Rect,
    id: ObjectSetId,
    object: &'a ObjectSet,
    set: &'a TrackSet,
    selected: Option<usize>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let accent = palette.primary;
    let tertiary = palette.outline;
    let inner = Rect::from_min_max(
        rect.min + Vec2::new(SIDE, AXIS + 4.0),
        rect.max - Vec2::new(SIDE + 6.0, 6.0),
    );
    let layout = grana_object::TrackLayout::of(set);
    let order = layout.order().to_vec();
    let lane_of = (0..set.len())
        .map(|index| layout.lane(index))
        .collect::<Vec<_>>();
    let content = order.len() as f32 * ROW;
    let limit = (content - inner.height()).max(0.0);
    let key = Key::new("tracks.tree");
    let offset = ctx.scroll_offset(key).clamp(0.0, limit);
    let pointer = ctx.pointer();
    let hovered = inner
        .contains(pointer)
        .then(|| {
            let lane = ((pointer.y - inner.min.y + offset) / ROW).floor();
            (lane >= 0.0)
                .then_some(lane as usize)
                .filter(|lane| *lane < order.len())
        })
        .flatten();
    let frames = state.scene.frame_count();
    let time = state.scene.time();
    let span = frames.max(2) - 1;
    let x = move |time: u32| inner.min.x + (time as f32 / span as f32) * inner.width();
    let lane_y = move |lane: usize| inner.min.y + lane as f32 * ROW + ROW * 0.5 - offset;
    let source = state.workspace.join;
    let divide = state.workspace.divide;
    let merge = state.workspace.merge;
    let merge_parents = state.workspace.merge_parents.clone();
    let reparent = state.workspace.reparent;
    let colors = (0..set.len())
        .map(|index| object.track_color(index))
        .collect::<Vec<_>>();
    let background = grana_ui::canvas(Length::Fill, rect.height(), {
        let order = order.clone();
        move |painter, info| {
            let rect = info.rect;
            painter.rect(
                rect,
                Corners::all(theme.metrics.corner_lg),
                palette.surface_container,
            );
            let clip = painter.clip();
            painter.push_clip(inner, Corners::ZERO);
            for part in 1..4 {
                let grid = x(span * part / 4);
                painter.segment(
                    Vec2::new(grid, inner.min.y),
                    Vec2::new(grid, inner.max.y),
                    0.5,
                    palette.outline_variant.with_alpha(0.35),
                );
            }
            painter.segment(
                Vec2::new(x(time), inner.min.y),
                Vec2::new(x(time), inner.max.y),
                1.2,
                accent.with_alpha(0.55),
            );
            if let Some(lane) = hovered {
                painter.rect(
                    Rect::from_min_size(
                        Vec2::new(inner.min.x, lane_y(lane) - ROW * 0.5),
                        Vec2::new(inner.width(), ROW),
                    ),
                    Corners::capsule(),
                    palette.on_surface.with_alpha(0.06),
                );
            }
            painter.pop_clip(clip);
            if limit > 0.0 {
                let bar = Rect::from_min_size(
                    Vec2::new(rect.max.x - 5.0, inner.min.y),
                    Vec2::new(3.0, inner.height()),
                );
                let thumb = (inner.height() * inner.height() / content).max(24.0);
                let travel = (bar.height() - thumb).max(0.0);
                painter.rect(bar, Corners::capsule(), Color::white_alpha(0.05));
                painter.rect(
                    Rect::from_min_size(
                        Vec2::new(bar.min.x, bar.min.y + travel * offset / limit),
                        Vec2::new(3.0, thumb),
                    ),
                    Corners::capsule(),
                    palette.outline.with_alpha(0.55),
                );
            }
            let label = TextStyle::new(TypeStyle::new(10.0, 500.0, 13.0), tertiary);
            for part in 0..4 {
                let tick = span * part / 3;
                let centre = Vec2::new(x(tick), inner.min.y - AXIS * 0.5);
                let tick_rect = if part == 3 {
                    Rect::from_min_max(
                        centre - Vec2::new(64.0, AXIS * 0.5),
                        centre + Vec2::new(0.0, AXIS * 0.5),
                    )
                } else {
                    Rect::from_center_size(centre, Vec2::new(64.0, AXIS))
                };
                painter.text_in(
                    tick_rect,
                    &format!("F{}", tick + 1),
                    if part == 3 {
                        label.aligned(Align::End)
                    } else {
                        label.aligned(Align::Center)
                    },
                    Align::Center,
                );
            }
            if let (Some(lane), true) = (
                hovered,
                source.is_none() && divide.is_none() && merge.is_none() && reparent.is_none(),
            ) {
                painter.text_in(
                    Rect::from_min_size(
                        Vec2::new(pointer.x + 10.0, lane_y(lane) - ROW * 0.5),
                        Vec2::new(64.0, ROW),
                    ),
                    &format!("#{}", order[lane] + 1),
                    TextStyle::new(TypeStyle::new(10.0, 600.0, 13.0), palette.on_surface),
                    Align::Center,
                );
            }
        }
    })
    .at(rect);
    let click = hovered.and_then(|lane| order.get(lane).copied());
    let select = |index: usize| {
        Some(Message::Command(Command::SelectTrack {
            set: id,
            index: Some(index),
        }))
    };
    let (down_message, up_message) = match click {
        Some(index) => {
            if let Some(parent) = divide {
                let daughters = state.workspace.divide_daughters.as_slice();
                let daughter = set
                    .track(index)
                    .and_then(|track| track.nodes().first())
                    .map(|node| (node.time(), node.object()))
                    .filter(|daughter| {
                        if *daughter == parent {
                            return false;
                        }
                        match daughters {
                            [] => daughter.0 > parent.0,
                            [first] => set.can_divide(parent, *first, *daughter),
                            _ => false,
                        }
                    });
                (
                    daughter.map(|daughter| Message::PickDivide { set: id, daughter }),
                    select(index),
                )
            } else if let Some(child) = merge {
                let parent = set
                    .track(index)
                    .and_then(|track| track.nodes().last())
                    .map(|node| (node.time(), node.object()))
                    .filter(|parent| {
                        parent.0 < child.0 && *parent != child && !merge_parents.contains(parent)
                    });
                (
                    parent.map(|parent| Message::PickMerge { set: id, parent }),
                    select(index),
                )
            } else if let Some(child) = reparent {
                let parent = set
                    .track(index)
                    .and_then(|track| track.nodes().last())
                    .map(|node| (node.time(), node.object()))
                    .filter(|parent| set.can_reparent(child, *parent));
                (
                    parent.map(|parent| Message::PickReparent { set: id, parent }),
                    select(index),
                )
            } else if let Some(source) = source {
                let target = set
                    .track(index)
                    .and_then(|track| track.nodes().first())
                    .map(|node| (node.time(), node.object()))
                    .filter(|target| set.can_join(source, *target));
                match target {
                    Some(target) => (
                        Some(Message::Command(Command::JoinTracks {
                            set: id,
                            source,
                            target,
                        })),
                        Some(Message::SetJoin(None)),
                    ),
                    None => (None, Some(Message::SetJoin(None))),
                }
            } else {
                let target = ((pointer.x - inner.min.x) / inner.width() * span as f32)
                    .clamp(0.0, span as f32)
                    .round() as u32;
                let time = set.track(index).and_then(|track| {
                    track
                        .nodes()
                        .iter()
                        .min_by_key(|node| node.time().abs_diff(target))
                        .map(|node| node.time())
                });
                (
                    Some(Message::Command(Command::SelectTrack {
                        set: id,
                        index: Some(index),
                    })),
                    time.map(|time| Message::Command(Command::SetTime(time))),
                )
            }
        }
        None => (None, None),
    };
    let tracks = grana_ui::canvas(Length::Fill, content.max(1.0), {
        let order = order.clone();
        let lane_of = lane_of.clone();
        move |painter, info| {
            for (lane, index) in order.iter().enumerate() {
                let y = info.rect.min.y + lane as f32 * ROW + ROW * 0.5;
                let Some(track) = set.track(*index) else {
                    continue;
                };
                let start = info.rect.min.x
                    + (track.start() as f32 / frames.max(2).saturating_sub(1) as f32)
                        * info.rect.width();
                let end = info.rect.min.x
                    + (track.end() as f32 / frames.max(2).saturating_sub(1) as f32)
                        * info.rect.width();
                let chosen = selected == Some(*index);
                let tint = if chosen {
                    accent
                } else {
                    Color::from_rgb_f32(colors.get(*index).copied().unwrap_or([0.6, 0.6, 0.6]))
                };
                for link in set.parents(*index) {
                    let parent = link.parent() as usize;
                    let Some(parent_track) = set.track(parent) else {
                        continue;
                    };
                    let parent_end = info.rect.min.x
                        + (parent_track.end() as f32 / frames.max(2).saturating_sub(1) as f32)
                            * info.rect.width();
                    let parent_y = info.rect.min.y + lane_of[parent] as f32 * ROW + ROW * 0.5;
                    draw_link(
                        painter,
                        Vec2::new(parent_end, parent_y),
                        Vec2::new(start, y),
                        link.kind(),
                        tint,
                    );
                }
                painter.segment(
                    Vec2::new(start, y),
                    Vec2::new(end.max(start + 1.0), y),
                    if chosen { 2.6 } else { 1.6 },
                    tint,
                );
                let nodes = track.nodes();
                let spacing = if nodes.len() > 1 {
                    (end - start) / (nodes.len() - 1) as f32
                } else {
                    0.0
                };
                if spacing >= 2.6 {
                    for node in nodes {
                        painter.rect(
                            Rect::from_center_size(
                                Vec2::new(
                                    info.rect.min.x
                                        + (node.time() as f32
                                            / frames.max(2).saturating_sub(1) as f32)
                                            * info.rect.width(),
                                    y,
                                ),
                                Vec2::splat(2.6),
                            ),
                            Corners::capsule(),
                            tint.with_alpha(0.88),
                        );
                    }
                } else if let (Some(first), Some(last)) = (nodes.first(), nodes.last()) {
                    for node in [first, last] {
                        painter.rect(
                            Rect::from_center_size(
                                Vec2::new(
                                    info.rect.min.x
                                        + (node.time() as f32
                                            / frames.max(2).saturating_sub(1) as f32)
                                            * info.rect.width(),
                                    y,
                                ),
                                Vec2::splat(3.0),
                            ),
                            Corners::capsule(),
                            tint,
                        );
                    }
                }
            }
            if let Some(source) = source
                && let Some(track) = set.track_at(source.0, source.1)
            {
                let y = info.rect.min.y + lane_of[track] as f32 * ROW + ROW * 0.5;
                painter.ring(
                    Vec2::new(
                        info.rect.min.x
                            + (source.0 as f32 / frames.max(2).saturating_sub(1) as f32)
                                * info.rect.width(),
                        y,
                    ),
                    4.5,
                    1.4,
                    0.0,
                    std::f32::consts::TAU,
                    accent,
                );
                for (lane, index) in order.iter().enumerate() {
                    let Some(track) = set.track(*index) else {
                        continue;
                    };
                    let Some(first) = track.nodes().first() else {
                        continue;
                    };
                    if set.can_join(source, (first.time(), first.object())) {
                        painter.rect(
                            Rect::from_center_size(
                                Vec2::new(
                                    info.rect.min.x
                                        + (first.time() as f32
                                            / frames.max(2).saturating_sub(1) as f32)
                                            * info.rect.width(),
                                    info.rect.min.y + lane as f32 * ROW + ROW * 0.5,
                                ),
                                Vec2::splat(4.0),
                            ),
                            Corners::capsule(),
                            accent,
                        );
                    }
                }
            }
        }
    })
    .on_pointer(move |pointer| match pointer.phase {
        grana_ui::Phase::Down => down_message.clone(),
        grana_ui::Phase::Up if inner.contains(pointer.position) => up_message.clone(),
        grana_ui::Phase::Move | grana_ui::Phase::Up => None,
    });
    let scroller = Element::new(grana_ui::Layout::column())
        .at(inner)
        .scroll(Axis::Column, Length::Fill, Some(content))
        .clipped()
        .children([tracks]);
    grana_ui::stack(rect, true, [background, scroller])
}

fn inspector<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    rect: Rect,
    id: ObjectSetId,
    object: &'a ObjectSet,
    selected: Option<usize>,
) -> Element<'a, Message> {
    let mut cards = Vec::new();
    if let Some(set) = object.tracks().map(|layer| layer.set()) {
        cards.push(summary(ctx, state, set, selected, id));
    }
    cards.push(parameters(ctx, state, id, object));
    if let Some(card) = appearance(ctx, state, id, object) {
        cards.push(card);
    }
    let column = grana_ui::column().gap(8.0).children(cards);
    Element::new(grana_ui::Layout::column())
        .at(rect)
        .scroll(Axis::Column, Length::Fill, None)
        .clipped()
        .children([column])
}

fn summary<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &View<'_>,
    set: &TrackSet,
    selected: Option<usize>,
    id: ObjectSetId,
) -> Element<'a, Message> {
    let language = ctx.language();
    let Some(index) = selected else {
        let title = view::card_title(
            ctx,
            Icon::Target,
            grana_i18n::track::selected_track(language),
            Some(grana_i18n::common::none(language)),
        );
        let caption = view::caption(ctx, grana_i18n::track::click_track(language));
        return view::card(ctx, 4.0, vec![title, caption]);
    };
    let Some(track) = set.track(index) else {
        return grana_ui::column();
    };
    let statistics = track.statistics(set.frame_interval());
    let parents = set
        .parents(index)
        .map(|link| format!("#{}", link.parent() + 1))
        .collect::<Vec<_>>();
    let parent = if parents.is_empty() {
        grana_i18n::track::root(language).to_string()
    } else {
        parents.join(" · ")
    };
    let children = set.children(index).collect::<Vec<_>>();
    let daughters = if children.is_empty() {
        grana_i18n::common::none(language).to_string()
    } else {
        children
            .iter()
            .map(|child| format!("#{}", child + 1))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let detail = grana_i18n::track::frame_count(language, statistics.frames());
    let title = view::card_title(
        ctx,
        Icon::Target,
        &grana_i18n::track::track_number(language, index + 1),
        Some(&detail),
    );
    let parented = set.parents(index).next().is_some();
    let first = track
        .nodes()
        .first()
        .map(|node| (node.time(), node.object()));
    let last = track
        .nodes()
        .last()
        .map(|node| (node.time(), node.object()));
    let divide = if state.workspace.divide.is_some() {
        grana_i18n::common::cancel(language)
    } else {
        grana_i18n::track::divide(language)
    };
    let reparent = if state.workspace.reparent.is_some() {
        grana_i18n::common::cancel(language)
    } else {
        grana_i18n::track::parent_action(language)
    };
    let merge = if state.workspace.merge.is_some() {
        grana_i18n::track::cancel_merge(language)
    } else {
        grana_i18n::track::merge(language)
    };
    let mut actions = Vec::new();
    if let Some(first) = first
        && set.len() >= 3
    {
        actions.push(enabled_button(
            ctx,
            "tracks.summary.merge",
            merge,
            if state.workspace.merge.is_some() {
                Role::Tonal
            } else {
                Role::SurfaceHigh
            },
            true,
            Some(if state.workspace.merge.is_some() {
                Message::CancelLineage
            } else {
                Message::SetMerge(Some(first))
            }),
        ));
    }
    if let Some(last) = last {
        actions.push(enabled_button(
            ctx,
            "tracks.summary.divide",
            divide,
            if state.workspace.divide.is_some() {
                Role::Tonal
            } else {
                Role::SurfaceHigh
            },
            true,
            Some(if state.workspace.divide.is_some() {
                Message::CancelLineage
            } else {
                Message::SetDivide(Some(last))
            }),
        ));
    }
    if let Some(first) = first {
        actions.push(enabled_button(
            ctx,
            "tracks.summary.parent",
            reparent,
            if state.workspace.reparent.is_some() {
                Role::Tonal
            } else {
                Role::SurfaceHigh
            },
            !parented,
            Some(if state.workspace.reparent.is_some() {
                Message::CancelLineage
            } else {
                Message::SetReparent(Some(first))
            }),
        ));
        actions.push(enabled_button(
            ctx,
            "tracks.summary.detach",
            grana_i18n::track::detach(language),
            Role::Standard,
            parented,
            Some(Message::Command(Command::DetachTrack {
                set: id,
                child: first,
            })),
        ));
    }
    let actions = grana_ui::row().gap(6.0).children(actions);
    let rows = vec![
        title,
        view::labelled(ctx, grana_i18n::track::parent(language), &parent),
        view::labelled(ctx, grana_i18n::track::daughters(language), &daughters),
        view::labelled(
            ctx,
            grana_i18n::track::duration(language),
            &format!("{:.2} s", statistics.duration()),
        ),
        view::labelled(
            ctx,
            grana_i18n::track::length(language),
            &format!("{:.2} µm", statistics.length()),
        ),
        view::labelled(
            ctx,
            grana_i18n::track::displacement(language),
            &format!("{:.2} µm", statistics.displacement()),
        ),
        view::labelled(
            ctx,
            grana_i18n::track::speed(language),
            &format!("{:.3} µm/s", statistics.speed()),
        ),
        view::labelled(
            ctx,
            grana_i18n::track::straightness(language),
            &format!("{:.3}", statistics.straightness()),
        ),
        actions,
    ];
    view::card(ctx, 3.0, rows)
}

fn parameters<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    object: &'a ObjectSet,
) -> Element<'a, Message> {
    let language = ctx.language();
    let mut children = vec![view::card_title(
        ctx,
        Icon::Sliders,
        grana_i18n::track::parameters(language),
        None,
    )];
    children.extend(parameter_rows(ctx, state, id, object, "tracks.panel"));
    view::card(ctx, 6.0, children)
}

fn parameter_rows<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    object: &'a ObjectSet,
    prefix: &str,
) -> Vec<Element<'a, Message>> {
    let Some(style) = object.track_style() else {
        return Vec::new();
    };
    let params = style.params();
    let language = ctx.language();
    let voxel = state.scene.dataset.voxel_size().min_element();
    let world = state.scene.dataset.world_size().max_element();
    let visible = style.visible();
    let thickness = style.thickness();
    let min_thickness = object.min_thickness();
    let max_thickness = object.max_thickness();
    let step = params.max_step();
    let gap = params.max_gap() as f32;
    let division = params.division();
    let range = params.division_range();
    let merge = params.merge();
    let merge_range = params.merge_range();
    let overlap = params.merge_overlap();
    let step_max = (0.25 * world).max(step * 4.0).max(16.0 * voxel);
    let minimum = step;
    let maximum = (4.0 * step).max(minimum);
    let mut children = vec![
        view::slider_readout(
            ctx,
            &format!("{prefix}.step"),
            Some(grana_i18n::track::max_step(language)),
            step,
            (voxel..=step_max, true),
            &format!("{step:.3} µm"),
            move |value| Command::SetTrackParams {
                set: id,
                params: params.with_max_step(value),
            },
        ),
        view::slider_readout(
            ctx,
            &format!("{prefix}.gap"),
            Some(grana_i18n::track::max_gap(language)),
            gap,
            (0.0..=10.0, false),
            &format!("{gap:.0} f"),
            move |value| Command::SetTrackParams {
                set: id,
                params: params.with_max_gap(value.round() as u32),
            },
        ),
        view::toggle_row(
            ctx,
            &format!("{prefix}.division"),
            grana_i18n::track::allow_division(language),
            division,
            move |value| Command::SetTrackParams {
                set: id,
                params: params.with_events(params.events().with_division(value)),
            },
        ),
    ];
    if division {
        let clamped = range.clamp(minimum, maximum);
        children.push(view::slider_readout(
            ctx,
            &format!("{prefix}.division.range"),
            Some(grana_i18n::track::division_range(language)),
            clamped,
            (minimum..=maximum, false),
            &format!("{clamped:.2} µm"),
            move |value| Command::SetTrackParams {
                set: id,
                params: params.with_events(params.events().with_division_range(value.max(minimum))),
            },
        ));
    }
    children.push(view::toggle_row(
        ctx,
        &format!("{prefix}.merge"),
        grana_i18n::track::allow_merge(language),
        merge,
        move |value| Command::SetTrackParams {
            set: id,
            params: params.with_events(params.events().with_merge(value)),
        },
    ));
    if merge {
        let merge_min = voxel.max(1.0e-4);
        let merge_max = (4.0 * step).max(merge_min * 2.0);
        let merge_range = merge_range.clamp(merge_min, merge_max);
        children.push(view::slider_readout(
            ctx,
            &format!("{prefix}.merge.range"),
            Some(grana_i18n::track::merge_range(language)),
            merge_range,
            (merge_min..=merge_max, false),
            &format!("{merge_range:.2} µm"),
            move |value| Command::SetTrackParams {
                set: id,
                params: params.with_events(params.events().with_merge_range(value)),
            },
        ));
        let overlap = overlap.clamp(0.0, 1.0);
        children.push(view::slider_readout(
            ctx,
            &format!("{prefix}.merge.overlap"),
            Some(grana_i18n::track::merge_overlap(language)),
            overlap,
            (0.0..=1.0, false),
            &format!("{:.0}%", overlap * 100.0),
            move |value| Command::SetTrackParams {
                set: id,
                params: params.with_events(params.events().with_merge_overlap(value)),
            },
        ));
    }
    children.push(view::divider(ctx));
    children.push(view::toggle_row(
        ctx,
        &format!("{prefix}.visible"),
        grana_i18n::track::show_tubes(language),
        visible,
        move |value| Command::SetTrackVisible {
            set: id,
            visible: value,
        },
    ));
    children.push(view::slider_readout(
        ctx,
        &format!("{prefix}.width"),
        Some(grana_i18n::track::tube_width(language)),
        thickness,
        (min_thickness..=max_thickness, true),
        &format!("{thickness:.3} µm"),
        move |value| Command::SetTrackThickness {
            set: id,
            thickness: value,
        },
    ));
    children
}

fn appearance<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    object: &'a ObjectSet,
) -> Option<Element<'a, Message>> {
    object.tracks()?;
    let language = ctx.language();
    let mut metrics = object
        .colorable_track_measurements()
        .into_iter()
        .map(|measurement| {
            (
                TrackColor::Measurement(measurement),
                measurement_label(language, measurement),
            )
        })
        .collect::<Vec<_>>();
    for relation in object.relations() {
        let name = state.scene.objects.get(relation.target()).map_or_else(
            || grana_i18n::track::target(language).to_string(),
            |target| target.name().to_string(),
        );
        let shift = match relation.shift() {
            0 => String::new(),
            shift => format!(" t{shift:+}"),
        };
        for field in RelationField::CONTINUOUS {
            let field_text = relation_field_label(language, field);
            for aggregate in Aggregate::ALL {
                metrics.push((
                    TrackColor::Relation {
                        target: relation.target(),
                        field,
                        aggregate,
                        shift: relation.shift(),
                    },
                    grana_i18n::track::relation_metric(
                        language,
                        &name,
                        &field_text,
                        &shift,
                        &aggregate_label(language, aggregate),
                    ),
                ));
            }
        }
    }
    if metrics.is_empty() {
        return None;
    }
    let coloring = object.track_coloring().clone();
    let extent = object.track_extent();
    let mut children = vec![view::card_title(
        ctx,
        Icon::Sparkles,
        grana_i18n::track::tube_coloring(language),
        None,
    )];
    let mut labels = vec![grana_i18n::track::uniform(language).to_string()];
    labels.extend(metrics.iter().map(|(_, label)| label.clone()));
    let selected = coloring
        .metric()
        .and_then(|metric| {
            metrics
                .iter()
                .position(|(candidate, _)| *candidate == metric)
        })
        .map_or(0, |index| index + 1);
    let metrics_for_select = metrics.clone();
    let coloring_for_select = coloring.clone();
    children.push(grana_ui::row().gap(8.0).align(Align::Center).children([
        field_label(ctx, grana_i18n::track::color_by(language)),
        combo(
            ctx,
            &format!("tracks.{}.metric", id.raw()),
            220.0,
            selected,
            &labels,
            move |index| {
                let metric = index
                    .checked_sub(1)
                    .and_then(|index| metrics_for_select.get(index))
                    .map(|(metric, _)| *metric);
                let mut next = coloring_for_select.clone();
                if next.set_metric(metric) {
                    if metric.is_some() {
                        next.set_manual(None);
                    }
                    Message::Command(Command::SetTrackColoring {
                        set: id,
                        coloring: next,
                    })
                } else {
                    Message::Noop
                }
            },
        ),
    ]));
    if let Some(metric) = coloring.metric() {
        let (unit, decimals) = (metric.unit(), metric.decimals());
        let palettes = Palette::ALL
            .iter()
            .map(|palette| palette.name().to_string())
            .collect::<Vec<_>>();
        let palette_index = Palette::ALL
            .iter()
            .position(|palette| *palette == coloring.palette())
            .unwrap_or(0);
        let coloring_for_palette = coloring.clone();
        let coloring_for_reverse = coloring.clone();
        children.push(
            grana_ui::row().gap(8.0).align(Align::Center).children([
                field_label(ctx, grana_i18n::track::palette(language)),
                combo(
                    ctx,
                    &format!("tracks.{}.palette", id.raw()),
                    140.0,
                    palette_index,
                    &palettes,
                    move |index| {
                        let Some(palette) = Palette::ALL.get(index).copied() else {
                            return Message::Noop;
                        };
                        let mut next = coloring_for_palette.clone();
                        if next.set_palette(palette) {
                            Message::Command(Command::SetTrackColoring {
                                set: id,
                                coloring: next,
                            })
                        } else {
                            Message::Noop
                        }
                    },
                ),
                grana_ui::widget::checkbox(
                    ctx.theme(),
                    grana_i18n::track::reverse(language),
                    coloring.reversed(),
                )
                .key(Key::new(format!("tracks.{}.reverse", id.raw())))
                .on_toggle(move |value| {
                    let mut next = coloring_for_reverse.clone();
                    if next.set_reversed(value) {
                        Message::Command(Command::SetTrackColoring {
                            set: id,
                            coloring: next,
                        })
                    } else {
                        Message::Noop
                    }
                }),
            ]),
        );
        let samples = 64;
        let colors = (0..samples)
            .map(|index| {
                let position = index as f32 / (samples - 1) as f32;
                let position = if coloring.reversed() {
                    1.0 - position
                } else {
                    position
                };
                Color::from_rgb_f32(coloring.palette().sample(position))
            })
            .collect::<Vec<_>>();
        children.push(grana_ui::widget::gradient_bar(&colors, 12.0));
        match (coloring.manual(), extent) {
            (Some((minimum, maximum)), _) => {
                let speed = ((maximum - minimum).abs() / 100.0).max(1.0e-6);
                let coloring_low = coloring.clone();
                let coloring_high = coloring.clone();
                let coloring_auto = coloring.clone();
                children.push(
                    grana_ui::row().gap(6.0).align(Align::Center).children([
                        field_label(ctx, grana_i18n::track::range(language)),
                        grana_ui::widget::drag_value(ctx.theme(), minimum, decimals as usize)
                            .key(Key::new(format!("tracks.{}.low", id.raw())))
                            .on_drag_value(speed, move |value| {
                                let mut next = coloring_low.clone();
                                if next.set_manual(Some((value, maximum))) {
                                    Message::Command(Command::SetTrackColoring {
                                        set: id,
                                        coloring: next,
                                    })
                                } else {
                                    Message::Noop
                                }
                            }),
                        grana_ui::widget::drag_value(ctx.theme(), maximum, decimals as usize)
                            .key(Key::new(format!("tracks.{}.high", id.raw())))
                            .on_drag_value(speed, move |value| {
                                let mut next = coloring_high.clone();
                                if next.set_manual(Some((minimum, value))) {
                                    Message::Command(Command::SetTrackColoring {
                                        set: id,
                                        coloring: next,
                                    })
                                } else {
                                    Message::Noop
                                }
                            }),
                        view::button(ctx, grana_i18n::track::auto(language), Role::SurfaceHigh)
                            .press(Message::Command(Command::SetTrackColoring {
                                set: id,
                                coloring: {
                                    let mut next = coloring_auto;
                                    next.set_manual(None);
                                    next
                                },
                            })),
                    ]),
                );
            }
            (None, Some((minimum, maximum))) => {
                let text = grana_i18n::track::auto_range(
                    language,
                    &grana_figure::value(minimum, unit, decimals),
                    &grana_figure::value(maximum, unit, decimals),
                );
                let coloring_fix = coloring.clone();
                children.push(
                    grana_ui::row().gap(6.0).align(Align::Center).children([
                        view::caption(ctx, &text),
                        view::button(
                            ctx,
                            grana_i18n::track::fix_range(language),
                            Role::SurfaceHigh,
                        )
                        .press(Message::Command(
                            Command::SetTrackColoring {
                                set: id,
                                coloring: {
                                    let mut next = coloring_fix;
                                    next.set_manual(Some((minimum, maximum)));
                                    next
                                },
                            },
                        )),
                    ]),
                );
            }
            (None, None) => children.push(view::weak(ctx, grana_i18n::track::no_values(language))),
        }
    }
    Some(view::card(ctx, 6.0, children))
}

fn measurement_label(language: Language, measurement: TrackMeasurement) -> String {
    grana_i18n::measure::term(language, measurement.label())
        .unwrap_or_else(|| measurement.label().to_string())
}

fn relation_field_label(language: Language, field: RelationField) -> String {
    grana_i18n::measure::term(language, field.label()).unwrap_or_else(|| field.label().to_string())
}

fn aggregate_label(language: Language, aggregate: Aggregate) -> String {
    grana_i18n::measure::term(language, aggregate.name())
        .unwrap_or_else(|| aggregate.name().to_string())
}

fn field_label<'a>(ctx: &mut BuildContext<'_, Message>, text: &str) -> Element<'a, Message> {
    let theme = *ctx.theme();
    grana_ui::text(
        text,
        TextStyle::new(theme.label_font(), theme.palette.on_surface_variant),
    )
}

fn combo<'a>(
    ctx: &mut BuildContext<'_, Message>,
    key: &str,
    width: f32,
    selected: usize,
    labels: &[String],
    message: impl Fn(usize) -> Message + 'static,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let label = labels.get(selected).cloned().unwrap_or_default();
    let style = TextStyle::new(theme.font(Level::Label), palette.on_surface).truncating();
    let rest = theme.surface(Role::Field, State::Rest);
    let hover = theme.surface(Role::Field, State::Hover);
    let anchor = grana_ui::canvas(Length::Fixed(width), 30.0, move |painter, info| {
        let surface = if info.hover > 0.5 { hover } else { rest };
        painter.outlined(
            info.rect,
            surface.corners,
            surface.fill,
            surface.border,
            surface.shadow,
        );
        painter.text_in(
            Rect::from_min_max(
                info.rect.min + Vec2::new(10.0, 0.0),
                Vec2::new(info.rect.max.x - 22.0, info.rect.max.y),
            ),
            &label,
            style,
            Align::Center,
        );
        icon::chevron(
            painter,
            Vec2::new(info.rect.max.x - 11.0, info.rect.center().y),
            5.0,
            std::f32::consts::FRAC_PI_2,
            palette.on_surface_variant,
            1.5,
        );
    })
    .named(key)
    .press(Message::Noop)
    .cursor(Cursor::Pointing);
    let mut rows = Vec::with_capacity(labels.len());
    for (index, label) in labels.iter().enumerate() {
        rows.push(
            grana_ui::widget::menu_row(&theme, label, index == selected)
                .key(Key::index(key, index))
                .press(message(index)),
        );
    }
    let popup = grana_ui::popover::popup(Key::new(key), width.max(120.0), rows);
    grana_ui::column()
        .width(Length::Auto)
        .children([anchor, popup])
}

fn draw_link(painter: &mut grana_ui::Painter, from: Vec2, to: Vec2, kind: Link, tint: Color) {
    match kind {
        Link::Gap => dashed(painter, from, to, 1.0, tint.with_alpha(0.35)),
        Link::Division => {
            painter.segment(from, to, 1.2, tint.with_alpha(0.50));
        }
        Link::Merge => {
            painter.segment(from, to, 1.2, tint.with_alpha(0.50));
            painter.ring(from, 2.0, 1.0, 0.0, std::f32::consts::TAU, tint);
        }
    }
}

fn dashed(painter: &mut grana_ui::Painter, from: Vec2, to: Vec2, width: f32, color: Color) {
    let length = from.distance(to);
    if length <= 0.0 {
        return;
    }
    let steps = (length / 5.0).ceil() as u32;
    for step in 0..steps {
        let start = step as f32 / steps as f32;
        let end = (step as f32 + 0.55) / steps as f32;
        painter.segment(from.lerp(to, start), from.lerp(to, end), width, color);
    }
}
