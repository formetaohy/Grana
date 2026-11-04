use glam::Vec2;
use grana_ui::{
    Align, Block, Border, Color, Corners, Cursor, Edges, Fill, Icon, Length, Level, Rect, Role,
    Shadow, TextStyle, TypeStyle, Ui, icon,
};

use crate::state::State;
use crate::widget;
use grana_session::Command;

mod header;
mod stage;
mod status;
mod timeline;

const HEADER_HEIGHT: f32 = 52.0;
const SIDEBAR_WIDTH: f32 = 288.0;
const STATUS_HEIGHT: f32 = 26.0;
const TRANSPORT_HEIGHT: f32 = 46.0;
const DOCK_HEADER: f32 = 38.0;
const DOCK_DEFAULT: f32 = 300.0;
const DOCK_MINIMUM: f32 = 168.0;
const STAGE_MINIMUM: f32 = 200.0;
const SPLITTER: f32 = 1.0;
const SIDEBAR_PAD: f32 = 14.0;

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
    header: Rect,
    sidebar: Rect,
    stage: Rect,
    volume: Rect,
    section: Rect,
    transport: Option<Rect>,
    dock: Option<Rect>,
    status: Rect,
}

impl Layout {
    fn new(ui: &mut Ui, state: &State<'_>, full: Rect) -> Self {
        let header = Rect::from_min_size(Vec2::ZERO, Vec2::new(full.width(), HEADER_HEIGHT));
        let status = Rect::from_min_size(
            Vec2::new(full.min.x, full.max.y - STATUS_HEIGHT),
            Vec2::new(full.width(), STATUS_HEIGHT),
        );
        let top = header.max.y;
        let bottom = status.min.y;
        let sidebar =
            Rect::from_min_max(Vec2::new(full.min.x, top), Vec2::new(SIDEBAR_WIDTH, bottom));
        let main = Rect::from_min_max(Vec2::new(SIDEBAR_WIDTH, top), Vec2::new(full.max.x, bottom));
        let transport_height = if state.scene.frame_count() > 1 {
            TRANSPORT_HEIGHT
        } else {
            0.0
        };
        let dock = state.workspace.panel.map(|_| {
            let available = (main.height() - transport_height - STAGE_MINIMUM).max(DOCK_MINIMUM);
            let stored = ui.value_of(ui.id("dock.height")).unwrap_or(DOCK_DEFAULT);
            let height = stored.clamp(DOCK_MINIMUM, available);
            Rect::from_min_max(Vec2::new(main.min.x, main.max.y - height), main.max)
        });
        let dock_top = dock.map_or(main.max.y, |rect| rect.min.y);
        let transport = (transport_height > 0.0).then(|| {
            Rect::from_min_max(
                Vec2::new(main.min.x, dock_top - transport_height),
                Vec2::new(main.max.x, dock_top),
            )
        });
        let stage_bottom = transport.map_or(dock_top, |rect| rect.min.y);
        let stage = Rect::from_min_max(main.min, Vec2::new(main.max.x, stage_bottom));
        let split = state.viewport.split();
        let width = (stage.width() - SPLITTER).max(0.0) * split;
        let volume = Rect::from_min_max(
            stage.min,
            Vec2::new((stage.min.x + width).min(stage.max.x), stage.max.y),
        );
        let section = Rect::from_min_max(
            Vec2::new((volume.max.x + SPLITTER).min(stage.max.x), stage.min.y),
            stage.max,
        );
        Self {
            header,
            sidebar,
            stage,
            volume,
            section,
            transport,
            dock,
            status,
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
        paint_shell(ui, &layout);
        header::bar(ui, layout.header, &mut state, &mut commands);
        sidebar_panel(ui, layout.sidebar, &mut state, &mut commands);
        state.stage.set_rects(layout.volume, layout.section);
        stage::paint(ui, &layout, &mut state, &mut commands);
        if let Some(rect) = layout.transport {
            timeline::transport(ui, rect, &mut state, &mut commands);
        }
        if let Some(rect) = layout.dock {
            dock_panel(ui, rect, &mut state, &mut commands);
        }
        status::bar(ui, layout.status, &state);
        stage::toast(ui, layout.stage, &state);
        ui.paint_tooltip();
    });
    commands
}

fn paint_shell(ui: &mut Ui, layout: &Layout) {
    let palette = ui.theme().palette;
    let full = Rect::from_min_size(Vec2::ZERO, ui.size());
    let mut painter = ui.painter();
    painter.rect(full, Corners::ZERO, palette.scene_backdrop);
    painter.rect(layout.header, Corners::ZERO, palette.card);
    painter.rect(layout.sidebar, Corners::ZERO, palette.card);
    painter.rect(
        Rect::from_min_size(
            Vec2::new(layout.sidebar.max.x - 1.0, layout.sidebar.min.y),
            Vec2::new(1.0, layout.sidebar.height()),
        ),
        Corners::ZERO,
        palette.border,
    );
    if let Some(rect) = layout.transport {
        painter.rect(rect, Corners::ZERO, palette.card);
        painter.rect(
            Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0)),
            Corners::ZERO,
            palette.border,
        );
    }
    if let Some(rect) = layout.dock {
        painter.rect(rect, Corners::ZERO, palette.card);
    }
    painter.rect(layout.status, Corners::ZERO, palette.card);
    painter.rect(
        Rect::from_min_size(
            Vec2::new(layout.header.min.x, layout.header.max.y - 1.0),
            Vec2::new(layout.header.width(), 1.0),
        ),
        Corners::ZERO,
        palette.border,
    );
    painter.rect(
        Rect::from_min_size(
            Vec2::new(layout.status.min.x, layout.status.min.y),
            Vec2::new(layout.status.width(), 1.0),
        ),
        Corners::ZERO,
        palette.border,
    );
}

fn sidebar_panel(ui: &mut Ui, rect: Rect, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let header_height = crate::inspector::dataset_height(state);
    let header = Rect::from_min_max(
        Vec2::new(rect.min.x + SIDEBAR_PAD, rect.min.y + 14.0),
        Vec2::new(rect.max.x - SIDEBAR_PAD, rect.min.y + 14.0 + header_height),
    );
    ui.place(header, |ui| {
        crate::inspector::dataset(ui, state, commands);
    });
    let tabs = Rect::from_min_size(
        Vec2::new(rect.min.x + SIDEBAR_PAD, header.max.y + 10.0),
        Vec2::new(rect.width() - SIDEBAR_PAD * 2.0, 34.0),
    );
    let labels = ["View", "Objects"];
    let key = ui.id("sidebar.tab");
    let active = ui
        .value_of(key)
        .map_or(0, |value| value.round().clamp(0.0, 1.0) as usize);
    ui.place(tabs, |ui| {
        if let Some(index) = widget::tab_bar(ui, "sidebar.tabs", &labels, active, true) {
            ui.store_value(key, index as f32);
        }
    });
    let body = Rect::from_min_max(
        Vec2::new(rect.min.x, tabs.max.y + 1.0),
        Vec2::new(rect.max.x, rect.max.y),
    );
    ui.stacked(body, true, |ui| {
        ui.scroll(
            ui.id(&format!("sidebar.scroll.{active}")),
            Length::Fill,
            |ui| {
                ui.column(
                    Block::column()
                        .padding(Edges::symmetric(SIDEBAR_PAD, 12.0))
                        .gaps(6.0),
                    |ui| match active {
                        0 => crate::inspector::view_body(ui, state, commands),
                        _ => crate::inspector::objects_body(ui, state, commands),
                    },
                );
            },
        );
    });
}

fn dock_panel(ui: &mut Ui, rect: Rect, state: &mut State<'_>, commands: &mut Vec<Command>) {
    let palette = ui.theme().palette;
    {
        let mut painter = ui.painter();
        painter.rect(
            Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0)),
            Corners::ZERO,
            palette.border,
        );
    }
    let grip = Rect::from_min_max(rect.min, Vec2::new(rect.max.x, rect.min.y + 5.0));
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
    if response.hovered {
        let mut painter = ui.painter();
        painter.rect(
            Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0)),
            Corners::ZERO,
            Color::white_alpha(0.18),
        );
    }
    let header = Rect::from_min_size(
        rect.min + Vec2::new(SIDEBAR_PAD, 0.0),
        Vec2::new(rect.width() - SIDEBAR_PAD * 2.0, DOCK_HEADER),
    );
    let close = Rect::from_center_size(
        Vec2::new(header.max.x - 10.0, header.center().y),
        Vec2::splat(26.0),
    );
    let tabs = Rect::from_min_max(header.min, Vec2::new(close.min.x - 12.0, header.max.y));
    let labels = Panel::ALL.map(Panel::name);
    let active = panel_index(state);
    ui.store_value(ui.id("dock.last"), active as f32);
    ui.place(tabs, |ui| {
        if let Some(index) = widget::tab_bar(ui, "dock.tabs", &labels, active, false) {
            state.workspace.panel = Some(Panel::ALL[index]);
        }
    });
    {
        let mut painter = ui.painter();
        painter.rect(
            Rect::from_min_size(
                Vec2::new(rect.min.x, header.max.y - 1.0),
                Vec2::new(rect.width(), 1.0),
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
            26.0,
            Role::Quiet,
            "Close panel",
        ) {
            state.workspace.panel = None;
        }
    });
    let content = Rect::from_min_max(
        Vec2::new(rect.min.x + SIDEBAR_PAD, header.max.y),
        Vec2::new(rect.max.x - SIDEBAR_PAD, rect.max.y - 12.0),
    );
    let panel = state.workspace.panel.unwrap_or(Panel::Statistics);
    ui.place(content, |ui| match panel {
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
        | grana_scene::Job::Coloc => "frames",
    };
    let prefix = match job {
        grana_scene::Job::Detect(_) if frames > 1 => format!("frame {} · ", progress.time + 1),
        grana_scene::Job::Detect(_)
        | grana_scene::Job::Recut(_)
        | grana_scene::Job::Track(_)
        | grana_scene::Job::Relation { .. }
        | grana_scene::Job::Coloc => String::new(),
    };
    if progress.total > 0 {
        format!("{prefix}{}/{} {unit}", progress.done, progress.total)
    } else {
        format!("{prefix}preparing")
    }
}

pub fn divider(ui: &mut Ui) {
    let rect = ui.allocate_full(13.0);
    let palette = ui.theme().palette;
    let mut painter = ui.painter();
    painter.rect(
        Rect::from_min_size(
            Vec2::new(rect.min.x, rect.center().y),
            Vec2::new(rect.width(), 1.0),
        ),
        Corners::ZERO,
        palette.separator,
    );
}

pub fn section_header(ui: &mut Ui, title: &str, detail: Option<&str>) {
    let rect = ui.allocate_full(24.0);
    let palette = ui.theme().palette;
    let style = TextStyle::new(ui.theme().label_font(), palette.text_tertiary);
    let detail_style =
        TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary).aligned(Align::End);
    let mut painter = ui.painter();
    painter.text_in(rect, title, style, Align::Center);
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
    let look = ui.theme().look(Role::Card, grana_ui::State::Rest);
    ui.column(
        Block::column()
            .padding(Edges::symmetric(12.0, 12.0))
            .gaps(gap)
            .background(look),
        content,
    )
}

pub fn card_surface(ui: &mut Ui, rect: Rect) {
    let palette = ui.theme().palette;
    let corners = Corners::all(ui.theme().metrics.card_radius);
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        corners,
        Fill::Solid(palette.card),
        Border {
            width: 1.0,
            color: palette.border,
        },
        Shadow::NONE,
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
    let rect = ui.allocate_full(20.0);
    let palette = ui.theme().palette;
    let title_style = TextStyle::new(TypeStyle::new(12.0, 500.0, 16.0), palette.text_primary);
    let detail_style =
        TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary).aligned(Align::End);
    {
        let mut painter = ui.painter();
        icon::paint(
            symbol,
            &mut painter,
            Vec2::new(rect.min.x + 7.0, rect.center().y),
            13.0,
            palette.text_tertiary,
            1.5,
        );
        painter.text_in(
            Rect::from_min_max(Vec2::new(rect.min.x + 18.0, rect.min.y), rect.max),
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
    let width = available.min(460.0);
    let detail_style = TextStyle::new(ui.theme().font(Level::Caption), palette.text_tertiary)
        .wrapping()
        .aligned(Align::Center);
    let detail_layout = ui.layout_text(detail, detail_style, (width - 48.0).max(80.0));
    let height = 22.0 + 40.0 + 8.0 + 20.0 + 6.0 + detail_layout.size.y + 22.0;
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
                color: palette.border,
            },
            Shadow::NONE,
        );
        let art = Rect::from_center_size(
            Vec2::new(rect.center().x, rect.min.y + 22.0 + 20.0),
            Vec2::splat(40.0),
        );
        painter.outlined(
            art,
            Corners::all(8.0),
            Fill::Solid(palette.raised),
            Border {
                width: 1.0,
                color: palette.border,
            },
            Shadow::NONE,
        );
        icon::paint(
            symbol,
            &mut painter,
            art.center(),
            18.0,
            palette.text_tertiary,
            1.5,
        );
    }
    ui.text_in(
        Rect::from_min_size(
            Vec2::new(rect.min.x + 24.0, rect.min.y + 22.0 + 42.0),
            Vec2::new(rect.width() - 48.0, 20.0),
        ),
        title,
        TextStyle::new(ui.theme().font(Level::Headline), palette.text_primary)
            .aligned(Align::Center),
        Align::Center,
    );
    ui.text_in(
        Rect::from_min_size(
            Vec2::new(rect.min.x + 24.0, rect.min.y + 22.0 + 42.0 + 26.0),
            Vec2::new(rect.width() - 48.0, detail_layout.size.y),
        ),
        detail,
        detail_style,
        Align::Center,
    );
}
