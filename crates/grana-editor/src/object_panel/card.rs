use super::*;

pub(super) fn header_row<'a>(
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
    let palette = theme.palette;
    let raw = id.raw();
    let name = set.name().to_string();
    let kind = set.kind();
    let channel = set.channel();
    let imported = set.provenance().is_imported();
    let channel_name = state
        .scene
        .detection_root(id)
        .and_then(|root| state.scene.objects.get(root))
        .and_then(|root| root.provenance().label())
        .map_or_else(
            || state.scene.dataset.channels()[channel].name().to_string(),
            str::to_string,
        );
    let tint = Color::from_rgb_f32(set.color());
    let visible = set.visible();
    let detected = set.detected();
    let hidden = set.hidden_count();
    let frames = state.scene.frame_count();
    let time = state.scene.time();
    let current = set
        .frame(time)
        .map(|frame| (frame.object_count(), frame.level()));
    let expanded = state.workspace.expanded(id);
    let width = width();
    let language = ctx.language();
    let name_key = Key::new(format!("object.{raw}.name"));
    let editing = ctx.is_editing(name_key) || state.workspace.renaming == Some(id);
    let arrow = ctx.animate(
        Key::new(format!("object.{raw}.arrow")).id(),
        if expanded { 1.0 } else { 0.0 },
        Motion::snappy(),
    );
    let subtitle = match current {
        Some((count, _level)) => {
            let noun = count_noun(language, kind);
            if frames > 1 {
                grana_i18n::object::header_frames(language, &channel_name, count, noun, detected)
            } else {
                grana_i18n::object::header_single(language, &channel_name, count, noun)
            }
        }
        None if detected > 0 => grana_i18n::object::header_empty(language, &channel_name, time + 1),
        None if imported => grana_i18n::object::ready_import(language, &channel_name),
        None => grana_i18n::object::ready_detect(language, &channel_name),
    };
    let subtitle = if hidden > 0 {
        format!(
            "{subtitle}{}",
            grana_i18n::object::hidden_suffix(language, hidden)
        )
    } else {
        subtitle
    };
    let name_style =
        TextStyle::new(theme.font(Level::Body).medium(), palette.text_primary).truncating();
    let subtitle_style =
        TextStyle::new(TypeStyle::new(10.5, 400.0, 13.0), palette.text_tertiary).truncating();
    let toggle = move || Message::ExpandObject {
        set: id,
        open: !expanded,
    };
    let body = grana_ui::canvas(Length::Fixed(40.0), HEADER, move |painter, info| {
        let rect = info.rect;
        let tile = Rect::from_center_size(
            Vec2::new(rect.min.x + 15.0, rect.center().y),
            Vec2::splat(30.0),
        );
        painter.rect(tile, Corners::all(9.0), tint.with_alpha(0.15));
        painter.outlined(
            tile,
            Corners::all(9.0),
            Fill::None,
            Border {
                width: 1.0,
                color: tint.with_alpha(0.32),
            },
            Shadow::NONE,
        );
        icon::paint(kind_icon(kind), painter, tile.center(), 14.0, tint, 1.5);
    })
    .key(Key::new(format!("object.{raw}.header.icon")))
    .press(toggle());
    let text_width = width - 114.0;
    let text = if editing {
        let current_name = name.clone();
        grana_ui::widget::text_field(&name, name_style, text_width, false)
            .key(name_key)
            .editing(true)
            .on_commit(move |text: &str| {
                let next = text.trim().to_string();
                if next == current_name {
                    Message::EndRename
                } else {
                    Message::RenameObject {
                        set: id,
                        name: next,
                    }
                }
            })
            .on_cancel(|| Message::EndRename)
    } else {
        let title = name.clone();
        grana_ui::canvas(Length::Fixed(text_width), HEADER, move |painter, info| {
            let rect = info.rect;
            painter.text_in(
                Rect::from_min_max(
                    Vec2::new(rect.min.x, rect.min.y + 1.0),
                    Vec2::new(rect.max.x, rect.center().y + 1.0),
                ),
                &title,
                name_style,
                Align::Center,
            );
            painter.text_in(
                Rect::from_min_max(Vec2::new(rect.min.x, rect.center().y + 2.0), rect.max),
                &subtitle,
                subtitle_style,
                Align::Center,
            );
        })
        .key(Key::new(format!("object.{raw}.header.text")))
        .press(toggle())
    };
    let eye = view::icon_button(
        ctx,
        &format!("object.{raw}.visible"),
        if visible { Icon::Eye } else { Icon::EyeOff },
        24.0,
        Role::Quiet,
        if visible {
            grana_i18n::object::hide_set(language)
        } else {
            grana_i18n::object::show_set(language)
        },
        Message::Command(Command::SetObjectVisible {
            set: id,
            visible: !visible,
        }),
    );
    let menu_button = view::icon_button(
        ctx,
        &format!("object.{raw}.menu"),
        Icon::Ellipsis,
        24.0,
        Role::Quiet,
        grana_i18n::object::object_commands(language),
        Message::Noop,
    );
    let chevron = grana_ui::canvas(Length::Fixed(16.0), 24.0, move |painter, info| {
        icon::chevron(
            painter,
            info.rect.center(),
            8.0,
            std::f32::consts::FRAC_PI_2 * arrow,
            palette.text_tertiary,
            1.5,
        );
    })
    .key(Key::new(format!("object.{raw}.arrow.anchor")))
    .press(toggle())
    .focus_ring(Corners::all(8.0))
    .cursor(grana_ui::Cursor::Pointing);
    let rows = object_menu(ctx, state, id, position, sets);
    let menu = view::menu(ctx, Key::new(format!("object.{raw}.menu")), 188.0, rows);
    let gap = grana_ui::canvas(Length::Fixed(6.0), 0.0, |_painter, _info| {});
    let thin = grana_ui::canvas(Length::Fixed(2.0), 0.0, |_painter, _info| {});
    let thin_two = grana_ui::canvas(Length::Fixed(2.0), 0.0, |_painter, _info| {});
    let row = grana_ui::row()
        .width(Length::Fixed(width))
        .height(Length::Fixed(HEADER))
        .align(Align::Center)
        .children([
            body,
            crate::inspector::centered(HEADER, text),
            gap,
            crate::inspector::centered(HEADER, eye),
            thin,
            crate::inspector::centered(HEADER, menu_button),
            thin_two,
            crate::inspector::centered(HEADER, chevron),
        ]);
    grana_ui::column().gap(0.0).children([row, menu])
}

pub(super) fn object_menu<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    position: usize,
    sets: usize,
) -> Vec<Element<'a, Message>> {
    let Some(set) = state.scene.objects.get(id) else {
        return Vec::new();
    };
    let kind = set.kind();
    let language = ctx.language();
    let merges = state
        .scene
        .objects
        .iter()
        .filter(|candidate| candidate.id() != id && candidate.kind() == kind)
        .map(|candidate| (candidate.id(), candidate.name().to_string()))
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    rows.push(action_row(
        ctx,
        &format!("object.{}.menu.rename", id.raw()),
        Icon::Pencil,
        grana_i18n::object::rename(language),
        Message::BeginRename(id),
    ));
    rows.push(action_row(
        ctx,
        &format!("object.{}.menu.duplicate", id.raw()),
        Icon::Layers,
        grana_i18n::object::duplicate(language),
        Message::Command(Command::DuplicateObject(id)),
    ));
    if set.detected() > 0 {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.mask", id.raw()),
            Icon::Channel,
            grana_i18n::object::mask_channel(language),
            Message::Command(Command::AddMaskChannel { set: id }),
        ));
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.coloc", id.raw()),
            Icon::Target,
            grana_i18n::object::colocalize_objects(language),
            Message::Command(Command::ComputeObjectColoc(id)),
        ));
    }
    if view::progress(state.jobs, Job::ObjectColoc(id)).is_some() {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.cancel.coloc", id.raw()),
            Icon::Close,
            grana_i18n::object::cancel_object_coloc(language),
            Message::Command(Command::CancelObjectColoc(id)),
        ));
    }
    if set.object_coloc().is_some() {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.clear.coloc", id.raw()),
            Icon::Reset,
            grana_i18n::object::clear_object_coloc(language),
            Message::Command(Command::ClearObjectColoc(id)),
        ));
    }
    if kind == ObjectKind::Surfaces && set.detected() > 0 && set.provenance().editing().is_none() {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.paint", id.raw()),
            Icon::Pencil,
            grana_i18n::object::paint_mask(language),
            Message::ArmMask(id),
        ));
    }
    if set.provenance().masking().is_some() {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.clear.mask", id.raw()),
            Icon::Reset,
            grana_i18n::object::clear_mask_edits(language),
            Message::Command(Command::ClearMask(id)),
        ));
    }
    if kind != ObjectKind::Spots {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.export.obj", id.raw()),
            Icon::Export,
            grana_i18n::object::export_mesh_obj(language),
            Message::Command(Command::ExportMesh {
                set: id,
                format: MeshFormat::Obj,
            }),
        ));
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.export.stl", id.raw()),
            Icon::Export,
            grana_i18n::object::export_mesh_stl(language),
            Message::Command(Command::ExportMesh {
                set: id,
                format: MeshFormat::Stl,
            }),
        ));
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.export.glb", id.raw()),
            Icon::Export,
            grana_i18n::object::export_mesh_glb(language),
            Message::Command(Command::ExportMesh {
                set: id,
                format: MeshFormat::Glb,
            }),
        ));
    }
    rows.push(action_row(
        ctx,
        &format!("object.{}.menu.labels", id.raw()),
        Icon::Export,
        grana_i18n::object::export_labels(language),
        Message::Command(Command::ExportLabels(id)),
    ));
    if set.kind().trackable() && set.tracks().is_some() {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.geff", id.raw()),
            Icon::Export,
            grana_i18n::object::export_geff(language),
            Message::Command(Command::ExportGeff(id)),
        ));
    }
    if !merges.is_empty() {
        rows.push(view::menu_separator(ctx));
        rows.push(view::caption(ctx, grana_i18n::object::merge_into(language)));
        for (index, (candidate, label)) in merges.iter().enumerate() {
            rows.push(view::menu_row(
                ctx,
                &format!("object.{}.merge.{index}", id.raw()),
                label,
                false,
                Some(Message::Command(Command::MergeObjects {
                    source: id,
                    target: *candidate,
                })),
            ));
        }
    }
    rows.extend(object_edit_rows(ctx, state, id, set));
    rows.push(view::menu_separator(ctx));
    if position > 0 {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.up", id.raw()),
            Icon::ChevronUp,
            grana_i18n::object::move_up(language),
            Message::Command(Command::ReorderObject {
                set: id,
                index: position - 1,
            }),
        ));
    }
    if position + 1 < sets {
        rows.push(action_row(
            ctx,
            &format!("object.{}.menu.down", id.raw()),
            Icon::ChevronDown,
            grana_i18n::object::move_down(language),
            Message::Command(Command::ReorderObject {
                set: id,
                index: position + 1,
            }),
        ));
    }
    rows.push(action_row(
        ctx,
        &format!("object.{}.menu.remove", id.raw()),
        Icon::Trash,
        grana_i18n::object::delete_set(language),
        Message::Command(Command::RemoveObject(id)),
    ));
    rows
}

const MERGE_CANDIDATES: usize = 8;

pub(super) fn object_edit_rows<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    id: ObjectSetId,
    set: &ObjectSet,
) -> Vec<Element<'a, Message>> {
    if set.kind() != ObjectKind::Surfaces {
        return Vec::new();
    }
    let language = ctx.language();
    let mut selected = state
        .scene
        .selections()
        .iter()
        .filter(|selection| selection.set == id)
        .filter_map(|selection| match selection.item {
            Item::Object(index) => Some(index),
            Item::Track(_) => None,
        })
        .collect::<Vec<_>>();
    selected.sort_unstable();
    selected.dedup();
    if selected.is_empty() {
        return Vec::new();
    }
    let time = state.scene.time();
    let Some(ObjectFrame::Surfaces(surfaces)) = set.frame(time) else {
        return Vec::new();
    };
    if !surfaces.cuttable() {
        return Vec::new();
    }
    let mut rows = vec![
        view::menu_separator(ctx),
        view::caption(ctx, grana_i18n::object::edit_objects(language)),
    ];
    if selected.len() > 1 {
        rows.push(view::menu_row(
            ctx,
            &format!("object.{}.edit.merge.selected", id.raw()),
            &grana_i18n::object::merge_selected(language, selected.len()),
            false,
            Some(Message::Command(Command::MergeSurfaces {
                set: id,
                time,
                objects: selected.clone(),
            })),
        ));
        return rows;
    }
    let object = selected[0];
    let centroids = surfaces
        .objects()
        .iter()
        .map(|statistics| statistics.centroid())
        .collect::<Vec<_>>();
    let Some(origin) = centroids.get(object) else {
        return rows;
    };
    let mut neighbours = centroids
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != object)
        .map(|(index, centroid)| (index, squared(origin, centroid)))
        .collect::<Vec<_>>();
    neighbours.sort_by(|left, right| left.1.total_cmp(&right.1));
    for (index, (candidate, _)) in neighbours.iter().take(MERGE_CANDIDATES).enumerate() {
        rows.push(view::menu_row(
            ctx,
            &format!("object.{}.edit.merge.{index}", id.raw()),
            &grana_i18n::object::merge_into_object(language, candidate + 1),
            false,
            Some(Message::Command(Command::MergeSurfaces {
                set: id,
                time,
                objects: vec![object, *candidate],
            })),
        ));
    }
    rows.push(view::menu_row(
        ctx,
        &format!("object.{}.edit.split", id.raw()),
        grana_i18n::object::split_object(language),
        false,
        Some(Message::Command(Command::SplitSurface {
            set: id,
            time,
            object,
        })),
    ));
    rows
}

pub(super) fn squared(left: &[f32; 3], right: &[f32; 3]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| (left - right) * (left - right))
        .sum()
}
