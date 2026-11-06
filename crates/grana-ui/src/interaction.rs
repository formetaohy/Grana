use glam::Vec2;

use crate::Ui;
use crate::element::{Axis, Element, Key, Layout, Length};
use crate::geometry::Rect;
use crate::input::{Button, Event, Key as InputKey};
use crate::theme::{Level, Theme};
use crate::widget;

const SIZE: Vec2 = Vec2::new(800.0, 600.0);

fn at(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::from_min_size(Vec2::new(x, y), Vec2::new(width, height))
}

fn body<'a>(children: Vec<Element<'a, u32>>) -> Element<'a, u32> {
    crate::place(Rect::from_min_size(Vec2::ZERO, SIZE), children)
}

fn step<F>(ui: &mut Ui<u32>, build: &F, events: Vec<Event>, time: &mut f32) -> Vec<u32>
where
    F: Fn(&Theme) -> Element<'static, u32>,
{
    *time += 0.016;
    for event in events {
        ui.input_mut().push(event);
    }
    let actions = ui.update(SIZE, 1.0, f64::from(*time));
    let theme = *ui.theme();
    ui.present(build(&theme));
    ui.finish();
    actions
}

fn click<F>(ui: &mut Ui<u32>, build: &F, target: Vec2, time: &mut f32) -> Vec<u32>
where
    F: Fn(&Theme) -> Element<'static, u32>,
{
    step(
        ui,
        build,
        vec![Event::Moved(target), Event::Down(Button::Primary)],
        time,
    );
    step(ui, build, vec![Event::Up(Button::Primary)], time)
}

fn key(key: InputKey) -> Vec<Event> {
    vec![Event::Key {
        key,
        pressed: true,
        repeat: false,
    }]
}

fn ready<F>(build: &F) -> (crate::test_gpu::TestGpu, Ui<u32>)
where
    F: Fn(&Theme) -> Element<'static, u32>,
{
    let (device, queue) = crate::test_gpu::gpu();
    let mut ui: Ui<u32> = Ui::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    ui.resize([800, 600]);
    let mut time = 0.0;
    step(&mut ui, build, Vec::new(), &mut time);
    (device, ui)
}

fn field(theme: &Theme, value: &str, editing: bool) -> Element<'static, u32> {
    let style = crate::text::TextStyle::new(theme.font(Level::Body), theme.palette.text_primary);
    widget::text_field(value, style, 200.0, false)
        .editing(editing)
        .on_commit(|text| text.len() as u32 * 10)
        .on_cancel(|| 999)
}

#[test]
fn clicks_and_activates() {
    let build = |theme: &Theme| {
        body(vec![
            widget::button(theme, "Ok", crate::theme::Role::Control)
                .at(at(10.0, 10.0, 80.0, 28.0))
                .press(1),
            widget::toggle(theme, true)
                .at(at(10.0, 60.0, 42.0, 26.0))
                .on_toggle(|value| if value { 3 } else { 2 }),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let mut time = 0.0;
    assert_eq!(
        click(&mut ui, &build, Vec2::new(50.0, 24.0), &mut time),
        [1]
    );
    assert_eq!(
        click(&mut ui, &build, Vec2::new(31.0, 73.0), &mut time),
        [2]
    );
}

#[test]
fn focuses_with_tab_and_activates_with_space() {
    let build = |theme: &Theme| {
        body(vec![
            widget::button(theme, "Ok", crate::theme::Role::Control)
                .at(at(10.0, 10.0, 80.0, 28.0))
                .press(1),
            widget::button(theme, "Off", crate::theme::Role::Control)
                .at(at(10.0, 60.0, 80.0, 28.0))
                .enabled(false)
                .press(9),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let mut time = 0.0;
    step(&mut ui, &build, key(InputKey::Tab), &mut time);
    assert!(ui.focus_visible());
    let actions = step(&mut ui, &build, key(InputKey::Space), &mut time);
    assert_eq!(actions, [1], "the disabled button never takes focus");
}

#[test]
fn drags_a_slider_and_adjusts_it_from_the_keyboard() {
    let build = |theme: &Theme| {
        body(vec![
            widget::slider_readout(theme, Some("Gain"), 0.0, 0.0..=10.0, false, "0.0")
                .at(at(10.0, 10.0, 300.0, 28.0))
                .on_slide(|value| (value * 100.0).round() as u32),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let mut time = 0.0;
    let mut actions = step(
        &mut ui,
        &build,
        vec![
            Event::Moved(Vec2::new(120.0, 24.0)),
            Event::Down(Button::Primary),
        ],
        &mut time,
    );
    actions.extend(step(
        &mut ui,
        &build,
        vec![Event::Moved(Vec2::new(290.0, 24.0))],
        &mut time,
    ));
    actions.extend(step(
        &mut ui,
        &build,
        vec![Event::Up(Button::Primary)],
        &mut time,
    ));
    assert!(
        actions.iter().copied().max().unwrap_or(0) > 700,
        "dragging reaches the end of the track: {actions:?}"
    );
    step(&mut ui, &build, key(InputKey::Tab), &mut time);
    assert!(!step(&mut ui, &build, key(InputKey::Right), &mut time).is_empty());
}

#[test]
fn drags_a_switch_knob_one_to_one() {
    let build = |theme: &Theme| {
        body(vec![
            widget::toggle_row(theme, "Switch", false)
                .named("row.switch")
                .at(at(40.0, 40.0, 300.0, 30.0))
                .on_toggle(|value| if value { 1 } else { 0 }),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let node = ui
        .tree
        .node_at(Key::new("row.switch"))
        .expect("the row switch exists");
    let mut time = 0.0;
    step(
        &mut ui,
        &build,
        vec![
            Event::Moved(Vec2::new(312.0, 55.0)),
            Event::Down(Button::Primary),
        ],
        &mut time,
    );
    step(
        &mut ui,
        &build,
        vec![Event::Moved(Vec2::new(318.0, 55.0))],
        &mut time,
    );
    let drag = ui.tree.nodes[node].runtime.drag_value;
    assert!(
        (drag - 0.56).abs() < 0.02,
        "the knob follows its own track, not the row: {drag}"
    );
    let actions = step(
        &mut ui,
        &build,
        vec![
            Event::Moved(Vec2::new(314.0, 55.0)),
            Event::Up(Button::Primary),
        ],
        &mut time,
    );
    assert!(actions.is_empty(), "the release stays on the knob's side");
    assert!(
        ui.tree.nodes[node].runtime.value.target < 0.01,
        "the knob settles where the pointer left it"
    );
    step(
        &mut ui,
        &build,
        vec![
            Event::Moved(Vec2::new(312.0, 55.0)),
            Event::Down(Button::Primary),
        ],
        &mut time,
    );
    step(
        &mut ui,
        &build,
        vec![Event::Moved(Vec2::new(330.0, 55.0))],
        &mut time,
    );
    let actions = step(&mut ui, &build, vec![Event::Up(Button::Primary)], &mut time);
    assert_eq!(actions, [1], "dragging across commits the far side");
    assert!(
        ui.tree.nodes[node].runtime.value.target > 0.99,
        "the committed side is not replayed from the old state"
    );
}

#[test]
fn drags_the_segment_lens_with_the_pointer() {
    let selected = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let state = selected.clone();
    let build = move |theme: &Theme| {
        let state = state.clone();
        let options = ["View".to_string(), "Objects".to_string()];
        body(vec![
            widget::segmented_fill(theme, &options, selected.get())
                .named("segment.drag")
                .at(at(10.0, 10.0, 300.0, 28.0))
                .on_segment(move |index| {
                    state.set(index);
                    index as u32
                }),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let node = ui
        .tree
        .node_at(Key::new("segment.drag"))
        .expect("the segment exists");
    let mut time = 0.0;
    step(
        &mut ui,
        &build,
        vec![
            Event::Moved(Vec2::new(80.0, 24.0)),
            Event::Down(Button::Primary),
        ],
        &mut time,
    );
    assert_eq!(
        step(
            &mut ui,
            &build,
            vec![Event::Moved(Vec2::new(200.0, 24.0))],
            &mut time
        ),
        [1]
    );
    let runtime = &ui.tree.nodes[node].runtime;
    let center = runtime.value.value + runtime.extent.value * 0.5;
    assert!(
        (center - 200.0).abs() < 1.0,
        "the lens stays under the pointer: {center}"
    );
    step(&mut ui, &build, vec![Event::Up(Button::Primary)], &mut time);
    assert!(
        (ui.tree.nodes[node].runtime.value.target - 160.0).abs() < 1.0,
        "the lens settles into the selected cell"
    );
    for _ in 0..40 {
        step(&mut ui, &build, Vec::new(), &mut time);
    }
    let runtime = &ui.tree.nodes[node].runtime;
    assert!((runtime.value.value - 160.0).abs() < 1.0);
    assert!((runtime.extent.value - 147.0).abs() < 1.0);
}

#[test]
fn edits_and_cancels_a_field() {
    let build = |theme: &Theme| {
        body(vec![
            field(theme, "abc", false).at(at(10.0, 10.0, 200.0, 24.0)),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let mut time = 0.0;
    click(&mut ui, &build, Vec2::new(120.0, 22.0), &mut time);
    assert!(ui.editing(), "a click starts editing");
    step(
        &mut ui,
        &build,
        vec![Event::Text("Z".to_string())],
        &mut time,
    );
    click(&mut ui, &build, Vec2::new(30.0, 22.0), &mut time);
    let actions = step(&mut ui, &build, key(InputKey::Enter), &mut time);
    assert_eq!(
        actions,
        [40],
        "the typed character survives an inside click"
    );
    assert!(!ui.editing());
    click(&mut ui, &build, Vec2::new(120.0, 22.0), &mut time);
    let actions = step(&mut ui, &build, key(InputKey::Escape), &mut time);
    assert_eq!(actions, [999]);
    assert!(!ui.editing());
}

#[test]
fn opens_and_dismisses_a_popup() {
    let anchor = Key::new("popup.anchor");
    let build = move |theme: &Theme| {
        body(vec![
            widget::button(theme, "Open", crate::theme::Role::Control)
                .at(at(10.0, 10.0, 80.0, 28.0))
                .key(anchor)
                .press(0),
            crate::popover::popup(
                anchor,
                200.0,
                vec![crate::popover::menu_row(theme, "Row", false).press(5)],
            ),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let mut time = 0.0;
    click(&mut ui, &build, Vec2::new(50.0, 24.0), &mut time);
    assert!(ui.popup_open_any());
    assert_eq!(
        click(&mut ui, &build, Vec2::new(60.0, 62.0), &mut time),
        [5]
    );
    assert!(!ui.popup_open_any());
    click(&mut ui, &build, Vec2::new(50.0, 24.0), &mut time);
    step(&mut ui, &build, key(InputKey::Escape), &mut time);
    assert!(!ui.popup_open_any());
}

#[test]
fn scrolls_and_clips_hits() {
    let scroll = Key::new("scroll.body");
    let build = move |theme: &Theme| {
        let content = crate::column()
            .height(Length::Fixed(1200.0))
            .children(
                [widget::button(theme, "Hidden", crate::theme::Role::Control)
                    .at(at(10.0, 400.0, 80.0, 28.0))
                    .press(1)],
            );
        body(vec![
            Element::new(Layout::column())
                .key(scroll)
                .at(at(10.0, 10.0, 300.0, 200.0))
                .scroll(Axis::Column, Length::Fill, None)
                .clipped()
                .children([content]),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let mut time = 0.0;
    assert!(
        click(&mut ui, &build, Vec2::new(100.0, 420.0), &mut time).is_empty(),
        "a clipped child is outside the hit region"
    );
    step(
        &mut ui,
        &build,
        vec![
            Event::Moved(Vec2::new(100.0, 100.0)),
            Event::Wheel(Vec2::new(0.0, -3.0)),
        ],
        &mut time,
    );
    step(&mut ui, &build, Vec::new(), &mut time);
    assert!(ui.build().scroll_offset(scroll) > 0.0);
}

#[test]
#[should_panic(expected = "duplicate retained key")]
fn rejects_duplicate_keys() {
    let build = |theme: &Theme| {
        body(vec![
            widget::caption(theme, "a").named("same"),
            widget::caption(theme, "b").named("same"),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let mut time = 0.0;
    step(&mut ui, &build, Vec::new(), &mut time);
}

#[test]
fn blocks_clicks_beneath_opaque_chrome() {
    let build = |theme: &Theme| {
        body(vec![
            widget::button(theme, "Ok", crate::theme::Role::Control)
                .at(at(10.0, 10.0, 80.0, 28.0))
                .press(1),
            crate::column()
                .at(at(0.0, 0.0, 200.0, 100.0))
                .background(theme.surface(crate::theme::Role::Card, crate::style::State::Rest)),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let mut time = 0.0;
    assert!(
        click(&mut ui, &build, Vec2::new(50.0, 24.0), &mut time).is_empty(),
        "opaque chrome swallows the click"
    );
}

#[test]
fn keeps_one_glass_item_per_widget() {
    let selected = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let state = selected.clone();
    let build = move |theme: &Theme| {
        let state = state.clone();
        let options = ["One".to_string(), "Two".to_string(), "Three".to_string()];
        body(vec![
            widget::segmented_fill(theme, &options, selected.get())
                .named("segment.test")
                .on_segment(move |index| {
                    state.set(index);
                    index as u32
                }),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let mut time = 0.0;
    click(&mut ui, &build, Vec2::new(160.0, 24.0), &mut time);
    let node = ui
        .tree
        .node_at(Key::new("segment.test"))
        .expect("the segment exists");
    for _ in 0..20 {
        step(&mut ui, &build, Vec::new(), &mut time);
        assert_eq!(
            ui.tree.nodes[node].sub_glass.len(),
            1,
            "a widget owns exactly one lens glass"
        );
    }
}

#[test]
fn grows_a_popup_from_its_anchor() {
    let anchor = Key::new("grow.anchor");
    let build = move |theme: &Theme| {
        body(vec![
            widget::button(theme, "Open", crate::theme::Role::Control)
                .at(at(10.0, 10.0, 80.0, 28.0))
                .key(anchor)
                .press(0),
            crate::popover::popup(
                anchor,
                200.0,
                vec![crate::popover::menu_row(theme, "Row", false).press(5)],
            ),
        ])
    };
    let (_device, mut ui) = ready(&build);
    let mut time = 0.0;
    click(&mut ui, &build, Vec2::new(50.0, 24.0), &mut time);
    let popup = *ui
        .popup_anchors
        .get(&anchor)
        .expect("the popup is anchored");
    let body = ui.tree.nodes[popup].rect;
    let (window, _) = ui.popup_windows[&popup];
    assert!(
        window.height() < body.height() - 4.0,
        "the window starts at the anchor: {window:?} vs {body:?}"
    );
    for _ in 0..40 {
        step(&mut ui, &build, Vec::new(), &mut time);
    }
    let (window, _) = ui.popup_windows[&popup];
    assert!(
        (window.height() - body.height()).abs() < 1.0,
        "the window reaches the popover body: {window:?} vs {body:?}"
    );
    click(&mut ui, &build, Vec2::new(700.0, 500.0), &mut time);
    assert!(!ui.popup_glass.contains_key(&popup));
    assert!(!ui.popup_retire.is_empty());
}
