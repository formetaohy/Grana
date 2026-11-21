use grana_editor::{
    Message, Prompt, PromptChoice, SettingsTab, UpdateAction, UpdatePhase, Workspace,
};
use grana_ui::{
    Band, Button, Corners, Edges, Element, Event, Icon, InputKey, Layout, Length, Rect, Size,
    Surface, TextStyle, Ui, Vec2,
};

use crate::support::gpu;

const SIZE: Vec2 = Vec2::new(320.0, 180.0);
const PROMPT_SIZE: Vec2 = Vec2::new(640.0, 360.0);
const SETTINGS_SIZE: Vec2 = Vec2::new(900.0, 720.0);

#[test]
fn a_press_keeps_the_control_it_went_down_on() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut ui: Ui<&'static str> = Ui::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        gpu.pipelines(),
    );
    ui.update(SIZE, 1.0, 0.0);
    let element = screen();
    ui.present(element);
    let left = ui.bounds("left").expect("the left control is laid out");
    let right = ui.bounds("right").expect("the right control is laid out");
    let (left, right) = (left.center(), right.center());
    for event in [
        Event::Moved(left),
        Event::Down(Button::Primary),
        Event::Moved(right),
        Event::Up(Button::Primary),
    ] {
        ui.input_mut().push(event);
    }
    let flicked = ui.update(SIZE, 1.0, 0.1);
    assert!(
        flicked.is_empty(),
        "a flick that leaves the control it pressed activates nothing: {flicked:?}"
    );
    let element = screen();
    ui.present(element);
    for event in [Event::Moved(left), Event::Down(Button::Primary)] {
        ui.input_mut().push(event);
    }
    ui.update(SIZE, 1.0, 0.2);
    let element = screen();
    ui.present(element);
    ui.input_mut().push(Event::Up(Button::Primary));
    let clicked = ui.update(SIZE, 1.0, 0.3);
    assert_eq!(clicked, ["left"], "a click activates the control it hits");
}

#[test]
fn submits_a_field_on_enter_and_commits_it_out_of_view() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut ui: Ui<&'static str> = Ui::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        gpu.pipelines(),
    );
    ui.update(SIZE, 1.0, 0.0);
    let element = search(&ui);
    ui.present(element);
    let row = ui.bounds("row").expect("the row is laid out").center();
    for event in [Event::Moved(row), Event::Down(Button::Primary)] {
        ui.input_mut().push(event);
    }
    let blurred = ui.update(SIZE, 1.0, 0.1);
    assert!(
        blurred.is_empty(),
        "a press away from the search field submits nothing: {blurred:?}"
    );
    let element = search(&ui);
    ui.present(element);
    ui.input_mut().push(Event::Up(Button::Primary));
    let clicked = ui.update(SIZE, 1.0, 0.2);
    assert_eq!(
        clicked,
        ["row"],
        "a press away from the search field leaves the clicked row in charge"
    );
    let element = search(&ui);
    ui.present(element);
    ui.input_mut().push(Event::Key {
        key: InputKey::Enter,
        pressed: true,
        repeat: false,
    });
    let submitted = ui.update(SIZE, 1.0, 0.3);
    assert_eq!(submitted, ["submit"], "enter submits the search field");
    let element = scrolled(&ui, false);
    ui.present(element);
    let started = ui.update(SIZE, 1.0, 0.4);
    assert!(
        started.is_empty(),
        "a field that scrolls into view starts editing quietly: {started:?}"
    );
    let element = scrolled(&ui, true);
    ui.present(element);
    let element = scrolled(&ui, true);
    ui.present(element);
    let committed = ui.update(SIZE, 1.0, 0.5);
    assert_eq!(
        committed,
        ["commit"],
        "a field that leaves the viewport commits its text"
    );
    let element = removable(&ui, true);
    ui.present(element);
    let element = removable(&ui, true);
    ui.present(element);
    let started = ui.update(SIZE, 1.0, 0.6);
    assert!(
        started.is_empty(),
        "a field that scrolls into view starts editing quietly: {started:?}"
    );
    let element = removable(&ui, true);
    ui.present(element);
    let element = removable(&ui, false);
    ui.present(element);
    let committed = ui.update(SIZE, 1.0, 0.7);
    assert_eq!(
        committed,
        ["removed"],
        "a field whose node disappears commits its text"
    );
}

#[test]
fn the_unsaved_prompt_covers_the_editor_and_resolves_its_choice() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut ui: Ui<Message> = Ui::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        gpu.pipelines(),
    );
    ui.update(PROMPT_SIZE, 1.0, 0.0);
    ui.resize([PROMPT_SIZE.x as u32, PROMPT_SIZE.y as u32]);
    let mut workspace = Workspace::default();
    workspace.prompt = Some(Prompt::Unsaved);
    let element = prompt_screen(&mut ui, &workspace);
    ui.present(element);
    for name in [
        "prompt.title",
        "prompt.body",
        "prompt.cancel",
        "prompt.discard",
        "prompt.save",
    ] {
        assert!(
            ui.bounds(name).is_some(),
            "the drawn prompt lays out {name}"
        );
    }
    let behind = ui
        .bounds("behind")
        .expect("the editor target is laid out")
        .center();
    let element = prompt_screen(&mut ui, &workspace);
    ui.present(element);
    ui.input_mut().push(Event::Key {
        key: InputKey::Tab,
        pressed: true,
        repeat: false,
    });
    let focused = ui.update(PROMPT_SIZE, 1.0, 0.05);
    assert!(focused.is_empty(), "tab alone activates nothing");
    let element = prompt_screen(&mut ui, &workspace);
    ui.present(element);
    ui.input_mut().push(Event::Key {
        key: InputKey::Enter,
        pressed: true,
        repeat: false,
    });
    let entered = ui.update(PROMPT_SIZE, 1.0, 0.06);
    assert!(
        !matches!(entered.as_slice(), [Message::FitView]),
        "enter inside the prompt never reaches the control behind it"
    );
    assert!(
        matches!(entered.as_slice(), [Message::ResolvePrompt(_)]),
        "enter activates the prompt action the focus ring names"
    );
    for event in [Event::Moved(behind), Event::Down(Button::Primary)] {
        ui.input_mut().push(event);
    }
    let pressed = ui.update(PROMPT_SIZE, 1.0, 0.1);
    assert!(
        pressed.is_empty(),
        "the prompt scrim swallows a press meant for the editor"
    );
    let element = prompt_screen(&mut ui, &workspace);
    ui.present(element);
    ui.input_mut().push(Event::Up(Button::Primary));
    let released = ui.update(PROMPT_SIZE, 1.0, 0.2);
    assert!(
        released.is_empty(),
        "releasing over the scrim reaches no editor control"
    );
    let card = ui
        .bounds("prompt")
        .expect("the prompt card is laid out")
        .center();
    for event in [Event::Moved(card), Event::Down(Button::Primary)] {
        ui.input_mut().push(event);
    }
    let held = ui.update(PROMPT_SIZE, 1.0, 0.25);
    assert!(
        held.is_empty(),
        "a press on the prompt card reaches no editor control"
    );
    assert!(
        ui.pointer_over_chrome(),
        "the prompt card holds the pointer back from the scene"
    );
    assert!(
        !ui.pointer_captured(),
        "the prompt card starts no drag under the pointer"
    );
    let element = prompt_screen(&mut ui, &workspace);
    ui.present(element);
    ui.input_mut().push(Event::Up(Button::Primary));
    assert!(
        ui.update(PROMPT_SIZE, 1.0, 0.28).is_empty(),
        "releasing over the prompt card resolves nothing"
    );
    let cancel = ui
        .bounds("prompt.cancel")
        .expect("the prompt offers a cancel action")
        .center();
    for event in [Event::Moved(cancel), Event::Down(Button::Primary)] {
        ui.input_mut().push(event);
    }
    assert!(ui.update(PROMPT_SIZE, 1.0, 0.3).is_empty());
    let element = prompt_screen(&mut ui, &workspace);
    ui.present(element);
    ui.input_mut().push(Event::Up(Button::Primary));
    let resolved = ui.update(PROMPT_SIZE, 1.0, 0.4);
    assert!(
        matches!(
            resolved.as_slice(),
            [Message::ResolvePrompt(PromptChoice::Cancel)]
        ),
        "the drawn prompt resolves its choice instead of a native popup"
    );
}

#[test]
fn the_settings_sheet_offers_the_release_it_found() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut ui: Ui<Message> = Ui::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        gpu.pipelines(),
    );
    ui.update(SETTINGS_SIZE, 1.0, 0.0);
    ui.resize([SETTINGS_SIZE.x as u32, SETTINGS_SIZE.y as u32]);
    let mut workspace = Workspace::default();
    workspace.settings.open = true;
    workspace.settings.tab = SettingsTab::About;
    workspace.update.phase = UpdatePhase::Available;
    workspace.update.version = Some("9.9.9".to_string());
    workspace.update.size = Some(28 << 20);
    workspace.update.notes =
        Some("https://github.com/formetaohy/Grana/releases/tag/v9.9.9".to_string());
    let element = settings_screen(&mut ui, &workspace);
    ui.present(element);
    let sheet = ui
        .bounds("settings.sheet")
        .expect("the settings sheet is laid out");
    for name in [
        "settings.brand",
        "settings.update",
        "settings.notes",
        "settings.automatic",
    ] {
        let row = ui
            .bounds(name)
            .unwrap_or_else(|| panic!("the settings sheet lays out {name}"));
        assert!(
            sheet.contains(row.center()),
            "the settings sheet shows {name} on its surface"
        );
    }
    let download = ui
        .bounds("settings.update")
        .expect("the settings sheet offers the download")
        .center();
    for event in [Event::Moved(download), Event::Down(Button::Primary)] {
        ui.input_mut().push(event);
    }
    assert!(
        ui.update(PROMPT_SIZE, 1.0, 0.1).is_empty(),
        "pressing the update row activates nothing yet"
    );
    let element = settings_screen(&mut ui, &workspace);
    ui.present(element);
    ui.input_mut().push(Event::Up(Button::Primary));
    let pressed = ui.update(SETTINGS_SIZE, 1.0, 0.2);
    assert!(
        matches!(
            pressed.as_slice(),
            [Message::Update(UpdateAction::Download)]
        ),
        "the update row asks for the download it offers"
    );
    workspace.update.phase = UpdatePhase::Ready;
    workspace.update.progress = None;
    let element = settings_screen(&mut ui, &workspace);
    ui.present(element);
    let restart = ui
        .bounds("settings.update")
        .expect("a staged update offers the restart")
        .center();
    for event in [Event::Moved(restart), Event::Down(Button::Primary)] {
        ui.input_mut().push(event);
    }
    assert!(ui.update(SETTINGS_SIZE, 1.0, 0.3).is_empty());
    let element = settings_screen(&mut ui, &workspace);
    ui.present(element);
    ui.input_mut().push(Event::Up(Button::Primary));
    let pressed = ui.update(SETTINGS_SIZE, 1.0, 0.4);
    assert!(
        matches!(pressed.as_slice(), [Message::Update(UpdateAction::Apply)]),
        "the staged update asks for the restart it offers"
    );
}

#[test]
fn a_disclosure_band_answers_a_press_inside_the_card_padding() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut ui: Ui<&'static str> = Ui::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        gpu.pipelines(),
    );
    ui.update(SIZE, 1.0, 0.0);
    let element = disclosure(&ui);
    ui.present(element);
    let card = ui.bounds("card").expect("the card is laid out");
    let row = ui.bounds("row").expect("the disclosure row is laid out");
    let padding = Vec2::new(card.min.x + 4.0, row.center().y);
    assert!(
        card.contains(padding) && !row.contains(padding),
        "the pointer lands on the padding the card adds around the row"
    );
    for event in [Event::Moved(padding), Event::Down(Button::Primary)] {
        ui.input_mut().push(event);
    }
    assert!(ui.update(SIZE, 1.0, 0.1).is_empty());
    let element = disclosure(&ui);
    ui.present(element);
    ui.input_mut().push(Event::Up(Button::Primary));
    let toggled = ui.update(SIZE, 1.0, 0.2);
    assert_eq!(
        toggled,
        ["toggle"],
        "the whole card band toggles the disclosure it heads"
    );
}

#[test]
fn a_popup_panel_rises_from_below_into_the_centre() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut ui: Ui<Message> = Ui::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        gpu.pipelines(),
    );
    ui.update(PROMPT_SIZE, 1.0, 0.1);
    ui.resize([PROMPT_SIZE.x as u32, PROMPT_SIZE.y as u32]);
    let mut workspace = Workspace::default();
    let element = settings_screen(&mut ui, &workspace);
    ui.present(element);
    workspace.settings.open = true;
    let element = settings_screen(&mut ui, &workspace);
    ui.present(element);
    let opening = ui
        .bounds("settings.sheet")
        .expect("the opening panel is laid out");
    assert!(
        opening.min.y >= PROMPT_SIZE.y,
        "the panel enters from outside the bottom edge of the pane: {opening:?}"
    );
    let mut time = 0.1;
    let mut settled = opening;
    for _ in 0..40 {
        time += 0.05;
        ui.update(PROMPT_SIZE, 1.0, time);
        let element = settings_screen(&mut ui, &workspace);
        ui.present(element);
        settled = ui
            .bounds("settings.sheet")
            .expect("the open panel is laid out");
    }
    assert!(
        (settled.center().y - PROMPT_SIZE.y * 0.5).abs() < 1.0
            && (settled.center().x - PROMPT_SIZE.x * 0.5).abs() < 1.0,
        "the panel settles in the middle of the pane: {settled:?}"
    );
}

fn disclosure(ui: &Ui<&'static str>) -> Element<'static, &'static str> {
    let theme = *ui.theme();
    let band = Band::new(Edges::all(12.0), Corners::all(theme.metrics.corner_lg));
    let row = grana_ui::widget::disclosure_row(
        &theme,
        Icon::Sliders,
        "Camera",
        "Perspective",
        false,
        0.0,
        band,
    )
    .named("row")
    .press("toggle");
    grana_ui::column()
        .named("card")
        .padding(Edges::all(12.0))
        .background(
            Surface::fill(theme.palette.surface_container)
                .with_corners(Corners::all(theme.metrics.corner_lg)),
        )
        .children([row])
}

fn prompt_screen(ui: &mut Ui<Message>, workspace: &Workspace) -> Element<'static, Message> {
    let full = Rect::from_min_size(Vec2::ZERO, ui.size());
    let behind = Element::new(Layout {
        size: Size::new(Length::Fixed(120.0), Length::Fixed(48.0)),
        ..Layout::row()
    })
    .named("behind")
    .at(Rect::from_min_size(Vec2::ZERO, Vec2::new(120.0, 48.0)))
    .press(Message::FitView);
    let mut elements = vec![behind];
    {
        let mut ctx = ui.build();
        elements.extend(grana_editor::prompt::view(&mut ctx, full, workspace));
    }
    grana_ui::place(full, elements)
}

fn settings_screen(ui: &mut Ui<Message>, workspace: &Workspace) -> Element<'static, Message> {
    let full = Rect::from_min_size(Vec2::ZERO, ui.size());
    let mut elements = Vec::new();
    {
        let mut ctx = ui.build();
        elements.extend(grana_editor::settings_panel::view(
            &mut ctx, full, workspace,
        ));
    }
    grana_ui::place(full, elements)
}

fn screen() -> Element<'static, &'static str> {
    Element::row()
        .gap(8.0)
        .children([panel("left"), panel("right")])
}

fn search(ui: &Ui<&'static str>) -> Element<'static, &'static str> {
    let theme = ui.theme();
    let style = TextStyle::new(theme.font(grana_ui::Level::Body), theme.palette.on_surface);
    Element::column().gap(8.0).children([
        grana_ui::widget::text_field("", style, 160.0, false)
            .named("field")
            .editing(true)
            .on_submit(|_| "submit"),
        panel("row"),
    ])
}

fn panel(name: &'static str) -> Element<'static, &'static str> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(120.0), Length::Fixed(48.0)),
        ..Layout::row()
    })
    .named(name)
    .press(name)
}

fn removable(ui: &Ui<&'static str>, present: bool) -> Element<'static, &'static str> {
    let theme = ui.theme();
    let style = TextStyle::new(theme.font(grana_ui::Level::Body), theme.palette.on_surface);
    let mut children = Vec::new();
    if present {
        children.push(
            grana_ui::widget::text_field("typed", style, 160.0, false)
                .named("edit")
                .editing(true)
                .on_commit(|_| "removed"),
        );
    }
    Element::column().gap(8.0).children(children)
}

fn scrolled(ui: &Ui<&'static str>, out: bool) -> Element<'static, &'static str> {
    let theme = ui.theme();
    let style = TextStyle::new(theme.font(grana_ui::Level::Body), theme.palette.on_surface);
    let field = grana_ui::widget::text_field("typed", style, 160.0, false)
        .named("field")
        .editing(true)
        .on_commit(|_| "commit");
    let filler = Element::new(Layout {
        size: Size::new(Length::Fill, Length::Fixed(if out { 200.0 } else { 0.0 })),
        ..Layout::row()
    });
    Element::column()
        .width(Length::Fixed(320.0))
        .height(Length::Fixed(60.0))
        .clipped()
        .scroll(grana_ui::Axis::Column, Length::Fixed(60.0), Some(260.0))
        .children([filler, field])
}
