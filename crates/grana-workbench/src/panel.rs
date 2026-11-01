use glam::Vec2;
use grana_ui::{
    Align, Block, Color, Corners, Cursor, Edges, Icon, Length, Level, Rect, Role, TextStyle,
    TypeStyle, Ui, icon,
};

use crate::action::Action;
use crate::state::State;
use crate::widget;

mod chrome;
mod stage;
mod timeline;

const MARGIN: f32 = 14.0;
const INSPECTOR_WIDTH: f32 = 316.0;
const TOOLBAR_HEIGHT: f32 = 46.0;
const SHEET_DEFAULT: f32 = 384.0;
const SHEET_MINIMUM: f32 = 220.0;
const GAP: f32 = 12.0;
const SPLITTER: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Statistics,
    Colocalization,
    Batch,
    Animation,
}

impl Panel {
    pub const ALL: [Panel; 4] = [
        Panel::Statistics,
        Panel::Colocalization,
        Panel::Batch,
        Panel::Animation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Statistics => "Statistics",
            Self::Colocalization => "Colocalization",
            Self::Batch => "Batch",
            Self::Animation => "Animation",
        }
    }
}

struct Layout {
    toolbar: Rect,
    inspector: Rect,
    stage: Rect,
    volume: Rect,
    section: Rect,
    timeline: Option<Rect>,
    sheet: Option<Rect>,
}

impl Layout {
    fn new(ui: &mut Ui, state: &State<'_>, full: Rect) -> Self {
        let toolbar = Rect::from_min_max(
            full.min + Vec2::splat(MARGIN),
            Vec2::new(full.max.x - MARGIN, MARGIN + TOOLBAR_HEIGHT),
        );
        let top = toolbar.max.y + GAP;
        let inspector = Rect::from_min_max(
            Vec2::new(MARGIN, top),
            Vec2::new(MARGIN + INSPECTOR_WIDTH, full.max.y - MARGIN),
        );
        let left = inspector.max.x + GAP;
        let sheet = state.panel.map(|_| {
            let available = full.max.y - MARGIN - top - GAP - 180.0;
            let height = sheet_height(ui, available);
            Rect::from_min_max(
                Vec2::new(left, full.max.y - MARGIN - height),
                Vec2::new(full.max.x - MARGIN, full.max.y - MARGIN),
            )
        });
        let bottom = sheet.map_or(full.max.y - MARGIN, |sheet| sheet.min.y - GAP);
        let stage =
            Rect::from_min_max(Vec2::new(left, top), Vec2::new(full.max.x - MARGIN, bottom));
        let split = state.viewport.split();
        let width = (stage.width() - SPLITTER).max(0.0) * split;
        let volume = Rect::from_min_max(stage.min, Vec2::new(stage.min.x + width, stage.max.y));
        let section = Rect::from_min_max(
            Vec2::new(stage.min.x + width + SPLITTER, stage.min.y),
            stage.max,
        );
        let timeline = (state.scene.frame_count() > 1 && volume.height() > 170.0)
            .then(|| timeline::timeline_rect(volume));
        Self {
            toolbar,
            inspector,
            stage,
            volume,
            section,
            timeline,
            sheet,
        }
    }
}

pub fn build(ui: &mut Ui, state: &mut State<'_>) -> Vec<Action> {
    let mut actions = Vec::new();
    let size = ui.size();
    let full = Rect::from_min_size(Vec2::ZERO, size);
    let layout = Layout::new(ui, state, full);
    ui.root(|ui| {
        stage::paint_backdrop(ui, &layout, state);
        stage::paint_stage(ui, &layout);
        state.viewport.set_rects(layout.volume, layout.section);
        stage::paint_overlay(ui, &layout, state);
        stage::split_handle(ui, layout.volume, layout.section, &mut actions);
        if let Some(rect) = layout.timeline {
            register_occluder(ui, rect);
            timeline::timeline_bar(ui, rect, state, &mut actions);
        }
        register_occluder(ui, layout.toolbar);
        chrome::toolbar_bar(ui, layout.toolbar, state, &mut actions);
        register_occluder(ui, layout.inspector);
        inspector_panel(ui, layout.inspector, state, &mut actions);
        if let Some(sheet) = layout.sheet {
            register_occluder(ui, sheet);
            sheet_panel(ui, sheet, state, &mut actions);
        }
        stage::toast(ui, layout.stage, state);
        ui.paint_tooltip();
    });
    actions
}

fn register_occluder(ui: &mut Ui, rect: Rect) {
    ui.register_occluder(rect);
}

fn inspector_panel(ui: &mut Ui, rect: Rect, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let look = ui.theme().look(Role::Panel, grana_ui::State::Rest);
    {
        let mut painter = ui.painter();
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        painter.gradient(
            Rect::from_min_size(rect.min, Vec2::new(rect.width(), 168.0)),
            Corners::top(24.0),
            Color::black_alpha(0.30),
            Color::TRANSPARENT,
        );
    }
    ui.stacked(rect, true, |ui| {
        let footer_height = 30.0;
        let header = Rect::from_min_size(
            rect.min + Vec2::new(18.0, 16.0),
            Vec2::new(rect.width() - 36.0, 146.0),
        );
        ui.place(header, |ui| {
            ui.column(Block::column().gaps(4.0), |ui| {
                crate::inspector::header(ui, state);
            });
        });
        let tabs = Rect::from_min_size(
            Vec2::new(header.min.x, header.max.y + 6.0),
            Vec2::new(rect.width() - 36.0, 30.0),
        );
        let tab_key = ui.id("inspector.tab");
        let labels = ["View", "Objects", "Render"];
        let tab = ui
            .value_of(tab_key)
            .map_or(0, |value| value.round().clamp(0.0, 2.0) as usize);
        ui.place(tabs, |ui| {
            if let Some(index) = widget::segmented(ui, "inspector.tabs", &labels, tab) {
                ui.store_value(tab_key, index as f32);
            }
        });
        let body = Rect::from_min_max(
            Vec2::new(rect.min.x, tabs.max.y + 12.0),
            Vec2::new(rect.max.x, rect.max.y - footer_height),
        );
        ui.place(body, |ui| {
            ui.scroll(
                ui.id(&format!("inspector.scroll.{tab}")),
                Length::Fill,
                |ui| {
                    ui.column(
                        Block::column()
                            .padding(Edges::symmetric(18.0, 4.0))
                            .gaps(14.0),
                        |ui| match tab {
                            0 => {
                                ui.column(Block::column().gaps(5.0), |ui| {
                                    crate::inspector::channels(ui, state.scene);
                                });
                                divider(ui);
                                ui.column(Block::column().gaps(8.0), |ui| {
                                    crate::inspector::display(ui, state.scene);
                                });
                                divider(ui);
                                ui.column(Block::column().gaps(8.0), |ui| {
                                    crate::inspector::section(ui, state, actions);
                                });
                                widget::weak(
                                    ui,
                                    "Drag to orbit · right-drag to pan · scroll to zoom",
                                );
                            }
                            1 => {
                                ui.column(Block::column().gaps(6.0), |ui| {
                                    crate::object_panel::build(ui, state, actions);
                                });
                            }
                            _ => {
                                ui.column(Block::column().gaps(6.0), |ui| {
                                    crate::inspector::region(ui, state);
                                    crate::inspector::rendering(ui, state.scene);
                                    crate::inspector::streaming(ui, state);
                                });
                            }
                        },
                    );
                },
            );
        });
        footer(ui, rect, footer_height, state);
    });
}

fn footer(ui: &mut Ui, rect: Rect, height: f32, state: &State<'_>) {
    let top = rect.max.y - height;
    let line = ui.theme().palette.separator;
    let adapter = state.adapter;
    let bytes = state.frame.bytes as f64 / (1024.0 * 1024.0);
    let tertiary = ui.theme().palette.text_tertiary;
    let style = TextStyle::new(TypeStyle::new(10.0, 460.0, 13.0), tertiary).truncating();
    {
        let mut painter = ui.painter();
        painter.rect(
            Rect::from_min_size(
                Vec2::new(rect.min.x + 18.0, top),
                Vec2::new(rect.width() - 36.0, 0.5),
            ),
            Corners::ZERO,
            line,
        );
        painter.push_clip(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 18.0, top),
                Vec2::new(rect.max.x - 18.0, rect.max.y),
            ),
            Corners::ZERO,
        );
        painter.text_at(Vec2::new(rect.min.x + 18.0, top + 9.0), adapter, style);
        painter.text_in(
            Rect::from_min_max(
                Vec2::new(rect.min.x + 18.0, top + 8.0),
                Vec2::new(rect.max.x - 18.0, rect.max.y - 2.0),
            ),
            &format!("{bytes:.0} MiB"),
            TextStyle::new(style.font, tertiary).aligned(Align::End),
            Align::Center,
        );
    }
}

fn divider(ui: &mut Ui) {
    let rect = ui.allocate_full(9.0);
    let color = ui.theme().palette.separator;
    let mut painter = ui.painter();
    painter.rect(
        Rect::from_min_size(
            Vec2::new(rect.min.x, rect.center().y),
            Vec2::new(rect.width(), 0.5),
        ),
        Corners::ZERO,
        color,
    );
}

fn sheet_panel(ui: &mut Ui, rect: Rect, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let appear = ui.appeared(ui.id("sheet.appear"), 0.28);
    let rect = rect.translate(Vec2::new(0.0, (1.0 - appear) * 26.0));
    let look = ui.theme().look(Role::Sheet, grana_ui::State::Rest);
    {
        let mut painter = ui.painter();
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    }
    ui.stacked(rect, true, |ui| {
        handle(ui, rect);
        let header = Rect::from_min_size(
            rect.min + Vec2::new(18.0, 14.0),
            Vec2::new(rect.width() - 36.0, 30.0),
        );
        let close = Rect::from_center_size(
            Vec2::new(header.max.x - 13.0, header.center().y),
            Vec2::splat(28.0),
        );
        let tabs_rect =
            Rect::from_min_max(header.min, Vec2::new(header.max.x - 44.0, header.max.y));
        ui.place(tabs_rect, |ui| {
            let labels = Panel::ALL.map(|panel| panel.name());
            if let Some(index) = widget::tabs(ui, "panel.tabs", &labels, panel_index(state)) {
                actions.push(Action::SetPanel(Some(Panel::ALL[index])));
            }
        });
        ui.place(close, |ui| {
            if widget::icon_button(
                ui,
                "hide.panel",
                Icon::Close,
                28.0,
                Role::Quiet,
                "Close panel",
            ) {
                actions.push(Action::SetPanel(None));
            }
        });
        let line = ui.theme().palette.separator;
        {
            let mut painter = ui.painter();
            painter.rect(
                Rect::from_min_size(
                    Vec2::new(rect.min.x + 18.0, header.max.y + 10.0),
                    Vec2::new(rect.width() - 36.0, 0.5),
                ),
                Corners::ZERO,
                line,
            );
        }
        let body = Rect::from_min_max(
            Vec2::new(rect.min.x + 18.0, header.max.y + 18.0),
            Vec2::new(rect.max.x - 18.0, rect.max.y - 16.0),
        );
        let panel = state.panel.unwrap_or(Panel::Statistics);
        ui.place(body, |ui| match panel {
            Panel::Statistics => crate::measure_panel::build(ui, state, actions),
            _ => {
                ui.scroll(ui.id("sheet.body"), Length::Fill, |ui| match panel {
                    Panel::Colocalization => crate::coloc_panel::build(ui, state, actions),
                    Panel::Batch => crate::batch_panel::build(ui, state, actions),
                    Panel::Animation => crate::animation_panel::build(ui, state, actions),
                    Panel::Statistics => unreachable!("statistics has its own layout"),
                });
            }
        });
    });
}

fn panel_index(state: &State<'_>) -> usize {
    Panel::ALL
        .iter()
        .position(|panel| Some(*panel) == *state.panel)
        .unwrap_or(0)
}

fn handle(ui: &mut Ui, rect: Rect) {
    let key = ui.id("sheet.resize");
    let strip = Rect::from_min_max(
        Vec2::new(rect.min.x, rect.min.y),
        Vec2::new(rect.max.x, rect.min.y + 12.0),
    );
    let response = ui.interact(key, strip);
    if response.dragging {
        let delta = ui.input().delta().y;
        let height = ui.value_of(ui.id("sheet.height")).unwrap_or(SHEET_DEFAULT) - delta;
        ui.store_value(ui.id("sheet.height"), height);
        ui.set_cursor(Cursor::ResizeVertical);
    }
    let grip = Rect::from_center_size(
        Vec2::new(rect.center().x, rect.min.y + 6.0),
        Vec2::new(36.0, 4.0),
    );
    let mut painter = ui.painter();
    painter.rect(
        grip,
        Corners::capsule(),
        Color::white_alpha(if response.hovered { 0.30 } else { 0.18 }),
    );
}

fn sheet_height(ui: &mut Ui, available: f32) -> f32 {
    let stored = ui.value_of(ui.id("sheet.height")).unwrap_or(SHEET_DEFAULT);
    stored.clamp(SHEET_MINIMUM, available.max(SHEET_MINIMUM))
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
            grana_scene::ObjectKind::Surfaces => "bricks",
            grana_scene::ObjectKind::Filaments => "frames",
        },
        grana_scene::Job::Track(_) | grana_scene::Job::Coloc => "frames",
    };
    let prefix = match job {
        grana_scene::Job::Detect(_) if frames > 1 => format!("frame {} · ", progress.time + 1),
        grana_scene::Job::Detect(_) | grana_scene::Job::Track(_) | grana_scene::Job::Coloc => {
            String::new()
        }
    };
    if progress.total > 0 {
        format!("{prefix}{}/{} {unit}", progress.done, progress.total)
    } else {
        format!("{prefix}preparing")
    }
}

pub fn caption(ui: &mut Ui, text: &str) {
    widget::weak(ui, text);
}

pub fn card(ui: &mut Ui, gap: f32, content: impl FnOnce(&mut Ui)) -> Rect {
    let mut look = ui.theme().look(Role::Card, grana_ui::State::Rest);
    look.corners = Corners::all(12.0);
    ui.column(
        Block::column()
            .padding(Edges::symmetric(14.0, 13.0))
            .gaps(gap)
            .background(look),
        content,
    )
}

pub fn card_title(ui: &mut Ui, symbol: Icon, title: &str, detail: Option<&str>) {
    let rect = ui.allocate_full(20.0);
    let icon_color = ui.theme().palette.text_secondary;
    let title_style = TextStyle::new(
        TypeStyle::new(11.0, 600.0, 14.0),
        ui.theme().palette.text_primary,
    );
    let detail_style = TextStyle::new(
        TypeStyle::new(10.0, 400.0, 13.0),
        ui.theme().palette.text_tertiary,
    )
    .aligned(Align::End);
    {
        let mut painter = ui.painter();
        icon::paint(
            symbol,
            &mut painter,
            Vec2::new(rect.min.x + 7.0, rect.center().y),
            13.0,
            icon_color,
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
    let mut look = ui.theme().look(Role::Card, grana_ui::State::Rest);
    look.corners = Corners::all(12.0);
    ui.column(
        Block::column()
            .padding(Edges::symmetric(24.0, 22.0))
            .gaps(9.0)
            .align(Align::Center)
            .background(look),
        |ui| {
            let art = ui.allocate(Vec2::splat(38.0));
            let art_color = ui.theme().palette.text_tertiary;
            {
                let mut painter = ui.painter();
                painter.rect(art, Corners::capsule(), Color::white_alpha(0.05));
                icon::paint(symbol, &mut painter, art.center(), 19.0, art_color, 1.5);
            }
            let title_style = TextStyle::new(
                ui.theme().font(Level::Headline),
                ui.theme().palette.text_primary,
            )
            .aligned(Align::Center);
            let title_rect = ui.allocate_full(18.0);
            {
                let mut painter = ui.painter();
                painter.text_in(title_rect, title, title_style, Align::Center);
            }
            let detail_style = TextStyle::new(
                ui.theme().font(Level::Caption),
                ui.theme().palette.text_tertiary,
            )
            .wrapping()
            .aligned(Align::Center);
            ui.text_style_wrapped(detail, detail_style, ui.available().x.min(420.0));
        },
    );
}
