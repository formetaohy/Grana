use glam::Vec2;
use grana_ui::{
    Align, Block, Color, Corners, Cursor, Edges, Fill, Length, Rect, Role, Shadow, TextStyle,
    TypeStyle, Ui, Vec2 as UiVec2,
};

use crate::action::Action;
use crate::overlay::{self, Overlay};
use crate::state::State;
use crate::widget;

const MARGIN: f32 = 16.0;
const INSPECTOR_WIDTH: f32 = 320.0;
const TOOLBAR_HEIGHT: f32 = 52.0;
const SHEET_DEFAULT: f32 = 320.0;
const SHEET_MINIMUM: f32 = 200.0;
const TIMELINE_WIDTH: f32 = 520.0;
const TIMELINE_HEIGHT: f32 = 44.0;
const GAP: f32 = 12.0;
const SPLITTER: f32 = 5.0;

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
        let stage_left = inspector.max.x + GAP;
        let sheet = state.panel.map(|_| {
            let available = full.max.y - MARGIN - top - GAP - 160.0;
            let height = sheet_height(ui, available);
            Rect::from_min_max(
                Vec2::new(stage_left, full.max.y - MARGIN - height),
                Vec2::new(full.max.x - MARGIN, full.max.y - MARGIN),
            )
        });
        let stage_bottom = sheet.map_or(full.max.y - MARGIN, |sheet| sheet.min.y - GAP);
        let stage = Rect::from_min_max(
            Vec2::new(stage_left, top),
            Vec2::new(full.max.x - MARGIN, stage_bottom),
        );
        Self {
            toolbar,
            inspector,
            stage,
            sheet,
        }
    }

    fn views(&self, split: f32) -> (Rect, Rect) {
        let width = (self.stage.width() - SPLITTER) * split;
        (
            Rect::from_min_max(
                self.stage.min,
                Vec2::new(self.stage.min.x + width, self.stage.max.y),
            ),
            Rect::from_min_max(
                Vec2::new(self.stage.min.x + width + SPLITTER, self.stage.min.y),
                self.stage.max,
            ),
        )
    }
}

pub fn build(ui: &mut Ui, state: &mut State<'_>) -> Vec<Action> {
    let mut actions = Vec::new();
    let size = ui.size();
    let full = Rect::from_min_size(Vec2::ZERO, size);
    let layout = Layout::new(ui, state, full);
    ui.root(|ui| {
        {
            let backdrop = ui.theme().palette.scene_backdrop;
            let mut painter = ui.painter();
            painter.rect(full, Corners::ZERO, backdrop);
        }
        let (volume, section) = layout.views(state.viewport.split());
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
            let uv = scene_uv(volume, size);
            painter.scene(volume, Corners::all(18.0), uv);
            painter.outlined(
                volume,
                Corners::all(18.0),
                grana_ui::Fill::None,
                grana_ui::Border {
                    width: 1.0,
                    color: Color::white_alpha(0.07),
                },
                grana_ui::Shadow::NONE,
            );
            painter.scene(stage, Corners::all(12.0), scene_uv(stage, size));
        }
        let legends = overlay::legends(state.scene);
        let volume_safe = Rect::from_min_max(
            Vec2::new(layout.inspector.max.x + GAP, volume.min.y),
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
        split_handle(ui, volume, section, &mut actions);
        if state.scene.frame_count() > 1 {
            let rect = timeline_rect(volume);
            register_occluder(ui, rect);
            timeline_bar(ui, rect, state, &mut actions);
        }
        register_occluder(ui, layout.toolbar);
        toolbar_bar(ui, layout.toolbar, state, &mut actions);
        register_occluder(ui, layout.inspector);
        inspector_panel(ui, layout.inspector, state, &mut actions);
        if let Some(sheet) = layout.sheet {
            register_occluder(ui, sheet);
            sheet_panel(ui, sheet, state, &mut actions);
        }
    });
    actions
}

fn register_occluder(ui: &mut Ui, rect: Rect) {
    ui.register_occluder(rect);
}

fn button_width(ui: &mut Ui, label: &str) -> f32 {
    let body = TextStyle::new(ui.theme().font(grana_ui::Level::Body), Color::WHITE);
    ui.measure_text(label, body).x + 26.0
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
    let left = 20.0 + brand + 14.0 + open + 4.0 + project + 20.0;
    let right = fit + 12.0 + 2.0 * button_width(ui, "Analyze") + 20.0;
    let title_width = (rect.width() - left - right).max(80.0);
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
                        .padding(Edges::symmetric(20.0, 0.0))
                        .gaps(4.0)
                        .align(Align::Center)
                        .height(Length::Fill),
                    |ui| {
                        ui.text_style(
                            "grana",
                            TextStyle::new(title_style(ui), ui.theme().palette.text_primary),
                        );
                        ui.space(10.0);
                        if widget::button(ui, "open", "Open", Role::Quiet) {
                            actions.push(Action::OpenDataset);
                        }
                        project_menu(ui, state, actions);
                    },
                );
            },
        );
        let center = Rect::from_center_size(rect.center(), Vec2::new(title_width, rect.height()));
        {
            let style = TextStyle::new(
                ui.theme().font(grana_ui::Level::Body),
                ui.theme().palette.text_secondary,
            )
            .aligned(Align::Center);
            let mut painter = ui.painter();
            painter.text_in(center, state.scene.dataset.name(), style, Align::Center);
        }
        ui.place(
            Rect::from_min_size(
                Vec2::new(rect.max.x - right, rect.min.y),
                Vec2::new(right, rect.height()),
            ),
            |ui| {
                ui.row(
                    Block::row()
                        .padding(Edges::symmetric(20.0, 0.0))
                        .gaps(8.0)
                        .align(Align::Center)
                        .justify(Align::End)
                        .height(Length::Fill),
                    |ui| {
                        if widget::button(ui, "fit", "Fit", Role::Quiet) {
                            actions.push(Action::Fit);
                        }
                        ui.space(6.0);
                        mode_switch(ui, state, actions);
                    },
                );
            },
        );
    });
}

fn mode_switch(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    let labels = ["Analyze", "Animate"];
    let font = ui.theme().font(grana_ui::Level::Callout);
    let style = TextStyle::new(font, Color::WHITE);
    let widths = labels.map(|label| ui.measure_text(label, style).x + 24.0);
    let height = ui.theme().metrics.control_height;
    let hairline = ui.theme().metrics.hairline;
    let cursor = ui.cursor_position();
    let track = Rect::from_min_size(cursor, Vec2::new(widths[0] + widths[1] + 4.0, height));
    let mut chosen = None;
    let active = match *state.panel {
        Some(Panel::Animation) => Some(1),
        Some(_) => Some(0),
        None => None,
    };
    for (index, width) in widths.iter().enumerate() {
        let cell = Rect::from_min_size(
            Vec2::new(
                track.min.x + 2.0 + if index == 0 { 0.0 } else { widths[0] },
                track.min.y + 2.0,
            ),
            Vec2::new(*width, track.height() - 4.0),
        );
        let key = ui.id(&format!("mode.{index}"));
        let response = ui.interact(key, cell);
        if response.clicked {
            chosen = Some(index);
        }
        let role = if active == Some(index) {
            Role::SegmentSelected
        } else {
            Role::Segment
        };
        let look = ui.look(key, role, &response);
        let text = if active == Some(index) {
            ui.theme().palette.text_primary
        } else {
            ui.theme().palette.text_secondary
        };
        let mut painter = ui.painter();
        painter.outlined(
            cell,
            Corners::capsule(),
            look.fill,
            look.border,
            look.shadow,
        );
        painter.text_in(
            cell,
            labels[index],
            TextStyle::new(font, text).aligned(Align::Center),
            Align::Center,
        );
    }
    let mut painter = ui.painter();
    painter.outlined(
        track,
        Corners::capsule(),
        Fill::Solid(Color::rgba(0.0, 0.0, 0.0, 0.20)),
        grana_ui::Border {
            width: hairline,
            color: Color::white_alpha(0.06),
        },
        Shadow::NONE,
    );
    ui.allocate(track.size());
    if let Some(index) = chosen {
        let panel = if index == 0 {
            if active == Some(0) {
                None
            } else {
                Some(Panel::Statistics)
            }
        } else if active == Some(1) {
            None
        } else {
            Some(Panel::Animation)
        };
        actions.push(Action::SetPanel(panel));
    }
}

fn project_menu(ui: &mut Ui, state: &State<'_>, actions: &mut Vec<Action>) {
    let key = ui.id("project.menu");
    let font = ui.theme().font(grana_ui::Level::Body);
    let width = ui
        .measure_text("Project", TextStyle::new(font, Color::WHITE))
        .x
        + 26.0;
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
        if grana_ui::popover::menu_row(ui, 2, "Save as...", false) {
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

fn timeline_rect(volume: Rect) -> Rect {
    let width = TIMELINE_WIDTH.min(volume.width() - 48.0).max(280.0);
    Rect::from_center_size(
        Vec2::new(
            volume.center().x,
            volume.max.y - 16.0 - TIMELINE_HEIGHT * 0.5,
        ),
        Vec2::new(width, TIMELINE_HEIGHT),
    )
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
                .padding(Edges::symmetric(14.0, 0.0))
                .gaps(12.0)
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
                let width = (ui.available().x - 108.0).max(80.0);
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

fn sheet_height(ui: &mut Ui, available: f32) -> f32 {
    let stored = ui.value_of(ui.id("sheet.height")).unwrap_or(SHEET_DEFAULT);
    stored.clamp(SHEET_MINIMUM, available.max(SHEET_MINIMUM))
}

fn inspector_panel(ui: &mut Ui, rect: Rect, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let look = ui.theme().look(Role::Panel, grana_ui::State::Rest);
    {
        let mut painter = ui.painter();
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    }
    ui.stacked(rect, true, |ui| {
        ui.scroll(ui.id("inspector"), Length::Fill, |ui| {
            ui.column(
                Block::column()
                    .padding(Edges::symmetric(18.0, 16.0))
                    .gaps(14.0),
                |ui| {
                    crate::inspector::header(ui, state);
                    ui.separator();
                    crate::inspector::channels(ui, state.scene);
                    ui.separator();
                    crate::inspector::display(ui, state.scene);
                    ui.separator();
                    crate::object_panel::build(ui, state, actions);
                    ui.separator();
                    crate::inspector::section(ui, state, actions);
                    ui.separator();
                    crate::inspector::more(ui, state);
                },
            );
        });
    });
}

fn sheet_panel(ui: &mut Ui, rect: Rect, state: &mut State<'_>, actions: &mut Vec<Action>) {
    let appear = ui.appeared(ui.id("sheet.appear"), 0.26);
    let rect = rect.translate(Vec2::new(0.0, (1.0 - appear) * 24.0));
    let look = ui.theme().look(Role::Sheet, grana_ui::State::Rest);
    {
        let mut painter = ui.painter();
        painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
    }
    ui.stacked(rect, true, |ui| {
        handle(ui, rect);
        ui.column(
            Block::column()
                .padding(Edges::symmetric(18.0, 14.0))
                .gaps(12.0),
            |ui| {
                let header = ui.allocate_full(ui.theme().metrics.control_height);
                let close = Rect::from_center_size(
                    Vec2::new(header.max.x - 13.0, header.center().y),
                    Vec2::splat(26.0),
                );
                ui.place(
                    Rect::from_min_max(header.min, Vec2::new(header.max.x - 34.0, header.max.y)),
                    |ui| {
                        let labels = Panel::ALL.map(|panel| panel.name());
                        if let Some(index) =
                            widget::tabs(ui, "panel.tabs", &labels, panel_index(state))
                        {
                            actions.push(Action::SetPanel(Some(Panel::ALL[index])));
                        }
                    },
                );
                let key = ui.id("hide.panel");
                let response = ui.interact(key, close);
                let look = ui.look(key, Role::Quiet, &response);
                {
                    let font = ui.theme().font(grana_ui::Level::Title);
                    let mut painter = ui.painter();
                    painter.outlined(close, look.corners, look.fill, look.border, look.shadow);
                    painter.text_in(
                        close,
                        "\u{00D7}",
                        TextStyle::new(font, look.text).aligned(Align::Center),
                        Align::Center,
                    );
                }
                if response.clicked {
                    actions.push(Action::SetPanel(None));
                }
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
        Vec2::new(rect.max.x, rect.min.y + 10.0),
    );
    let response = ui.interact(key, strip);
    if response.dragging {
        let delta = ui.input().delta().y;
        let height = ui.value_of(ui.id("sheet.height")).unwrap_or(SHEET_DEFAULT) - delta;
        ui.store_value(ui.id("sheet.height"), height);
        ui.set_cursor(Cursor::ResizeVertical);
    }
    let grip = Rect::from_center_size(
        Vec2::new(rect.center().x, rect.min.y + 4.0),
        Vec2::new(36.0, 4.0),
    );
    let mut painter = ui.painter();
    painter.rect(
        grip,
        Corners::capsule(),
        Color::white_alpha(if response.hovered { 0.28 } else { 0.16 }),
    );
}

fn split_handle(ui: &mut Ui, volume: Rect, section: Rect, actions: &mut Vec<Action>) {
    let x = volume.max.x + SPLITTER * 0.5;
    let rect = Rect::from_center_size(Vec2::new(x, volume.center().y), Vec2::new(5.0, 76.0));
    let response = ui.interact(ui.id("split.handle"), rect.expand(4.0));
    if response.hovered || response.dragging {
        ui.set_cursor(Cursor::ResizeHorizontal);
    }
    if response.dragging {
        let pointer = ui.input().position().x;
        let total = volume.width() + section.width() + SPLITTER;
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
    let dots = Color::white_alpha(if response.hovered { 0.55 } else { 0.30 });
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
