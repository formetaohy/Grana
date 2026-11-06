use glam::Vec2;
use grana_ui::{Clipboard, Edit, EditOutcome, Event, Input, InputKey as Key, Modifiers};

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

fn shortcut(input: &mut Input, character: char) {
    input.push(Event::Modifiers(Modifiers {
        control: true,
        ..Default::default()
    }));
    press(input, Key::Character(character));
}

#[derive(Default)]
struct FakeClipboard {
    text: Option<String>,
}

impl Clipboard for FakeClipboard {
    fn read(&mut self) -> Option<String> {
        self.text.clone()
    }

    fn write(&mut self, text: &str) {
        self.text = Some(text.to_string());
    }
}

#[test]
fn types_replaces_and_commits_a_name() {
    let mut input = Input::new();
    let mut clipboard = FakeClipboard::default();
    let mut edit = Edit::new(7, "Cells 1", 0.0);
    assert_eq!(
        edit.selection(),
        Some((0, 7)),
        "activation selects the name"
    );

    press(&mut input, Key::Character('N'));
    step(&mut input, 0.1);
    assert_eq!(edit.poll(&input, &mut clipboard, 0.1), None);
    assert_eq!(edit.text(), "N");
    assert_eq!(edit.caret(), 1);

    for character in ['u', 'c'] {
        press(&mut input, Key::Character(character));
        step(&mut input, 0.2);
        assert!(edit.poll(&input, &mut clipboard, 0.2).is_none());
    }
    assert_eq!(edit.text(), "Nuc");

    press(&mut input, Key::Left);
    step(&mut input, 0.3);
    assert!(edit.poll(&input, &mut clipboard, 0.3).is_none());
    press(&mut input, Key::Backspace);
    step(&mut input, 0.4);
    assert!(edit.poll(&input, &mut clipboard, 0.4).is_none());
    assert_eq!(edit.text(), "Nc");
    assert_eq!(edit.caret(), 1);

    press(&mut input, Key::End);
    step(&mut input, 0.5);
    assert!(edit.poll(&input, &mut clipboard, 0.5).is_none());
    press(&mut input, Key::Character('s'));
    step(&mut input, 0.6);
    assert!(edit.poll(&input, &mut clipboard, 0.6).is_none());
    assert_eq!(edit.text(), "Ncs");

    press(&mut input, Key::Enter);
    step(&mut input, 0.7);
    assert_eq!(
        edit.poll(&input, &mut clipboard, 0.7),
        Some(EditOutcome::Commit)
    );

    press(&mut input, Key::Escape);
    step(&mut input, 0.8);
    assert_eq!(
        edit.poll(&input, &mut clipboard, 0.8),
        Some(EditOutcome::Cancel)
    );
}

#[test]
fn copies_cuts_and_pastes_through_the_clipboard() {
    let mut input = Input::new();
    let mut clipboard = FakeClipboard::default();
    let mut edit = Edit::new(3, "Nucleus", 0.0);

    shortcut(&mut input, 'c');
    step(&mut input, 0.1);
    assert!(edit.poll(&input, &mut clipboard, 0.1).is_none());
    assert_eq!(
        clipboard.text.as_deref(),
        Some("Nucleus"),
        "the selection reaches the clipboard"
    );

    shortcut(&mut input, 'x');
    step(&mut input, 0.2);
    assert!(edit.poll(&input, &mut clipboard, 0.2).is_none());
    assert_eq!(edit.text(), "", "cutting removes the selection");

    shortcut(&mut input, 'v');
    step(&mut input, 0.3);
    assert!(edit.poll(&input, &mut clipboard, 0.3).is_none());
    assert_eq!(edit.text(), "Nucleus", "pasting inserts the clipboard");
}

#[test]
fn keeps_a_preedit_beside_the_committed_text() {
    let mut input = Input::new();
    let mut clipboard = FakeClipboard::default();
    let mut edit = Edit::new(5, "ab", 0.0);
    edit.place(0, false, 0.0);

    input.push(Event::Preedit(Some("细".to_string())));
    step(&mut input, 0.1);
    assert!(edit.poll(&input, &mut clipboard, 0.1).is_none());
    assert_eq!(edit.text(), "ab", "a preedit is not committed");

    input.push(Event::Text("细胞".to_string()));
    input.push(Event::Preedit(None));
    step(&mut input, 0.2);
    assert!(edit.poll(&input, &mut clipboard, 0.2).is_none());
    assert_eq!(edit.text(), "细胞ab", "a committed composition inserts");
}

#[test]
fn navigates_multiline_text() {
    let mut input = Input::new();
    let mut clipboard = FakeClipboard::default();
    let mut edit = Edit::multiline(13, "ab\ncde\nf", 0.0);
    edit.place(1, false, 0.0);

    press(&mut input, Key::Down);
    step(&mut input, 0.1);
    assert!(edit.poll(&input, &mut clipboard, 0.1).is_none());
    assert_eq!(edit.caret(), 4);

    press(&mut input, Key::End);
    step(&mut input, 0.2);
    assert!(edit.poll(&input, &mut clipboard, 0.2).is_none());
    assert_eq!(edit.caret(), 6);

    press(&mut input, Key::Down);
    step(&mut input, 0.3);
    assert!(edit.poll(&input, &mut clipboard, 0.3).is_none());
    assert_eq!(edit.caret(), 8);

    press(&mut input, Key::Home);
    step(&mut input, 0.4);
    assert!(edit.poll(&input, &mut clipboard, 0.4).is_none());
    assert_eq!(edit.caret(), 7);

    press(&mut input, Key::Enter);
    step(&mut input, 0.5);
    assert!(edit.poll(&input, &mut clipboard, 0.5).is_none());
    assert_eq!(edit.text(), "ab\ncde\n\nf");
}

#[test]
fn counts_clicks_and_selects_words() {
    let mut edit = Edit::new(9, "nucleus 42 / cell", 0.0);
    assert_eq!(edit.click(Vec2::splat(10.0), 0.0), 1);
    assert_eq!(edit.click(Vec2::splat(10.0), 0.2), 2);
    assert_eq!(edit.click(Vec2::splat(10.0), 0.4), 3);
    assert_eq!(
        edit.click(Vec2::splat(200.0), 0.5),
        1,
        "a far click restarts"
    );
    assert_eq!(edit.word(3), (0, 7), "a word runs to its boundary");
    assert_eq!(edit.word(9), (8, 10), "digits form a word");
    assert_eq!(edit.word(11), (11, 12), "a symbol is its own run");
}

#[test]
fn undoes_and_redoes_edits() {
    let mut input = Input::new();
    let mut clipboard = FakeClipboard::default();
    let mut edit = Edit::new(11, "abc", 0.0);

    input.push(Event::Text("x".to_string()));
    step(&mut input, 0.1);
    assert!(edit.poll(&input, &mut clipboard, 0.1).is_none());
    assert_eq!(edit.text(), "x");

    shortcut(&mut input, 'z');
    step(&mut input, 0.2);
    assert!(edit.poll(&input, &mut clipboard, 0.2).is_none());
    assert_eq!(edit.text(), "abc");

    shortcut(&mut input, 'y');
    step(&mut input, 0.3);
    assert!(edit.poll(&input, &mut clipboard, 0.3).is_none());
    assert_eq!(edit.text(), "x");
}

#[test]
fn moves_and_erases_by_word() {
    let mut input = Input::new();
    let mut clipboard = FakeClipboard::default();
    let mut edit = Edit::new(12, "alpha beta gamma", 0.0);
    edit.place(0, false, 0.0);
    input.push(Event::Modifiers(Modifiers {
        control: true,
        ..Default::default()
    }));

    press(&mut input, Key::Right);
    step(&mut input, 0.1);
    assert!(edit.poll(&input, &mut clipboard, 0.1).is_none());
    assert_eq!(edit.caret(), 5);

    press(&mut input, Key::Right);
    step(&mut input, 0.2);
    assert!(edit.poll(&input, &mut clipboard, 0.2).is_none());
    assert_eq!(edit.caret(), 10);

    press(&mut input, Key::Left);
    step(&mut input, 0.3);
    assert!(edit.poll(&input, &mut clipboard, 0.3).is_none());
    assert_eq!(edit.caret(), 6);

    press(&mut input, Key::Delete);
    step(&mut input, 0.4);
    assert!(edit.poll(&input, &mut clipboard, 0.4).is_none());
    assert_eq!(edit.text(), "alpha  gamma");
    assert_eq!(edit.caret(), 6);
}
