use glam::Vec2;
use grana_display::Viewport;
use grana_scene::{Item, Selection};
use grana_session::{Command, SessionView};
use grana_ui::glass::GlassStyle;
use grana_ui::{
    Axis, BuildContext, Corners, Edges, Element, Icon, Key, Layout as UiLayout, Length, Motion,
    Rect, Role, Ui,
};

use crate::message::Message;
use crate::stage::Stage;
use crate::state::{FrameStats, View, Workspace};

mod header;
mod stage;
mod timeline;

pub(crate) use stage::remote;

pub const MARGIN: f32 = 16.0;
pub const GAP: f32 = 12.0;
pub const FORM_LABEL: f32 = 72.0;

pub const TOOLBAR_HEIGHT: f32 = 36.0;
pub const SIDEBAR_WIDTH: f32 = 320.0;
pub const SIDEBAR_PAD: f32 = 14.0;
pub const TRANSPORT_HEIGHT: f32 = 44.0;
pub const DOCK_HEADER: f32 = 36.0;
pub const DOCK_DEFAULT: f32 = 340.0;
pub const DOCK_MINIMUM: f32 = 220.0;
pub const ANALYSIS_WIDTH: f32 = 114.0;
pub const TOOLBAR_LEFT: f32 = 320.0;
pub const TOOLBAR_RIGHT: f32 = 168.0;
pub const STAGE_INSET: f32 = 12.0;
const MINIMUM_STAGE_WIDTH: f32 = 240.0;
const MINIMUM_STAGE_HEIGHT: f32 = 160.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Statistics,
    Tracks,
    Colocalization,
    Relations,
    Annotations,
    Plate,
    Batch,
    Animation,
}

impl Panel {
    pub const ALL: [Panel; 8] = [
        Panel::Statistics,
        Panel::Tracks,
        Panel::Colocalization,
        Panel::Relations,
        Panel::Annotations,
        Panel::Plate,
        Panel::Batch,
        Panel::Animation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Statistics => "Statistics",
            Self::Tracks => "Tracks",
            Self::Colocalization => "Colocalization",
            Self::Relations => "Relations",
            Self::Annotations => "Annotations",
            Self::Plate => "Plate",
            Self::Batch => "Batch",
            Self::Animation => "Animation",
        }
    }
}

pub fn build(
    ui: &mut Ui<Message>,
    actions: Vec<Message>,
    view: &SessionView<'_>,
    viewport: &mut Viewport,
    stage: &mut Stage,
    workspace: &mut Workspace,
    frame: FrameStats,
) -> Vec<Command> {
    let mut commands = Vec::new();
    apply(ui, actions, view, viewport, stage, workspace, &mut commands);
    if let Some(panel) = workspace.panel {
        workspace.last_panel = panel;
    }
    let full = Rect::from_min_size(Vec2::ZERO, ui.size());
    let Some(scene) = view.scene else {
        let palette = crate::palette::resolve(ui, view, workspace);
        let elements = {
            let mut ctx = ui.build();
            let mut elements = crate::shell::view(&mut ctx, view, workspace, full);
            if let Some(palette) = palette.as_ref() {
                elements.extend(crate::palette::view(&mut ctx, full, palette));
            }
            elements
        };
        ui.present(grana_ui::place(full, elements));
        return commands;
    };
    if workspace.panel == Some(Panel::Statistics)
        && view.measure.set().is_none()
        && let Some(set) = scene.objects.iter().find(|set| set.detected() > 0)
    {
        commands.push(Command::SetMeasureSet(Some(set.id())));
    }
    let palette = crate::palette::resolve(ui, view, workspace);
    let state = View {
        scene,
        measure: view.measure,
        timeline: view.timeline,
        jobs: view.jobs,
        io: &view.io,
        batch: view.batch.as_ref(),
        history: view.history,
        series: &view.series,
        plate: view.plate,
        labels: view.labels,
        views: view.views,
        recent: view.recent,
        palette: palette.as_ref(),
        viewport: &*viewport,
        stage: &*stage,
        workspace: &*workspace,
        export: view.export,
        frame,
    };
    let (volume, section, elements) = {
        let mut ctx = ui.build();
        let layout = Layout::new(&mut ctx, &state, full);
        let elements = view_elements(&mut ctx, &layout, &state, full);
        (layout.volume, layout.section, elements)
    };
    ui.present(grana_ui::place(full, elements));
    stage.set_rects(volume, section);
    commands
}

fn view_elements<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
    full: Rect,
) -> Vec<Element<'a, Message>> {
    let mut elements = stage::view(ctx, layout, state, full);
    elements.extend(header::view(ctx, layout, state));
    if layout.sidebar_visible {
        elements.push(sidebar(ctx, layout, state));
    }
    if let Some(rect) = layout.transport {
        elements.push(timeline::transport(ctx, rect, state));
    }
    if let Some(rect) = layout.dock {
        elements.push(dock(ctx, rect, state));
    }
    elements.extend(stage::toast(ctx, layout, state));
    elements.extend(crate::panel::remote(
        ctx,
        layout.stage,
        state.workspace.remote_open,
    ));
    if let Some(palette) = state.palette {
        elements.extend(crate::palette::view(ctx, full, palette));
    }
    elements
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub toolbar_left: Rect,
    pub toolbar_right: Rect,
    pub analysis: Rect,
    pub sidebar: Rect,
    pub stage: Rect,
    pub volume: Rect,
    pub section: Rect,
    pub transport: Option<Rect>,
    pub dock: Option<Rect>,
    pub sidebar_visible: bool,
}

impl Layout {
    fn new(ctx: &mut BuildContext<'_, Message>, state: &View<'_>, full: Rect) -> Self {
        let top = full.min.y + MARGIN;
        let toolbar_left = Rect::from_min_size(
            Vec2::new(full.min.x + MARGIN, top),
            Vec2::new(TOOLBAR_LEFT, TOOLBAR_HEIGHT),
        );
        let analysis = Rect::from_min_size(
            Vec2::new(full.max.x - MARGIN - ANALYSIS_WIDTH, top),
            Vec2::new(ANALYSIS_WIDTH, TOOLBAR_HEIGHT),
        );
        let toolbar_right = Rect::from_min_size(
            Vec2::new(analysis.min.x - 8.0 - TOOLBAR_RIGHT, top),
            Vec2::new(TOOLBAR_RIGHT, TOOLBAR_HEIGHT),
        );
        let sidebar_open = !state.workspace.sidebar_hidden;
        let sidebar_t = ctx.animate(
            Key::new("sidebar.open").id(),
            if sidebar_open { 1.0 } else { 0.0 },
            Motion::snappy(),
        );
        let sidebar_visible = sidebar_t > 0.005;
        let sidebar_slide = (1.0 - sidebar_t) * (SIDEBAR_WIDTH + MARGIN + GAP);
        let sidebar = if sidebar_visible {
            Rect::from_min_max(
                Vec2::new(
                    full.min.x + MARGIN - sidebar_slide,
                    toolbar_left.max.y + GAP,
                ),
                Vec2::new(
                    full.min.x + MARGIN + SIDEBAR_WIDTH - sidebar_slide,
                    full.max.y - MARGIN,
                ),
            )
        } else {
            Rect::ZERO
        };
        let left = full.min.x
            + MARGIN
            + if sidebar_open {
                SIDEBAR_WIDTH + GAP
            } else {
                0.0
            };
        let right = full.max.x - MARGIN;
        let transport_height = if state.scene.frame_count() > 1 {
            TRANSPORT_HEIGHT
        } else {
            0.0
        };
        let transport = (transport_height > 0.0).then(|| {
            Rect::from_min_max(
                Vec2::new(left, full.max.y - MARGIN - transport_height),
                Vec2::new(right, full.max.y - MARGIN),
            )
        });
        let dock_bottom = transport.map_or(full.max.y - MARGIN, |rect| rect.min.y - GAP);
        let dock_open = state.workspace.panel.is_some();
        let dock_t = ctx.animate(
            Key::new("dock.open").id(),
            if dock_open { 1.0 } else { 0.0 },
            Motion::snappy(),
        );
        let dock_top = toolbar_left.max.y + GAP + 120.0;
        let dock_available = (dock_bottom - dock_top).max(DOCK_MINIMUM);
        let dock_stored = if state.workspace.dock_height > 0.0 {
            state.workspace.dock_height
        } else {
            DOCK_DEFAULT
        };
        let dock_height = dock_stored.clamp(DOCK_MINIMUM, dock_available);
        let dock = (dock_t > 0.005).then(|| {
            let height = ctx.animate(
                Key::new("dock.height.spring").id(),
                dock_height,
                Motion::response(0.20, 0.08),
            );
            let slide = (1.0 - dock_t) * (height + MARGIN + GAP * 2.0);
            Rect::from_min_max(
                Vec2::new(left, dock_bottom - height + slide),
                Vec2::new(right, dock_bottom + slide),
            )
        });
        let stage_bottom = if dock_open {
            dock_bottom - dock_height - GAP
        } else {
            dock_bottom
        };
        let stage = Rect::from_min_max(
            Vec2::new(left, toolbar_left.max.y + GAP),
            Vec2::new(
                right.max(left + MINIMUM_STAGE_WIDTH),
                stage_bottom.max(toolbar_left.max.y + GAP + MINIMUM_STAGE_HEIGHT),
            ),
        );
        let split_ratio = ctx.animate(
            Key::new("viewport.split.spring").id(),
            state.viewport.split(),
            Motion::response(0.18, 0.06),
        );
        let split = (stage.min.x + stage.width() * split_ratio).round();
        let volume = Rect::from_min_max(stage.min, Vec2::new(split, stage.max.y));
        let section = Rect::from_min_max(Vec2::new(split, stage.min.y), stage.max);
        Self {
            toolbar_left,
            toolbar_right,
            analysis,
            sidebar,
            stage,
            volume,
            section,
            transport,
            dock,
            sidebar_visible,
        }
    }
}

fn sidebar<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let rect = layout.sidebar;
    let palette = ctx.theme().palette;
    let tabs_rect = Rect::from_min_size(
        Vec2::new(rect.min.x + SIDEBAR_PAD, rect.min.y + SIDEBAR_PAD),
        Vec2::new(rect.width() - SIDEBAR_PAD * 2.0, 28.0),
    );
    let active = state.workspace.sidebar_tab.min(1);
    let tabs = crate::view::segmented_fill(
        ctx,
        "sidebar.tabs",
        &["View", "Objects"],
        active,
        Message::SetSidebarTab,
    )
    .at(tabs_rect);
    let separator_rect = Rect::from_min_size(
        Vec2::new(rect.min.x + SIDEBAR_PAD, tabs_rect.max.y + 8.0),
        Vec2::new(rect.width() - SIDEBAR_PAD * 2.0, 1.0),
    );
    let separator = grana_ui::canvas(Length::Fill, 1.0, move |painter, info| {
        painter.rect(info.rect, Corners::ZERO, palette.separator);
    })
    .at(separator_rect);
    let body = Rect::from_min_max(
        Vec2::new(rect.min.x, tabs_rect.max.y + 9.0),
        Vec2::new(rect.max.x, rect.max.y - 4.0),
    );
    let mut content = vec![crate::inspector::source(ctx, state)];
    match active {
        0 => content.extend(crate::inspector::view_body(ctx, state)),
        _ => content.extend(crate::inspector::objects_body(ctx, state)),
    }
    let column = grana_ui::column()
        .padding(Edges {
            left: SIDEBAR_PAD,
            top: 8.0,
            right: SIDEBAR_PAD,
            bottom: 16.0,
        })
        .gap(8.0)
        .children(content);
    let scroll = Element::new(UiLayout::column())
        .named("sidebar.scroll")
        .at(body)
        .scroll(Axis::Column, Length::Fill, None)
        .clipped()
        .children([column]);
    Element::new(UiLayout::column())
        .named("sidebar")
        .at(rect)
        .glass(GlassStyle::panel())
        .children([tabs, separator, scroll])
}

fn dock<'a>(
    ctx: &mut BuildContext<'_, Message>,
    rect: Rect,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let grip = Rect::from_min_max(rect.min, Vec2::new(rect.max.x, rect.min.y + 10.0));
    let grip_element = grana_ui::canvas(Length::Fill, 10.0, move |painter, info| {
        let engaged = info.hover.max(info.press);
        painter.rect(
            Rect::from_center_size(
                Vec2::new(info.rect.center().x, info.rect.min.y + 5.0),
                Vec2::new(36.0 + 16.0 * engaged, 4.0),
            ),
            Corners::capsule(),
            palette.text_quaternary.lerp(palette.text_tertiary, engaged),
        );
    })
    .at(grip)
    .cursor(grana_ui::Cursor::ResizeVertical)
    .on_pointer(|pointer| {
        (pointer.phase == grana_ui::Phase::Move).then_some(Message::SetDockHeight(-pointer.delta.y))
    });
    let header = Rect::from_min_size(
        rect.min + Vec2::new(16.0, 8.0),
        Vec2::new(rect.width() - 32.0, DOCK_HEADER),
    );
    let close = Rect::from_center_size(
        Vec2::new(header.max.x - 14.0, header.center().y),
        Vec2::splat(28.0),
    );
    let tabs_rect = Rect::from_min_max(header.min, Vec2::new(close.min.x - 12.0, header.max.y));
    let labels = Panel::ALL.map(Panel::name);
    let panel = state.workspace.panel.unwrap_or(state.workspace.last_panel);
    let active = Panel::ALL
        .iter()
        .position(|candidate| *candidate == panel)
        .unwrap_or(0);
    let tabs = crate::view::tabs(ctx, "dock.tabs", &labels, active, |index| {
        Message::SetPanel(Some(Panel::ALL[index]))
    })
    .at(Rect::from_min_size(
        tabs_rect.min,
        Vec2::new(tabs_rect.width(), 32.0),
    ));
    let close_element = crate::view::icon_button(
        ctx,
        "dock.close",
        Icon::Close,
        28.0,
        Role::Quiet,
        "Close panel",
        Message::SetPanel(None),
    )
    .at(close);
    let separator_rect = Rect::from_min_size(
        Vec2::new(rect.min.x + 16.0, header.max.y + 2.0),
        Vec2::new(rect.width() - 32.0, 1.0),
    );
    let separator = grana_ui::canvas(Length::Fill, 1.0, move |painter, info| {
        painter.rect(info.rect, Corners::ZERO, palette.separator);
    })
    .at(separator_rect);
    let content = Rect::from_min_max(
        Vec2::new(rect.min.x + 16.0, header.max.y + 11.0),
        Vec2::new(rect.max.x - 16.0, rect.max.y - 14.0),
    );
    let mut children = vec![grip_element, tabs, close_element, separator];
    match panel {
        Panel::Statistics => children.push(clipped(
            content,
            crate::measure_panel::view(ctx, state, content),
        )),
        Panel::Tracks => children.push(clipped(
            content,
            crate::track_panel::view(ctx, state, content),
        )),
        panel => {
            let body = match panel {
                Panel::Colocalization => crate::coloc_panel::view(ctx, state),
                Panel::Relations => crate::relation_panel::view(ctx, state),
                Panel::Annotations => crate::annotation_panel::view(ctx, state),
                Panel::Plate => crate::plate_panel::view(ctx, state, content.width()),
                Panel::Batch => crate::batch_panel::view(ctx, state),
                Panel::Animation => crate::animation_panel::view(ctx, state),
                Panel::Statistics | Panel::Tracks => {
                    unreachable!("statistics and tracks manage their own layout")
                }
            };
            children.push(
                Element::new(UiLayout::column())
                    .named("dock.scroll")
                    .at(content)
                    .scroll(Axis::Column, Length::Fill, None)
                    .clipped()
                    .children([body]),
            );
        }
    }
    Element::new(UiLayout::column())
        .named("dock")
        .at(rect)
        .glass(GlassStyle::sheet())
        .children(children)
}

fn clipped<'a>(rect: Rect, children: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(UiLayout::column())
        .at(rect)
        .clipped()
        .children(children)
}

pub fn apply(
    ui: &mut Ui<Message>,
    actions: Vec<Message>,
    view: &SessionView<'_>,
    viewport: &mut Viewport,
    stage: &mut Stage,
    workspace: &mut Workspace,
    commands: &mut Vec<Command>,
) {
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
                if matches!(command, Command::Track(_) | Command::CutTrack { .. }) {
                    workspace.join = None;
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
            Message::ToggleSidebar => workspace.sidebar_hidden = !workspace.sidebar_hidden,
            Message::SetDockHeight(value) => {
                let height = if workspace.dock_height > 0.0 {
                    workspace.dock_height
                } else {
                    DOCK_DEFAULT
                };
                workspace.dock_height = (height + value).max(DOCK_MINIMUM);
            }
            Message::SetSplit(value) => viewport.set_split(value),
            Message::SetColumnWidth { index, width } => {
                workspace.column_widths.insert(index, width);
            }
            Message::ToggleOpen(key) => workspace.sections.toggle(key),
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
            Message::SetComparisonMetric(index) => {
                if let Some(metric) = crate::state::ComparisonMetric::ALL.get(index).copied() {
                    workspace.comparison_metric = metric;
                }
            }
            Message::SetComparisonGroup(group) => {
                workspace.comparison_group = group;
                workspace.comparison_key = None;
            }
            Message::SetComparisonKey(key) => workspace.comparison_key = key,
            Message::ToggleComparisonDetail { group, key } => {
                let selected = (group, key);
                workspace.comparison_open = if workspace.comparison_open.as_ref() == Some(&selected)
                {
                    None
                } else {
                    Some(selected)
                };
            }
            Message::SelectPair {
                source,
                target,
                objects,
            } => {
                let first = Selection {
                    set: source,
                    item: Item::Object(objects.0),
                };
                let second = Selection {
                    set: target,
                    item: Item::Object(objects.1),
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
                workspace.join = None;
            }
            Message::SetJoin(join) => workspace.join = join,
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
            Message::SetSectionPreset(index) => {
                let (azimuth, elevation) = crate::section_preset(index);
                viewport.section_mut().set_orientation(azimuth, elevation);
                if let Some(scene) = view.scene {
                    crate::fitting::fit_section(scene, viewport, stage);
                }
            }
            Message::SetSectionProjection(index) => {
                if let Some(projection) = grana_display::Projection::from_index(index) {
                    let section = viewport.section_mut();
                    section.set_projection(projection);
                    if section.projected() && section.thickness() <= 0.0 {
                        section.set_thickness(grana_display::PROJECTION_THICKNESS);
                    }
                }
            }
            Message::SetSectionThickness(value) => {
                let section = viewport.section_mut();
                section.set_thickness(value.clamp(0.0, grana_display::THICKNESS_LIMIT));
            }
            Message::SetSectionOrientation { azimuth, elevation } => {
                viewport.section_mut().set_orientation(azimuth, elevation);
            }
            Message::SetSectionDepth(position) => {
                if let Some(scene) = view.scene {
                    let world = scene.dataset.world_size();
                    let section = viewport.section_mut();
                    let normal = section.normal();
                    let extent = world.abs().dot(normal.abs());
                    let target = position.clamp(0.0, extent) - 0.5 * extent;
                    let center = section.center();
                    section.set_center(center + normal * (target - center.dot(normal)));
                }
            }
            Message::SetPsfThreshold(value) => {
                workspace.psf_threshold = value.clamp(1.0, 64.0);
            }
            Message::SetPsfSeparation(value) => {
                workspace.psf_separation = value.clamp(0.1, 50.0);
            }
            Message::SetExport(export) => {
                commands.push(Command::SetExport(export));
            }
            Message::FitVolume => {
                if let Some(scene) = view.scene {
                    crate::fitting::fit_volume(scene, viewport, stage);
                }
            }
            Message::FitSection => {
                if let Some(scene) = view.scene {
                    crate::fitting::fit_section(scene, viewport, stage);
                }
            }
            Message::Plot(event) => {
                if let Some(scene) = view.scene {
                    crate::plot::apply(event, scene, view.measure, workspace, commands);
                }
            }
            Message::Locate(row) => {
                if let Some(scene) = view.scene {
                    crate::plot::locate(scene, view.measure, row, commands);
                }
            }
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
                let grown = (half[axis] + 0.5 * direction * delta)
                    .clamp(0.5 * minimum, world.length().max(minimum));
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
        }
    }
}
