use grana_ui::{Button, Element, Event, InputKey, Layout, Length, Size, TextStyle, Ui, Vec2};

use crate::support::gpu;

const SIZE: Vec2 = Vec2::new(320.0, 180.0);

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
