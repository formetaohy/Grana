use grana_session::{Command, SessionView};

use super::{DOCK_DEFAULT, DOCK_MINIMUM, Frame, Panel, Stage};
use crate::message::Message;
use crate::state::Workspace;

pub fn apply(
    ui: &mut grana_ui::Ui<Message>,
    actions: Vec<Message>,
    view: &SessionView<'_>,
    camera: &mut grana_display::Camera,
    stage: &mut Stage,
    workspace: &mut Workspace,
    frame: &mut Frame,
) {
    let commands = &mut frame.commands;
    let mut queue = std::collections::VecDeque::from(actions);
    while let Some(action) = queue.pop_front() {
        match action {
            Message::Noop => {}
            Message::Copy(text) => ui.copy(&text),
            Message::OpenPalette => {
                workspace.palette = match workspace.palette {
                    Some(_) => None,
                    None => Some(crate::state::Palette::default()),
                };
                workspace.palette_actions.clear();
            }
            Message::ClosePalette => {
                workspace.palette = None;
                workspace.palette_actions.clear();
            }
            Message::RunSelected => {
                let selected = workspace
                    .palette
                    .as_ref()
                    .map_or(0, |palette| palette.selected);
                let chosen = workspace.palette_actions.get(selected).cloned();
                workspace.palette = None;
                workspace.palette_actions.clear();
                if let Some(chosen) = chosen {
                    queue.push_front(chosen);
                }
            }
            Message::RunPaletteIndex(index) => {
                let chosen = workspace.palette_actions.get(index).cloned();
                workspace.palette = None;
                workspace.palette_actions.clear();
                if let Some(chosen) = chosen {
                    queue.push_front(chosen);
                }
            }
            Message::Command(command) => {
                let busy = view.io.loading.is_some()
                    && matches!(
                        command,
                        Command::OpenImage
                            | Command::OpenDataset
                            | Command::OpenProject
                            | Command::SaveProject
                            | Command::SaveProjectAs
                            | Command::SaveSnapshot
                    );
                if matches!(
                    command,
                    Command::Track(_)
                        | Command::CutTrack { .. }
                        | Command::JoinTracks { .. }
                        | Command::DivideTracks { .. }
                        | Command::MergeTracks { .. }
                        | Command::ReparentTrack { .. }
                        | Command::DetachTrack { .. }
                ) {
                    workspace.cancel_lineage();
                }
                if !busy {
                    commands.push(command);
                }
            }
            Message::SetPanel(panel) => {
                workspace.panel = panel;
                if let Some(panel) = panel {
                    workspace.last_panel = panel;
                }
            }
            Message::SetSidebarTab(index) => workspace.sidebar_tab = index,
            Message::SetLanguage(language) => workspace.language = language,
            Message::ToggleSidebar => workspace.sidebar_hidden = !workspace.sidebar_hidden,
            Message::SetDockHeight(value) => {
                let height = if workspace.dock_height > 0.0 {
                    workspace.dock_height
                } else {
                    DOCK_DEFAULT
                };
                workspace.dock_height = (height + value).max(DOCK_MINIMUM);
            }
            Message::SetColumnWidth { index, width } => {
                workspace.column_widths.insert(index, width);
            }
            Message::ToggleOpen(key) => workspace.disclosures.toggle(key),
            Message::ExpandObject { set, open } => {
                if open {
                    workspace.expanded.insert(set);
                } else {
                    workspace.expanded.remove(&set);
                }
            }
            Message::BeginRename(set) => workspace.renaming = Some(set),
            Message::EndRename => workspace.renaming = None,
            Message::RenameObject { set, name } => {
                commands.push(Command::RenameObject { set, name });
                workspace.renaming = None;
            }
            Message::SetRelation {
                source,
                target,
                shift,
            } => workspace.relation = Some((source, target, shift)),
            Message::OpenRelation {
                source,
                target,
                shift,
            } => {
                workspace.relation = Some((source, target, shift));
                workspace.panel = Some(Panel::Relations);
                workspace.last_panel = Panel::Relations;
            }
            Message::SetRelationMetric(index) => {
                if let Some(metric) = crate::state::RelationMetric::ALL.get(index).copied() {
                    workspace.relation_metric = metric;
                    workspace.relation_filter = 0.0;
                }
            }
            Message::SetRelationFilter(value) => {
                workspace.relation_filter = value.clamp(0.0, 1.0);
            }
            Message::SetDistributionStyle(index) => {
                if let Some(style) = crate::state::DistributionStyle::ALL.get(index) {
                    workspace.distribution_style = *style;
                }
            }
            Message::SetComparisonMetric(key) => {
                workspace.comparison_metric = Some(key);
            }
            Message::SetComparisonAxis(index) => {
                if let Some(axis) = crate::state::ComparisonAxis::ALL.get(index).copied() {
                    workspace.comparison_axis = axis;
                    workspace.comparison_open = None;
                }
            }
            Message::SetComparisonGroup(group) => {
                workspace.comparison_group = group;
                workspace.comparison_key = None;
            }
            Message::SetComparisonKey(key) => workspace.comparison_key = key,
            Message::ToggleComparisonDetail(index) => {
                if let Some(slot) = workspace.batch_chart.slots.get(index).cloned() {
                    workspace.comparison_open = if workspace.comparison_open.as_ref() == Some(&slot)
                    {
                        None
                    } else {
                        Some(slot)
                    };
                }
            }
            Message::SelectPair {
                source,
                target,
                objects,
            } => {
                let first = grana_scene::Selection {
                    set: source,
                    item: grana_scene::Item::Object(objects.0),
                };
                let second = grana_scene::Selection {
                    set: target,
                    item: grana_scene::Item::Object(objects.1),
                };
                commands.push(Command::Select(Some(first)));
                if first != second {
                    commands.push(Command::ToggleSelect(second));
                }
            }
            Message::BeginRemote => workspace.remote_open = true,
            Message::CancelRemote => workspace.remote_open = false,
            Message::OpenRemote(url) => {
                workspace.remote_open = false;
                if view.io.loading.is_none() {
                    commands.push(Command::OpenRemote(url));
                }
            }
            Message::SetPlaying(playing) => {
                workspace.playback.playing = playing;
                if playing {
                    workspace.playback.previewing = false;
                }
            }
            Message::SetPreviewing(previewing) => {
                workspace.playback.previewing = previewing;
                if previewing {
                    workspace.playback.playing = false;
                }
            }
            Message::SetPlaybackPosition(position) => workspace.playback.position = position,
            Message::SetPlaybackSelected(selected) => workspace.playback.selected = selected,
            Message::SetTrackSet(set) => {
                workspace.track_set = set;
                workspace.cancel_lineage();
            }
            Message::ExportLineage(set) => {
                workspace.track_set = Some(set);
                commands.push(Command::SaveLineage);
            }
            Message::SetJoin(join) => {
                workspace.cancel_lineage();
                workspace.join = join;
            }
            Message::SetDivide(parent) => {
                workspace.cancel_lineage();
                workspace.divide = parent;
            }
            Message::PickDivide { set, daughter } => {
                let Some(parent) = workspace.divide else {
                    continue;
                };
                if daughter == parent || workspace.divide_daughters.contains(&daughter) {
                    continue;
                }
                workspace.divide_daughters.push(daughter);
                if workspace.divide_daughters.len() == 2 {
                    let first = workspace.divide_daughters[0];
                    let second = workspace.divide_daughters[1];
                    commands.push(Command::DivideTracks {
                        set,
                        parent,
                        first,
                        second,
                    });
                    workspace.cancel_lineage();
                }
            }
            Message::SetMerge(child) => {
                workspace.cancel_lineage();
                workspace.merge = child;
            }
            Message::PickMerge { set, parent } => {
                let Some(child) = workspace.merge else {
                    continue;
                };
                if parent == child
                    || parent.0 >= child.0
                    || workspace.merge_parents.contains(&parent)
                {
                    continue;
                }
                workspace.merge_parents.push(parent);
                if workspace.merge_parents.len() == 2 {
                    let first = workspace.merge_parents[0];
                    let second = workspace.merge_parents[1];
                    commands.push(Command::MergeTracks {
                        set,
                        first,
                        second,
                        child,
                    });
                    workspace.cancel_lineage();
                }
            }
            Message::SetReparent(child) => {
                workspace.cancel_lineage();
                workspace.reparent = child;
            }
            Message::PickReparent { set, parent } => {
                let Some(child) = workspace.reparent.take() else {
                    continue;
                };
                workspace.divide_daughters.clear();
                commands.push(Command::ReparentTrack { set, child, parent });
            }
            Message::CancelLineage => workspace.cancel_lineage(),
            Message::SetSeed(seed) => {
                workspace.seed = seed;
                if seed.is_some() {
                    workspace.annotation_tool = None;
                    workspace.annotation_pending = None;
                    workspace.mask = None;
                }
            }
            Message::ArmMask(set) => {
                let radius = view.scene.map_or(1.0, |scene| {
                    (3.0 * scene.dataset.voxel_size().min_element()).max(0.1)
                });
                workspace.mask = Some(crate::state::MaskBrush {
                    set,
                    tool: crate::state::MaskBrushTool::Brush,
                    radius,
                    tolerance: 0.05,
                });
                workspace.seed = None;
                workspace.annotation_tool = None;
                workspace.annotation_pending = None;
            }
            Message::DisarmMask => workspace.mask = None,
            Message::SetMaskTool(index) => {
                if let Some(brush) = workspace.mask.as_mut()
                    && let Some(tool) = crate::state::MaskBrushTool::ALL.get(index).copied()
                {
                    brush.tool = tool;
                }
            }
            Message::SetMaskRadius(value) => {
                if let Some(brush) = workspace.mask.as_mut() {
                    brush.radius = value.clamp(0.05, 50.0);
                }
            }
            Message::SetMaskTolerance(value) => {
                if let Some(brush) = workspace.mask.as_mut() {
                    brush.tolerance = value.clamp(0.0, 1.0);
                }
            }
            Message::SetAnnotationTool(tool) => {
                if workspace.annotation_tool != tool {
                    workspace.annotation_tool = tool;
                    workspace.annotation_pending = None;
                    if tool.is_some() {
                        workspace.seed = None;
                        workspace.mask = None;
                    }
                }
            }
            Message::SetClassifierFeatures { set, features } => {
                workspace.classifier.insert(set, features);
            }
            Message::FitView => {
                if let Some(scene) = view.scene {
                    crate::fitting::fit_camera(scene, camera, stage.aspect());
                }
            }
            Message::SetCameraProjection(index) => {
                if let Some(projection) = grana_display::CameraProjection::from_index(index) {
                    camera.set_projection(projection);
                }
            }
            Message::ToggleSlice(enabled) => {
                if let Some(scene) = view.scene {
                    if enabled {
                        let axis = camera.slice().map_or(2, |slice| slice.axis());
                        crate::fitting::enter_slice(scene, camera, axis, stage.aspect());
                    } else {
                        camera.clear_slice();
                    }
                }
            }
            Message::SetSliceAxis(axis) => {
                if let Some(scene) = view.scene
                    && axis < 3
                {
                    crate::fitting::enter_slice(scene, camera, axis, stage.aspect());
                }
            }
            Message::SetSliceDepth(at) => {
                if let Some(scene) = view.scene {
                    crate::fitting::set_slice_depth(scene, camera, at);
                }
            }
            Message::StepSlice(steps) => {
                if let Some(scene) = view.scene {
                    crate::fitting::slice_step(scene, camera, steps);
                }
            }
            Message::SetAperture(value) => {
                camera.set_aperture(value.clamp(0.0, grana_display::APERTURE_LIMIT));
            }
            Message::SetFocus(value) => {
                camera.set_focus(value.clamp(1.0e-3, grana_display::APERTURE_LIMIT));
            }
            Message::FocusCamera => camera.focus_at_pivot(),
            Message::SetPsfThreshold(value) => {
                workspace.psf_threshold = value.clamp(1.0, 64.0);
            }
            Message::SetPsfSeparation(value) => {
                workspace.psf_separation = value.clamp(0.1, 50.0);
            }
            Message::SetExport(export) => {
                commands.push(Command::SetExport(export));
            }
            Message::Plot(event) => crate::plot::apply(event, view.measure, workspace, commands),
            Message::Locate(row) => crate::plot::locate(view.measure, row, commands),
            Message::ClearGates => crate::plot::clear_gates(view.measure, commands),
            Message::Region { axis, side, delta } => {
                let Some(scene) = view.scene else {
                    continue;
                };
                let current = scene.region;
                let world = scene.dataset.world_size();
                let minimum = (1.0e-3 * world.min_element()).max(1.0e-5);
                let direction = if side == 0 { -1.0 } else { 1.0 };
                let half = current.half();
                let reach = 0.5 * world.abs().dot(current.axis(axis).abs()).max(minimum);
                let grown = (half[axis] + 0.5 * direction * delta).clamp(0.5 * minimum, reach);
                if grown != half[axis] {
                    let mut region = current;
                    let mut sizes = half;
                    sizes[axis] = grown;
                    let center =
                        current.center() + current.axis(axis) * (direction * (grown - half[axis]));
                    region.set_center(center.clamp(-0.5 * world, 0.5 * world));
                    region.set_half(sizes);
                    commands.push(Command::SetRegion(region));
                }
            }
            Message::Window(action) => frame.window.push(action),
        }
    }
}
