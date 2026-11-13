use glam::Vec2;
use grana_session::{Command, SessionView};
use grana_ui::{
    Align, Axis, BuildContext, Corners, Edges, Element, Icon, Key, Layout as UiLayout, Length,
    Level, Motion, Rect, Role, State, TextStyle,
};

use crate::message::Message;
use crate::stage::Stage;
use crate::state::{View, Workspace};

mod header;
mod stage;
mod timeline;

pub(crate) use stage::remote;

pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 8.0;
pub const FORM_LABEL: f32 = 70.0;

pub const TOOLBAR_HEIGHT: f32 = 42.0;
pub const SIDEBAR_WIDTH: f32 = 336.0;
pub const SIDEBAR_PAD: f32 = 12.0;
pub const TRANSPORT_HEIGHT: f32 = 44.0;
pub const DOCK_HEADER: f32 = 36.0;
pub const DOCK_DEFAULT: f32 = 332.0;
pub const DOCK_MINIMUM: f32 = 210.0;
pub const TOOLBAR_LEFT: f32 = 336.0;
pub const TOOLBAR_RIGHT: f32 = 336.0;
pub const STAGE_INSET: f32 = 12.0;

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

    pub const VIEWED: [Panel; 7] = [
        Panel::Statistics,
        Panel::Tracks,
        Panel::Colocalization,
        Panel::Relations,
        Panel::Annotations,
        Panel::Plate,
        Panel::Animation,
    ];

    pub fn available(authoring: bool) -> &'static [Panel] {
        if authoring {
            &Panel::ALL
        } else {
            &Panel::VIEWED
        }
    }

    pub fn name(self, language: grana_i18n::Language) -> &'static str {
        match self {
            Self::Statistics => grana_i18n::panel::statistics(language),
            Self::Tracks => grana_i18n::panel::tracks(language),
            Self::Colocalization => grana_i18n::panel::colocalization(language),
            Self::Relations => grana_i18n::panel::relations(language),
            Self::Annotations => grana_i18n::panel::annotations(language),
            Self::Plate => grana_i18n::panel::plate(language),
            Self::Batch => grana_i18n::panel::batch(language),
            Self::Animation => grana_i18n::panel::animation(language),
        }
    }
}

#[derive(Default)]
pub struct Frame {
    pub commands: Vec<Command>,
    pub window: Vec<crate::chrome::WindowAction>,
}

pub fn build(
    ui: &mut grana_ui::Ui<Message>,
    actions: Vec<Message>,
    view: &SessionView<'_>,
    camera: &mut grana_display::Camera,
    stage: &mut Stage,
    workspace: &mut Workspace,
) -> Frame {
    let mut frame = Frame::default();
    apply(ui, actions, view, camera, stage, workspace, &mut frame);
    ui.set_language(workspace.language);
    if let Some(panel) = workspace.panel {
        workspace.last_panel = panel;
    }
    let full = Rect::from_min_size(Vec2::ZERO, ui.size());
    stage.set_viewport(full);
    let Some(scene) = view.scene else {
        let palette = crate::palette::resolve(ui, view, workspace);
        let elements = {
            let mut ctx = ui.build();
            let mut elements = crate::shell::view(&mut ctx, view, workspace, full);
            elements.extend(crate::chrome::view(&mut ctx, full, workspace.chrome));
            if let Some(palette) = palette.as_ref() {
                elements.extend(crate::palette::view(&mut ctx, full, palette));
            }
            elements
        };
        ui.present(grana_ui::place(full, elements));
        return frame;
    };
    if workspace.panel == Some(Panel::Statistics)
        && view.measure.set().is_none()
        && let Some(set) = scene.objects.iter().find(|set| set.detected() > 0)
    {
        frame.commands.push(Command::SetMeasureSet(Some(set.id())));
    }
    let palette = crate::palette::resolve(ui, view, workspace);
    let state = View {
        access: view.access,
        scene,
        measure: view.measure,
        measuring: view.measuring,
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
        import: view.import,
        palette: palette.as_ref(),
        camera: &*camera,
        stage: &*stage,
        workspace: &*workspace,
        export: view.export,
    };
    let elements = {
        let mut ctx = ui.build();
        let layout = Layout::new(&mut ctx, &state, full);
        view_elements(&mut ctx, &layout, &state, full)
    };
    ui.present(grana_ui::place(full, elements));
    frame
}

fn view_elements<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
    full: Rect,
) -> Vec<Element<'a, Message>> {
    let mut elements = stage::view(ctx, layout, state, full);
    elements.extend(crate::chrome::view(ctx, full, state.workspace.chrome));
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
        layout.viewport,
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
    pub sidebar: Rect,
    pub viewport: Rect,
    pub left: f32,
    pub right: f32,
    pub transport: Option<Rect>,
    pub dock: Option<Rect>,
    pub sidebar_visible: bool,
}

impl Layout {
    fn new(ctx: &mut BuildContext<'_, Message>, state: &View<'_>, full: Rect) -> Self {
        let chrome = state.workspace.chrome;
        let top = full.min.y + MARGIN;
        let toolbar_left = Rect::from_min_size(
            Vec2::new(full.min.x + MARGIN + chrome.leading, top),
            Vec2::new(TOOLBAR_LEFT, TOOLBAR_HEIGHT),
        );
        let controls = if chrome.controls {
            crate::chrome::CONTROLS_WIDTH + GAP
        } else {
            0.0
        };
        let toolbar_right = Rect::from_min_size(
            Vec2::new(full.max.x - MARGIN - controls - TOOLBAR_RIGHT, top),
            Vec2::new(TOOLBAR_RIGHT, TOOLBAR_HEIGHT),
        );
        let sidebar_open = !state.workspace.sidebar_hidden;
        let sidebar_t = ctx.animate(
            Key::new("sidebar.open").id(),
            if sidebar_open { 1.0 } else { 0.0 },
            Motion::spatial_default(),
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
            Motion::spatial_default(),
        );
        let dock_top = toolbar_left.max.y + GAP + 100.0;
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
                Motion::spatial_fast(),
            );
            let slide = (1.0 - dock_t) * (height + MARGIN + GAP * 2.0);
            Rect::from_min_max(
                Vec2::new(left, dock_bottom - height + slide),
                Vec2::new(right, dock_bottom + slide),
            )
        });
        Self {
            toolbar_left,
            toolbar_right,
            sidebar,
            viewport: full,
            left,
            right,
            transport,
            dock,
            sidebar_visible,
        }
    }

    pub fn overlay_top(&self) -> f32 {
        self.toolbar_left.max.y + GAP
    }

    pub fn content(&self) -> Rect {
        Rect::from_min_max(
            Vec2::new(self.left, self.overlay_top() + GAP),
            Vec2::new(self.right, self.floor()),
        )
    }

    pub fn floor(&self) -> f32 {
        let transport = self
            .transport
            .map_or(self.viewport.max.y - MARGIN, |rect| rect.min.y - GAP);
        self.dock.map_or(transport, |rect| rect.min.y - GAP)
    }
}

fn sidebar<'a>(
    ctx: &mut BuildContext<'_, Message>,
    layout: &Layout,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let rect = layout.sidebar;
    let theme = *ctx.theme();
    let palette = theme.palette;
    let tabs_rect = Rect::from_min_size(
        Vec2::new(rect.min.x + SIDEBAR_PAD, rect.min.y + SIDEBAR_PAD),
        Vec2::new(
            rect.width() - SIDEBAR_PAD * 2.0,
            theme.metrics.control_height,
        ),
    );
    let active = state.workspace.sidebar_tab.min(1);
    let language = ctx.language();
    let objects_count = state.scene.objects.len();
    let objects_label = if objects_count > 0 {
        format!(
            "{} · {objects_count}",
            grana_i18n::panel::objects_tab(language)
        )
    } else {
        grana_i18n::panel::objects_tab(language).to_string()
    };
    let tabs = crate::view::segmented_fill(
        ctx,
        "sidebar.tabs",
        &[grana_i18n::panel::view_tab(language), &objects_label],
        active,
        Message::SetSidebarTab,
    )
    .at(tabs_rect);
    let summary_rect = Rect::from_min_size(
        Vec2::new(rect.min.x + SIDEBAR_PAD, tabs_rect.max.y + 6.0),
        Vec2::new(rect.width() - SIDEBAR_PAD * 2.0, 26.0),
    );
    let dataset = &state.scene.dataset;
    let extent = dataset.extent();
    let channels = dataset.channels().len();
    let frames = dataset.frame_count();
    let dims = if frames > 1 {
        format!(
            "{}×{}×{} · {}C · {}T",
            extent.x(),
            extent.y(),
            extent.z(),
            channels,
            frames
        )
    } else {
        format!(
            "{}×{}×{} · {}C",
            extent.x(),
            extent.y(),
            extent.z(),
            channels
        )
    };
    let voxel = dataset.voxel_size();
    let spacing = format!("{:.2}×{:.2} µm", voxel.x, voxel.z);
    let mono = theme.font(Level::Monospace);
    let summary = grana_ui::canvas(Length::Fill, 26.0, move |painter, info| {
        painter.rect(
            info.rect,
            Corners::all(theme.metrics.corner_sm),
            palette.surface_container,
        );
        let inner = info.rect.shrink2(Vec2::new(10.0, 0.0));
        painter.text_in(
            inner,
            &dims,
            TextStyle::new(mono, palette.on_surface_variant).truncating(),
            Align::Center,
        );
        painter.text_in(
            inner,
            &spacing,
            TextStyle::new(mono, palette.primary).aligned(Align::End),
            Align::Center,
        );
    })
    .named("sidebar.summary")
    .at(summary_rect);
    let body_bottom = rect.max.y - SIDEBAR_PAD;
    let body = Rect::from_min_max(
        Vec2::new(rect.min.x, summary_rect.max.y + 6.0),
        Vec2::new(rect.max.x, body_bottom),
    );
    let mut content = vec![crate::inspector::source(ctx, state)];
    match active {
        0 => content.extend(crate::inspector::view_body(ctx, state)),
        _ => content.extend(crate::inspector::objects_body(ctx, state)),
    }
    let column = grana_ui::column()
        .padding(Edges {
            left: SIDEBAR_PAD,
            top: 4.0,
            right: SIDEBAR_PAD,
            bottom: 12.0,
        })
        .gap(8.0)
        .children(content);
    let scroll = Element::new(UiLayout::column())
        .named("sidebar.scroll")
        .at(body)
        .scroll(Axis::Column, Length::Fill, None)
        .clipped()
        .children([column]);
    let surface = theme.surface(Role::Sheet, State::Rest);
    Element::new(UiLayout::column())
        .named("sidebar")
        .at(rect)
        .background(surface)
        .children([tabs, summary, scroll])
}

fn dock<'a>(
    ctx: &mut BuildContext<'_, Message>,
    rect: Rect,
    state: &'a View<'a>,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let grip = Rect::from_min_max(rect.min, Vec2::new(rect.max.x, rect.min.y + 10.0));
    let grip_element = grana_ui::canvas(Length::Fill, 10.0, move |painter, info| {
        let engaged = info.hover.max(info.press);
        painter.rect(
            Rect::from_center_size(
                Vec2::new(info.rect.center().x, info.rect.min.y + 5.0),
                Vec2::new(36.0 + 14.0 * engaged, 4.0),
            ),
            Corners::capsule(),
            palette.outline_variant.lerp(palette.outline, engaged),
        );
    })
    .at(grip)
    .cursor(grana_ui::Cursor::ResizeVertical)
    .on_pointer(|pointer| {
        (pointer.phase == grana_ui::Phase::Move).then_some(Message::SetDockHeight(-pointer.delta.y))
    });
    let header = Rect::from_min_size(
        rect.min + Vec2::new(14.0, 10.0),
        Vec2::new(rect.width() - 28.0, DOCK_HEADER),
    );
    let close = Rect::from_center_size(
        Vec2::new(header.max.x - 15.0, header.center().y),
        Vec2::splat(30.0),
    );
    let tabs_rect = Rect::from_min_max(header.min, Vec2::new(close.min.x - 10.0, header.max.y));
    let panels = Panel::available(state.access.author());
    let labels = panels
        .iter()
        .map(|panel| panel.name(ctx.language()))
        .collect::<Vec<_>>();
    let panel = state.workspace.panel.unwrap_or(state.workspace.last_panel);
    let active = panels
        .iter()
        .position(|candidate| *candidate == panel)
        .unwrap_or(0);
    let tabs = crate::view::tabs(ctx, "dock.tabs", &labels, active, move |index| {
        Message::SetPanel(panels.get(index).copied())
    })
    .at(Rect::from_min_size(
        Vec2::new(tabs_rect.min.x, header.center().y - 16.0),
        Vec2::new(tabs_rect.width(), 32.0),
    ));
    let close_element = crate::view::icon_button(
        ctx,
        "dock.close",
        Icon::Close,
        30.0,
        Role::SurfaceHigh,
        grana_i18n::panel::close_panel(ctx.language()),
        Message::SetPanel(None),
    )
    .at(close);
    let content = Rect::from_min_max(
        Vec2::new(rect.min.x + 14.0, header.max.y + 8.0),
        Vec2::new(rect.max.x - 14.0, rect.max.y - 12.0),
    );
    let mut children = vec![grip_element, tabs, close_element];
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
    let surface = theme.surface(Role::Sheet, State::Rest);
    Element::new(UiLayout::column())
        .named("dock")
        .at(rect)
        .background(surface)
        .children(children)
}

fn clipped<'a>(rect: Rect, children: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(UiLayout::column())
        .at(rect)
        .clipped()
        .children(children)
}

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
            Message::ToggleComparisonDetail { group, key } => {
                let selected = (group, key);
                workspace.comparison_open = if workspace.comparison_open.as_ref() == Some(&selected)
                {
                    None
                } else {
                    Some(selected)
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
            Message::Window(action) => frame.window.push(action),
        }
    }
}
