use glam::Vec2;
use grana_ui::{
    Align, Block, Color, Corners, Cursor, Edges, Length, Rect, Role, TextStyle, TypeStyle, Ui,
    Vec2 as UiVec2,
};

use crate::action::Action;
use crate::overlay::{self, Overlay};
use crate::state::State;
use crate::widget;

const MARGIN: f32 = 12.0;
const INSPECTOR_WIDTH: f32 = 328.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const SHEET_DEFAULT: f32 = 330.0;
const SHEET_MINIMUM: f32 = 180.0;
const TIMELINE_WIDTH: f32 = 520.0;
const GAP: f32 = 8.0;

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

pub fn build(ui: &mut Ui, state: &mut State<'_>) -> Vec<Action> {
    let mut actions = Vec::new();
    let size = ui.size();
    let full = Rect::from_min_size(Vec2::ZERO, size);
    let content = Rect::from_min_max(
        Vec2::new(MARGIN, 10.0 + TOOLBAR_HEIGHT + 10.0),
        Vec2::new(full.max.x - MARGIN, full.max.y - MARGIN),
    );
    ui.root(|ui| {
        {
            let backdrop = ui.theme().palette.scene_backdrop;
            let mut painter = ui.painter();
            painter.rect(full, Corners::ZERO, backdrop);
        }
        let split = state.viewport.split();
        let splitter = 5.0;
        let volume = Rect::from_min_max(
            content.min,
            Vec2::new(
                content.min.x + (content.width() - splitter) * split,
                content.max.y,
            ),
        );
        let section = Rect::from_min_max(
            Vec2::new(volume.max.x + splitter, content.min.y),
            content.max,
        );
        state.viewport.set_rects(volume, section);
        let (plane_u, plane_v) = state.viewport.section().axis().in_plane();
        let world = state.scene.dataset.world_size();
        let points_per_micron = section.height() / state.viewport.section().span();
        let stage = Rect::from_center_size(
            section.center(),
            Vec2::new(
                world[plane_u.index()] * points_per_micron,
                world[plane_v.index()] * points_per_micron,
            ),
        )
        .intersect(section);
        {
            let mut painter = ui.painter();
            let backdrop = Color::rgba(0.016, 0.02, 0.03, 0.55);
            painter.rect(section, Corners::all(16.0), backdrop);
            let uv = scene_uv(volume, size);
            painter.scene(volume, Corners::all(16.0), uv);
            painter.scene(stage, Corners::ZERO, scene_uv(stage, size));
        }
        let legends = overlay::legends(state.scene);
        let toolbar = toolbar_rect(ui, full);
        let volume_safe = Rect::from_min_max(
            Vec2::new(inspector_rect(full, toolbar, None).max.x, volume.min.y),
            volume.max,
        );
        let mut overlay = Overlay {
            camera: state.viewport.camera(),
            section: state.viewport.section(),
            world: state.scene.dataset.world_size(),
            volume_safe,
            section_stage: stage,
            levels: state.scene.dataset.level_count(),
            volume_level: state.frame.volume.level,
            section_level: state.frame.section.level,
            samples: state.frame.samples,
            volume_rect: volume,
            section_rect: section,
            region: &mut state.scene.region,
            legends: &legends,
            recording: state.animation.movie,
        };
        overlay::paint(ui, &mut overlay);
        split_handle(ui, volume, section, state, &mut actions);
        let sheet_for_timeline = state
            .panel
            .map(|_| sheet_rect(full, sheet_height(ui, size)));
        let timeline =
            (state.scene.frame_count() > 1).then(|| timeline_rect(volume, sheet_for_timeline));
        if let Some(rect) = timeline {
            register_occluder(ui, rect);
            timeline_bar(ui, rect, state, &mut actions);
        }
        register_occluder(ui, toolbar);
        toolbar_bar(ui, toolbar, state, &mut actions);
        let sheet_height = sheet_height(ui, size);
        let sheet = state.panel.map(|_| sheet_rect(full, sheet_height));
        let inspector = inspector_rect(full, toolbar, sheet);
        if let Some(sheet) = sheet {
            register_occluder(ui, sheet);
        }
        register_occluder(ui, inspector);
        inspector_panel(ui, inspector, state, &mut actions);
        if let Some(sheet) = sheet {
            sheet_panel(ui, sheet, state, &mut actions);
        }
    });
    actions
}

fn register_occluder(ui: &mut Ui, rect: Rect) {
    ui.register_occluder(rect);
}

fn toolbar_rect(_ui: &mut Ui, full: Rect) -> Rect {
    Rect::from_min_max(
        Vec2::new(MARGIN, 10.0),
        Vec2::new(full.max.x - MARGIN, 10.0 + TOOLBAR_HEIGHT),
    )
}

fn button_width(ui: &mut Ui, label: &str) -> f32 {
    let body = TextStyle::new(ui.theme().font(grana_ui::Level::Body), Color::WHITE);
    ui.measure_text(label, body).x + 22.0
}

fn title_style(ui: &Ui) -> TypeStyle {
    ui.theme().font(grana_ui::Level::Title)
}

fn toolbar_bar(ui: &mut Ui, rect: Rect, state: &State<'_>, actions: &mut Vec<Action>) {
    let look = ui.theme().look(Role::Toolbar, grana_ui::State::Rest);
    let brand = ui
        .measure_text("grana", TextStyle::new(title_style(ui), Color::WHITE))
        .x;
    let open = button_width(ui, "Open");
    let project = button_width(ui, "Project");
    let fit = button_width(ui, "Fit");
    let analysis = button_width(ui, "Analysis");
    let animation = button_width(ui, "Animation");
    let left = 18.0 + brand + 13.0 + open + GAP + project + GAP + fit + 10.0;
    let right = analysis + GAP + animation + 18.0;
    let title_size = ui
        .measure_text(
            state.scene.dataset.name(),
            TextStyle::new(ui.theme().font(grana_ui::Level::Body), Color::WHITE),
        )
        .x;
    let title_width = (rect.width() - 2.0 * left.max(right) - 24.0).max(60.0);
    ui.stacked(rect, false, |ui| {
        {
            let mut painter = ui.painter();
            painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        }
        ui.place(
            Rect::from_min_size(rect.min, Vec2::new(left, rect.height())),
            |ui| {
                ui.row(
                    Block::row()
                        .padding(Edges::symmetric(18.0, 0.0))
                        .gaps(GAP)
                        .align(Align::Center)
                        .height(Length::Fill),
                    |ui| {
                        ui.text_style(
                            "grana",
                            TextStyle::new(title_style(ui), ui.theme().palette.text_primary),
                        );
                        separator(ui, TOOLBAR_HEIGHT - 20.0);
                        if widget::button(ui, "open", "Open", Role::Quiet) {
                            actions.push(Action::OpenDataset);
                        }
                        project_menu(ui, state, actions);
                        if widget::button(ui, "fit", "Fit", Role::Quiet) {
                            actions.push(Action::Fit);
                        }
                    },
                );
            },
        );
        if title_width > title_size {
            ui.place(
                Rect::from_center_size(rect.center(), Vec2::new(title_width, rect.height())),
                |ui| {
                    let style = TextStyle::new(
                        ui.theme().font(grana_ui::Level::Body),
                        ui.theme().palette.text_secondary,
                    )
                    .aligned(Align::Center);
                    ui.text_in(
                        Rect::from_min_size(
                            Vec2::new(rect.center().x - title_width * 0.5, rect.min.y),
                            Vec2::new(title_width, rect.height()),
                        ),
                        state.scene.dataset.name(),
                        style,
                        Align::Center,
                    );
                },
            );
        }
        ui.place(
            Rect::from_min_size(
                Vec2::new(rect.max.x - right, rect.min.y),
                Vec2::new(right, rect.height()),
            ),
            |ui| {
                ui.row(
                    Block::row()
                        .padding(Edges::symmetric(18.0, 0.0))
                        .gaps(GAP)
                        .align(Align::Center)
                        .height(Length::Fill),
                    |ui| {
                        let analysis_open = state.panel.is_some();
                        if widget::button(
                            ui,
                            "analysis",
                            "Analysis",
                            if analysis_open {
                                Role::Tinted
                            } else {
                                Role::Quiet
                            },
                        ) {
                            actions.push(Action::SetPanel(
                                (!analysis_open).then_some(Panel::Statistics),
                            ));
                        }
                        let animation_open = *state.panel == Some(Panel::Animation);
                        if widget::button(
                            ui,
                            "animation",
                            "Animation",
                            if animation_open {
                                Role::Tinted
                            } else {
                                Role::Quiet
                            },
                        ) {
                            actions.push(Action::SetPanel(
                                (!animation_open).then_some(Panel::Animation),
                            ));
                        }
                    },
                );
            },
        );
    });
}
fn project_menu(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    let key = ui.id("project.menu");
    let font = ui.theme().font(grana_ui::Level::Body);
    let width = ui
        .measure_text("Project", TextStyle::new(font, Color::WHITE))
        .x
        + 22.0;
    let height = ui.theme().metrics.control_height;
    let rect = ui.allocate(Vec2::new(width, height));
    let response = ui.interact(key, rect);
    if response.clicked {
        ui.toggle_popup(key);
    }
    let look = ui.look(key, Role::Quiet, &response);
    let font = ui.theme().font(grana_ui::Level::Body);
    {
        let mut painter = ui.painter();
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        painter.text_in(
            rect,
            "Project",
            TextStyle::new(font, look.text).aligned(Align::Center),
            Align::Center,
        );
    }
    let open = ui.popup_open(key);
    let mut chosen: Option<Action> = None;
    let busy = state.loading.is_some();
    grana_ui::popover::popover(ui, "project.menu", open, rect, 200.0, |ui| {
        if grana_ui::popover::menu_row(ui, 0, "Open project", false) {
            chosen = Some(Action::OpenProject);
        }
        if grana_ui::popover::menu_row(ui, 1, "Save project", false) {
            chosen = Some(Action::SaveProject);
        }
        if grana_ui::popover::menu_row(ui, 2, "Save as…", false) {
            chosen = Some(Action::SaveProjectAs);
        }
        if grana_ui::popover::menu_row(ui, 3, "Save snapshot", false) {
            chosen = Some(Action::SaveSnapshot);
        }
    });
    if let Some(action) = chosen {
        ui.close_popup();
        if !busy {
            actions.push(action);
        }
    }
}

fn separator(ui: &mut Ui, height: f32) {
    let rect = ui.allocate(Vec2::new(9.0, height));
    let mut painter = ui.painter();
    painter.rect(
        Rect::from_center_size(rect.center(), Vec2::new(1.0, height - 8.0)),
        Corners::ZERO,
        Color::white_alpha(0.12),
    );
}

fn timeline_rect(volume: Rect, sheet: Option<Rect>) -> Rect {
    let width = TIMELINE_WIDTH.min(volume.width() - 40.0).max(280.0);
    let bottom = match sheet {
        Some(sheet) => (sheet.min.y - 30.0).max(volume.min.y + 80.0),
        None => volume.max.y - 26.0,
    };
    Rect::from_center_size(Vec2::new(volume.center().x, bottom), Vec2::new(width, 40.0))
}

fn timeline_bar(ui: &mut Ui, rect: Rect, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let look = ui.theme().look(Role::Toolbar, grana_ui::State::Rest);
    ui.stacked(rect, false, |ui| {
        {
            let mut painter = ui.painter();
            painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        }
        ui.row(
            Block::row()
                .padding(Edges::symmetric(10.0, 0.0))
                .gaps(10.0)
                .align(Align::Center),
            |ui| {
                let symbol = if state.playing {
                    "\u{23F8}"
                } else {
                    "\u{25B6}"
                };
                if widget::icon_button(ui, "play", symbol, 26.0, Role::Quiet) {
                    actions.push(Action::TogglePlay);
                }
                let frames = state.scene.frame_count();
                let mut time = state.scene.time() as f32;
                let width = (ui.available().x - 96.0).max(80.0);
                if let Some(value) = widget::slider_sized(
                    ui,
                    "time",
                    None,
                    time,
                    0.0..=(frames - 1) as f32,
                    false,
                    Length::Fixed(width),
                ) {
                    time = value;
                    actions.push(Action::SetTime(time.round() as u32));
                }
                let seconds = time * state.scene.dataset.frame_interval();
                let style = TextStyle::new(
                    ui.theme().font(grana_ui::Level::Monospace),
                    ui.theme().palette.text_secondary,
                );
                let text = format!("{:>3} / {} · {seconds:.1} s", (time as u32) + 1, frames);
                let text_width = ui.measure_text(&text, style).x;
                ui.place(
                    Rect::from_min_size(
                        Vec2::new(ui.cursor_position().x, rect.min.y),
                        Vec2::new(text_width + 4.0, rect.height()),
                    ),
                    |ui| {
                        ui.text_in(
                            Rect::from_min_size(
                                ui.cursor_position(),
                                Vec2::new(text_width + 4.0, rect.height()),
                            ),
                            &text,
                            style,
                            Align::Center,
                        );
                    },
                );
            },
        );
    });
}

fn sheet_height(ui: &mut Ui, size: Vec2) -> f32 {
    let stored = ui.value_of(ui.id("sheet.height")).unwrap_or(SHEET_DEFAULT);
    stored.clamp(SHEET_MINIMUM, size.y * 0.7)
}

fn sheet_rect(full: Rect, height: f32) -> Rect {
    let left = MARGIN + INSPECTOR_WIDTH + 14.0;
    Rect::from_min_max(
        Vec2::new(left, full.max.y - MARGIN - height),
        Vec2::new(full.max.x - MARGIN, full.max.y - MARGIN),
    )
}

fn inspector_rect(full: Rect, toolbar: Rect, _sheet: Option<Rect>) -> Rect {
    let top = toolbar.max.y + 14.0;
    Rect::from_min_max(
        Vec2::new(MARGIN + 8.0, top),
        Vec2::new(MARGIN + 8.0 + INSPECTOR_WIDTH, full.max.y - MARGIN),
    )
}

fn inspector_panel(ui: &mut Ui, rect: Rect, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let look = ui.theme().look(Role::Panel, grana_ui::State::Rest);
    ui.stacked(rect, true, |ui| {
        {
            let mut painter = ui.painter();
            painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        }
        ui.scroll(ui.id("inspector"), Length::Fill, |ui| {
            ui.column(
                Block::column()
                    .padding(Edges::symmetric(16.0, 14.0))
                    .gaps(10.0),
                |ui| {
                    crate::inspector::header(ui, state);
                    ui.separator();
                    crate::inspector::channels(ui, state.scene);
                    ui.separator();
                    crate::inspector::display(ui, state.scene);
                    ui.separator();
                    crate::inspector::section(ui, state, actions);
                    ui.separator();
                    crate::inspector::region(ui, state);
                    ui.separator();
                    crate::object_panel::build(ui, state, actions);
                    ui.separator();
                    crate::inspector::rendering(ui, state.scene);
                    ui.separator();
                    crate::inspector::streaming(ui, state);
                    ui.separator();
                    crate::inspector::footer(ui, state);
                },
            );
        });
    });
}

fn sheet_panel(ui: &mut Ui, rect: Rect, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let appear = ui.appeared(ui.id("sheet.appear"), 0.26);
    let rect = rect.translate(Vec2::new(0.0, (1.0 - appear) * 26.0));
    let look = ui.theme().look(Role::Sheet, grana_ui::State::Rest);
    ui.stacked(rect, true, |ui| {
        {
            let mut painter = ui.painter();
            painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        }
        handle(ui, rect);
        ui.column(
            Block::column()
                .padding(Edges::symmetric(18.0, 12.0))
                .gaps(10.0),
            |ui| {
                ui.row(Block::row().gaps(12.0).align(Align::Center), |ui| {
                    let labels = Panel::ALL.map(|panel| panel.name());
                    if let Some(index) = widget::tabs(ui, "panel.tabs", &labels, panel_index(state))
                    {
                        actions.push(Action::SetPanel(Some(Panel::ALL[index])));
                    }
                    if widget::button(ui, "hide.panel", "Hide", Role::Quiet) {
                        actions.push(Action::SetPanel(None));
                    }
                });
                ui.separator();
                let panel = state.panel.unwrap_or(Panel::Statistics);
                ui.scroll(ui.id("sheet.body"), Length::Fill, |ui| match panel {
                    Panel::Statistics => crate::measure_panel::build(ui, state, actions),
                    Panel::Colocalization => crate::coloc_panel::build(ui, state, actions),
                    Panel::Batch => crate::batch_panel::build(ui, state, actions),
                    Panel::Animation => crate::animation_panel::build(ui, state, actions),
                });
            },
        );
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
        Vec2::new(rect.max.x, rect.min.y + 8.0),
    );
    let response = ui.interact(key, strip);
    if response.dragging {
        let delta = ui.input().delta().y;
        let height = ui.value_of(ui.id("sheet.height")).unwrap_or(SHEET_DEFAULT) - delta;
        ui.store_value(ui.id("sheet.height"), height);
        ui.set_cursor(Cursor::ResizeVertical);
    }

    let grip = Rect::from_center_size(
        Vec2::new(rect.center().x, rect.min.y + 3.0),
        Vec2::new(38.0, 4.0),
    );
    let mut painter = ui.painter();
    painter.rect(
        grip,
        Corners::capsule(),
        Color::white_alpha(if response.hovered { 0.22 } else { 0.12 }),
    );
}

fn split_handle(
    ui: &mut Ui,
    volume: Rect,
    section: Rect,
    state: &mut State<'_>,
    actions: &mut Vec<Action>,
) {
    let split = state.viewport.split();
    let x = volume.max.x + 2.5;
    let rect = Rect::from_center_size(Vec2::new(x, volume.center().y), Vec2::new(5.0, 74.0));
    let response = ui.interact(ui.id("split.handle"), rect.expand(3.0));
    if response.hovered || response.dragging {
        ui.set_cursor(Cursor::ResizeHorizontal);
    }
    if response.dragging {
        let pointer = ui.input().position().x;
        let total = volume.width() + section.width() + 5.0;
        actions.push(Action::SetSplit(
            ((pointer - volume.min.x) / total).clamp(0.25, 0.8),
        ));
    }
    let look = ui.theme().look(Role::Toolbar, grana_ui::State::Rest);
    let mut painter = ui.painter();
    painter.outlined(
        rect,
        Corners::capsule(),
        look.fill,
        look.border,
        look.shadow,
    );
    let dots = Color::white_alpha(if response.hovered { 0.5 } else { 0.28 });
    for offset in [-6.0f32, 0.0, 6.0] {
        painter.rect(
            Rect::from_center_size(
                Vec2::new(rect.center().x, rect.center().y + offset),
                Vec2::splat(1.6),
            ),
            Corners::capsule(),
            dots,
        );
    }
    let _ = split;
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

pub fn swatch(ui: &mut Ui, tint: [f32; 3]) -> Rect {
    let rect = ui.allocate(UiVec2::new(10.0, 10.0));
    let mut painter = ui.painter();
    painter.rect(rect, Corners::all(3.0), Color::from_rgb_f32(tint));
    rect
}
