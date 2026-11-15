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
mod update;

pub(crate) use stage::remote;
pub use update::apply;

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
    pub startup: Option<Vec2>,
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
        frame.startup = ui
            .bounds("shell.panel")
            .map(|panel| crate::shell::window_size(panel.size()));
        return frame;
    };
    if workspace.panel == Some(Panel::Statistics)
        && view.measure.set().is_none()
        && let Some(set) = scene.objects.iter().find(|set| set.detected() > 0)
    {
        frame.commands.push(Command::SetMeasureSet(Some(set.id())));
    }
    workspace.prepare(view, ui.size());
    let palette = crate::palette::resolve(ui, view, workspace);
    let state = View {
        access: view.access,
        scene,
        measure: view.measure,
        measuring: view.measuring,
        timeline: view.timeline,
        jobs: view.jobs,
        localizations: view.localizations,
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
    let summary = grana_ui::canvas(Length::Fill, 26.0)
        .view((dims.clone(), spacing.clone()), move |_, painter, info| {
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
    let grip_element = grana_ui::canvas(Length::Fill, 10.0)
        .view_static(move |painter, info| {
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
        .on_drag(Axis::Column, |delta| Some(Message::SetDockHeight(-delta)));
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
