use grana_ui::{Button, Element, Event, Layout, Length, Size, Ui, Vec2};

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
    ui.present(screen());
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
    ui.present(screen());
    for event in [Event::Moved(left), Event::Down(Button::Primary)] {
        ui.input_mut().push(event);
    }
    ui.update(SIZE, 1.0, 0.2);
    ui.present(screen());
    ui.input_mut().push(Event::Up(Button::Primary));
    let clicked = ui.update(SIZE, 1.0, 0.3);
    assert_eq!(clicked, ["left"], "a click activates the control it hits");
}

fn screen() -> Element<'static, &'static str> {
    Element::row()
        .gap(8.0)
        .children([panel("left"), panel("right")])
}

fn panel(name: &'static str) -> Element<'static, &'static str> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(120.0), Length::Fixed(48.0)),
        ..Layout::row()
    })
    .named(name)
    .press(name)
}
