use glam::Vec2;
use grana_ui::glass::GlassStyle;
use grana_ui::{
    Align, Block, Border, Corners, Cursor, Edges, Fill, Icon, Length, Level, Rect, Role, TextStyle,
    TypeStyle, Ui, icon,
};

use crate::state::State;
use crate::widget;
use grana_session::Command;

mod header;
mod stage;
mod timeline;

const MARGIN: f32 = 16.0;
const GAP: f32 = 12.0;
const TOOLBAR_HEIGHT: f32 = 52.0;
const SIDEBAR_WIDTH: f32 = 312.0;
const TRANSPORT_HEIGHT: f32 = 46.0;
const DOCK_HEADER: f32 = 46.0;
const DOCK_DEFAULT: f32 = 360.0;
const DOCK_MINIMUM: f32 = 220.0;
const SIDEBAR_PAD: f32 = 18.0;
const ANALYSIS_WIDTH: f32 = 126.0;
const TOOLBAR_LEFT: f32 = 332.0;
const TOOLBAR_RIGHT: f32 = 192.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Statistics,
    Tracks,
    Colocalization,
    Batch,
    Animation,
}

impl Panel {
    pub const ALL: [Panel; 5] = [
        Panel::Statistics,
        Panel::Tracks,
        Panel::Colocalization,
        Panel::Batch,
        Panel::Animation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Statistics => "Statistics",
            Self::Tracks => "Tracks",
            Self::Colocalization => "Colocalization",
            Self::Batch => "Batch",
            Self::Animation => "Animation",
        }
    }
}

struct Layout {
    toolbar_left: Rect,
    toolbar_right: Rect,
    analysis: Rect,
    sidebar: Rect,
    stage: Rect,
    volume: Rect,
    section: Rect,
    transport: Option<Rect>,
    dock: Option<Rect>,
    sidebar_visible: bool,
}

impl Layout {
    fn new(ui: &mut Ui, state: &State<'_>, full: Rect) -> Self {
        let top = full.min.y + MARGIN;
        let toolbar_left = Rect::from_min_size(
            Vec2::new(full.min.x + MARGIN, top),
            Vec2::new(TOOLBAR_LEFT, TOOLBAR_HEIGHT),
        );
        let analysis = Rect::from_min_size(
            Vec2::new(full.max.x - MARGIN - ANALYSIS_WIDTH, top + 9.0),
            Vec2::new(ANALYSIS_WIDTH, 34.0),
        );
        let toolbar_right = Rect::from_min_size(
            Vec2::new(analysis.min.x - GAP - TOOLBAR_RIGHT, top),
            Vec2::new(TOOLBAR_RIGHT, TOOLBAR_HEIGHT),
        );
        let sidebar_target = if state.workspace.sidebar_hidden {
            0.0
        } else {
            1.0
        };
        let sidebar_t = ui.spring_to(
            ui.id("sidebar.open"),
            sidebar_target,
            grana_ui::Motion::snappy(),
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
        let left = full.min.x + MARGIN + (SIDEBAR_WIDTH + GAP) * sidebar_t.clamp(0.0, 1.0);
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
        let dock_target = if state.workspace.panel.is_some() {
            1.0
        } else {
            0.0
        };
        let dock_t = ui.spring_to(ui.id("dock.open"), dock_target, grana_ui::Motion::snappy());
        let dock = (dock_t > 0.005).then(|| {
            let top = toolbar_left.max.y + GAP + 120.0;
            let available = (dock_bottom - top).max(DOCK_MINIMUM);
            let stored = ui.value_of(ui.id("dock.height")).unwrap_or(DOCK_DEFAULT);
            let clamped = stored.clamp(DOCK_MINIMUM, available);
            let height = ui.spring_to(
                ui.id("dock.height.spring"),
                clamped,
                grana_ui::Motion::response(0.20, 0.08),
            );
            let slide = (1.0 - dock_t) * (height + MARGIN + GAP * 2.0);
            Rect::from_min_max(
                Vec2::new(left, dock_bottom - height + slide),
                Vec2::new(right, dock_bottom + slide),
            )
        });
        let split_ratio = ui.spring_to(
            ui.id("viewport.split.spring"),
            state.viewport.split(),
            grana_ui::Motion::response(0.18, 0.06),
        );
        let split = full.width() * split_ratio;
        let gutter = GAP * 0.5;
        let volume = Rect::from_min_max(full.min, Vec2::new(split - gutter, full.max.y));
        let section = Rect::from_min_max(Vec2::new(split + gutter, full.min.y), full.max);
        Self {
            toolbar_left,
            toolbar_right,
            analysis,
            sidebar,
            stage: full,
            volume,
            section,
            transport,
            dock,
            sidebar_visible,
        }
    }
}

pub fn build(
    ui: &mut Ui,
    view: &grana_session::SessionView<'_>,
    viewport: &mut grana_display::Viewport,
    stage: &mut crate::stage::Stage,
    workspace: &mut crate::state::Workspace,
    frame: crate::state::FrameStats,
) -> Vec<Command> {
    let mut state = State {
        scene: view.scene,
        measure: view.measure,
        timeline: view.timeline,
        jobs: view.jobs,
        io: &view.io,
        batch: view.batch.as_ref(),
        history: view.history,
        series: &view.series,
        viewport,
        stage,
        workspace,
        frame,
    };
    let mut commands = Vec::new();
    let size = ui.size();
    let full = Rect::from_min_size(Vec2::ZERO, size);
    let layout = Layout::new(ui, &state, full);
    ui.root(|ui| {
        state.stage.set_rects(layout.volume, layout.section);
        stage::paint(ui, &layout, &mut state, &mut commands);
        header::bar(ui, &layout, &mut state, &mut commands);
        if layout.sidebar_visible {
            sidebar_panel(ui, &layout, &mut state, &mut commands);
        }
        if let Some(rect) = layout.transport {
            timeline::transport(ui, rect, &mut state, &mut commands);
        }
        if let Some(rect) = layout.dock {
            dock_panel(ui, rect, &mut state, &mut commands);
        }
        stage::toast(ui, &layout, &state);
        ui.paint_tooltip();
    });
    commands
}

fn sidebar_panel(ui: &mut Ui, layout: &Layout, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let rect = layout.sidebar;
    ui.glass(ui.id("sidebar.glass"), rect, GlassStyle::panel());
    let tabs = Rect::from_min_size(
        Vec2::new(rect.min.x + SIDEBAR_PAD, rect.min.y + SIDEBAR_PAD),
        Vec2::new(rect.width() - SIDEBAR_PAD * 2.0, 28.0),
    );
    let labels = ["View", "Objects"];
    let key = ui.id("sidebar.tab");
    let active = ui
        .value_of(key)
        .map_or(0, |value| value.round().clamp(0.0, 1.0) as usize);
    ui.place(tabs, |ui| {
        if let Some(index) = widget::segmented_fill(ui, "sidebar.tabs", &labels, active) {
            ui.store_value(key, index as f32);
        }
    });
    let body = Rect::from_min_max(
        Vec2::new(rect.min.x, tabs.max.y + 10.0),
        Vec2::new(rect.max.x, rect.max.y),
    );
    ui.stacked(body, true, |ui| {
        ui.scroll(
            ui.id(&format!("sidebar.scroll.{active}")),
            Length::Fill,
            |ui| {
                ui.column(
                    Block::column()
                        .padding(Edges {
                            left: SIDEBAR_PAD,
                            top: 6.0,
                            right: SIDEBAR_PAD,
                            bottom: 20.0,
                        })
                        .gaps(10.0),
                    |ui| {
                        crate::inspector::source(ui, state, commands);
                        match active {
                            0 => crate::inspector::view_body(ui, state, commands),
                            _ => crate::inspector::objects_body(ui, state, commands),
                        }
                    },
                );
            },
        );
    });
}

fn dock_panel(ui: &mut Ui, rect: Rect, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let palette = ui.theme().palette;
    ui.glass(ui.id("dock.glass"), rect, GlassStyle::sheet());
    let grip = Rect::from_min_max(rect.min, Vec2::new(rect.max.x, rect.min.y + 10.0));
    let response = ui.interact(ui.id("dock.resize"), grip);
    if response.hovered || response.dragging {
        ui.set_cursor(Cursor::ResizeVertical);
    }
    if response.dragging {
        let delta = ui.input().delta().y;
        let key = ui.id("dock.height");
        let stored = ui.value_of(key).unwrap_or(DOCK_DEFAULT);
        ui.store_value(key, stored - delta);
    }
    {
        let mut painter = ui.painter();
        painter.rect(
            Rect::from_center_size(
                Vec2::new(grip.center().x, grip.min.y + 5.0),
                Vec2::new(
                    if response.hovered || response.dragging {
                        56.0
                    } else {
                        40.0
                    },
                    4.0,
                ),
            ),
            Corners::capsule(),
            if response.hovered || response.dragging {
                palette.text_tertiary
            } else {
                palette.text_quaternary
            },
        );
    }
    let header = Rect::from_min_size(
        rect.min + Vec2::new(SIDEBAR_PAD, 6.0),
        Vec2::new(rect.width() - SIDEBAR_PAD * 2.0, DOCK_HEADER),
    );
    let close = Rect::from_center_size(
        Vec2::new(header.max.x - 14.0, header.center().y),
        Vec2::splat(28.0),
    );
    let tabs = Rect::from_min_max(header.min, Vec2::new(close.min.x - 16.0, header.max.y));
    let labels = Panel::ALL.map(Panel::name);
    let active = if state.workspace.panel.is_some() {
        let index = panel_index(state);
        ui.store_value(ui.id("dock.last"), index as f32);
        index
    } else {
        ui.value_of(ui.id("dock.last"))
            .map_or(0, |value| value.round().clamp(0.0, 4.0) as usize)
    };
    ui.place(tabs, |ui| {
        if let Some(index) = widget::tabs(ui, "dock.tabs", &labels, active) {
            state.workspace.panel = Some(Panel::ALL[index]);
        }
    });
    {
        let mut painter = ui.painter();
        painter.rect(
            Rect::from_min_size(
                Vec2::new(rect.min.x + SIDEBAR_PAD, header.max.y + 2.0),
                Vec2::new(rect.width() - SIDEBAR_PAD * 2.0, 1.0),
            ),
            Corners::ZERO,
            palette.separator,
        );
    }
    ui.place(close, |ui| {
        if widget::icon_button(
            ui,
            "dock.close",
            Icon::Close,
            28.0,
            Role::Quiet,
            "Close panel",
        ) {
            state.workspace.panel = None;
        }
    });
    let content = Rect::from_min_max(
        Vec2::new(rect.min.x + SIDEBAR_PAD, header.max.y + 14.0),
        Vec2::new(rect.max.x - SIDEBAR_PAD, rect.max.y - 18.0),
    );
    let panel = state.workspace.panel.unwrap_or(Panel::ALL[active]);
    ui.stacked(content, true, |ui| match panel {
        Panel::Statistics => crate::measure_panel::build(ui, state, commands),
        Panel::Tracks => crate::track_panel::build(ui, state, commands),
        _ => {
            ui.scroll(
                ui.id(&format!("dock.body.{active}")),
                Length::Fill,
                |ui| match panel {
                    Panel::Colocalization => crate::coloc_panel::build(ui, state, commands),
                    Panel::Batch => crate::batch_panel::build(ui, state, commands),
                    Panel::Animation => crate::animation_panel::build(ui, state, commands),
                    Panel::Statistics | Panel::Tracks => {
                        unreachable!("statistics and tracks have their own layout")
                    }
                },
            );
        }
    });
}

fn panel_index(state: &State<'_>) -> usize {
    Panel::ALL
        .iter()
        .position(|panel| Some(*panel) == state.workspace.panel)
        .unwrap_or(0)
}

pub fn scene_uv(rect: Rect, size: Vec2) -> [f32; 4] {
    [
        rect.min.x / size.x,
        rect.min.y / size.y,
        rect.max.x / size.x,
        rect.max.y / size.y,
    ]
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
    kind: grana_scene::ObjectKind,
    job: grana_scene::Job,
    progress: grana_scene::Progress,
    frames: u32,
) -> String {
    let unit = match job {
        grana_scene::Job::Detect(_) => match kind {
            grana_scene::ObjectKind::Spots => "blocks",
            grana_scene::ObjectKind::Surfaces => "tiles",
            grana_scene::ObjectKind::Filaments => "frames",
        },
        grana_scene::Job::Track(_)
        | grana_scene::Job::Recut(_)
        | grana_scene::Job::Relation { .. }
        | grana_scene::Job::Coloc
        | grana_scene::Job::Drift => "frames",
    };
    let prefix = match job {
        grana_scene::Job::Detect(_) if frames > 1 => format!("frame {} · ", progress.time + 1),
        grana_scene::Job::Detect(_)
        | grana_scene::Job::Recut(_)
        | grana_scene::Job::Track(_)
        | grana_scene::Job::Relation { .. }
        | grana_scene::Job::Coloc
        | grana_scene::Job::Drift => String::new(),
    };
    if progress.total > 0 {
        format!("{prefix}{}/{} {unit}", progress.done, progress.total)
    } else {
        format!("{prefix}preparing")
    }
}

pub fn divider(ui: &mut Ui) {
    let rect = ui.allocate_full(15.0);
    let palette = ui.theme().palette;
    let mut painter = ui.painter();
    painter.rect(
        Rect::from_min_size(
            Vec2::new(rect.min.x + 2.0, rect.center().y),
            Vec2::new(rect.width() - 4.0, 1.0),
        ),
        Corners::ZERO,
        palette.separator.amplify(0.7),
    );
}

pub fn section_header(ui: &mut Ui, title: &str, detail: Option<&str>) {
    let rect = ui.allocate_full(26.0);
    let palette = ui.theme().palette;
    let font = TypeStyle {
        size: 10.0,
        weight: 600.0,
        tracking: 0.8,
        leading: 14.0,
    };
    let style = TextStyle::new(font, palette.text_tertiary);
    let detail_style =
        TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary).aligned(Align::End);
    let mut painter = ui.painter();
    painter.text_in(rect, &title.to_uppercase(), style, Align::Center);
    if let Some(detail) = detail {
        painter.text_in(
            Rect::from_min_max(Vec2::new(rect.min.x + 64.0, rect.min.y), rect.max),
            detail,
            detail_style,
            Align::Center,
        );
    }
}

pub fn card(ui: &mut Ui, gap: f32, content: impl FnOnce(&mut Ui)) -> Rect {
    let surface = ui.theme().surface(Role::Card, grana_ui::State::Rest);
    ui.column(
        Block::column()
            .padding(Edges::all(13.0))
            .gaps(gap)
            .background(surface),
        content,
    )
}

pub fn card_surface(ui: &mut Ui, rect: Rect) {
    let surface = ui.theme().surface(Role::Card, grana_ui::State::Rest);
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        surface.corners,
        surface.fill,
        surface.border,
        surface.shadow,
    );
}

pub fn column_label(column: &grana_measure::Column) -> String {
    let unit = column.unit().suffix();
    if unit.is_empty() {
        column.label().to_string()
    } else {
        format!("{} ({unit})", column.label())
    }
}

pub fn card_title(ui: &mut Ui, symbol: Icon, title: &str, detail: Option<&str>) {
    let rect = ui.allocate_full(22.0);
    let palette = ui.theme().palette;
    let title_style = TextStyle::new(TypeStyle::new(12.5, 600.0, 18.0), palette.text_primary);
    let detail_style =
        TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary).aligned(Align::End);
    {
        let mut painter = ui.painter();
        icon::paint(
            symbol,
            &mut painter,
            Vec2::new(rect.min.x + 8.0, rect.center().y),
            13.0,
            palette.text_tertiary,
            1.5,
        );
        painter.text_in(
            Rect::from_min_max(Vec2::new(rect.min.x + 22.0, rect.min.y), rect.max),
            title,
            title_style,
            Align::Center,
        );
        if let Some(detail) = detail {
            painter.text_in(rect, detail, detail_style, Align::Center);
        }
    }
}

pub fn empty_state(ui: &mut Ui, symbol: Icon, title: &str, detail: &str) {
    let palette = ui.theme().palette;
    let corners = Corners::all(ui.theme().metrics.card_radius);
    let available = ui.available().x;
    let width = available.min(520.0);
    let detail_style = TextStyle::new(ui.theme().font(Level::Callout), palette.text_tertiary)
        .wrapping()
        .aligned(Align::Center);
    let detail_layout = ui.layout_text(detail, detail_style, (width - 64.0).max(80.0));
    let height = 30.0 + 56.0 + 14.0 + 22.0 + 8.0 + detail_layout.size.y + 30.0;
    let outer = ui.allocate(Vec2::new(available, height));
    let rect = Rect::from_min_size(
        Vec2::new(outer.min.x + (available - width) * 0.5, outer.min.y),
        Vec2::new(width, height),
    );
    {
        let mut painter = ui.painter();
        painter.outlined(
            rect,
            corners,
            Fill::None,
            Border {
                width: 1.0,
                color: palette.material_border,
            },
            grana_ui::Shadow::NONE,
        );
        let art = Rect::from_center_size(
            Vec2::new(rect.center().x, rect.min.y + 30.0 + 28.0),
            Vec2::splat(56.0),
        );
        painter.outlined(
            art,
            Corners::all(18.0),
            Fill::Solid(palette.material_fill.amplify(0.8)),
            Border {
                width: 1.0,
                color: palette.material_border,
            },
            grana_ui::Shadow::NONE,
        );
        painter.rect(
            Rect::from_center_size(art.center(), Vec2::splat(40.0)),
            Corners::all(13.0),
            palette.accent.with_alpha(0.10),
        );
        icon::paint(
            symbol,
            &mut painter,
            art.center(),
            22.0,
            palette.text_tertiary,
            1.5,
        );
    }
    ui.text_in(
        Rect::from_min_size(
            Vec2::new(rect.min.x + 32.0, rect.min.y + 30.0 + 60.0),
            Vec2::new(rect.width() - 64.0, 22.0),
        ),
        title,
        TextStyle::new(ui.theme().font(Level::Headline), palette.text_primary)
            .aligned(Align::Center),
        Align::Center,
    );
    ui.text_in(
        Rect::from_min_size(
            Vec2::new(rect.min.x + 32.0, rect.min.y + 30.0 + 60.0 + 30.0),
            Vec2::new(rect.width() - 64.0, detail_layout.size.y),
        ),
        detail,
        detail_style,
        Align::Center,
    );
}
