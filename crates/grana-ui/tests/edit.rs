use glam::Vec2;
use grana_ui::{Edit, EditOutcome, Event, Input, Key};

fn step(input: &mut Input, time: f64) {
    input.begin(Vec2::new(400.0, 300.0), 1.0, time);
}

fn press(input: &mut Input, key: Key) {
    input.push(Event::Key {
        key,
        pressed: true,
        repeat: false,
    });
}

#[test]
fn types_replaces_and_commits_a_name() {
    let mut input = Input::new();
    let mut edit = Edit::new(7, "Cells 1", 0.0);
    assert_eq!(
        edit.selection(),
        Some((0, 7)),
        "activation selects the name"
    );

    press(&mut input, Key::Character('N'));
    step(&mut input, 0.1);
    assert_eq!(edit.poll(&input, 0.1), None);
    assert_eq!(edit.text(), "N");
    assert_eq!(edit.caret(), 1);

    for character in ['u', 'c'] {
        press(&mut input, Key::Character(character));
        step(&mut input, 0.2);
        assert!(edit.poll(&input, 0.2).is_none());
    }
    assert_eq!(edit.text(), "Nuc");

    press(&mut input, Key::Left);
    step(&mut input, 0.3);
    assert!(edit.poll(&input, 0.3).is_none());
    press(&mut input, Key::Backspace);
    step(&mut input, 0.4);
    assert!(edit.poll(&input, 0.4).is_none());
    assert_eq!(edit.text(), "Nc");
    assert_eq!(edit.caret(), 1);

    press(&mut input, Key::End);
    step(&mut input, 0.5);
    assert!(edit.poll(&input, 0.5).is_none());
    press(&mut input, Key::Character('s'));
    step(&mut input, 0.6);
    assert!(edit.poll(&input, 0.6).is_none());
    assert_eq!(edit.text(), "Ncs");

    press(&mut input, Key::Enter);
    step(&mut input, 0.7);
    assert_eq!(edit.poll(&input, 0.7), Some(EditOutcome::Commit));

    press(&mut input, Key::Escape);
    step(&mut input, 0.8);
    assert_eq!(edit.poll(&input, 0.8), Some(EditOutcome::Cancel));
}
