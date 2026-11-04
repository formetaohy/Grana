use glam::Vec2;

use crate::clipboard::Clipboard;
use crate::input::{Input, Key};

const BLINK_PERIOD: f32 = 1.06;
const BLINK_DUTY: f32 = 0.53;
const MULTI_CLICK_SECONDS: f32 = 0.35;
const MULTI_CLICK_SLOP: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Commit,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Symbol,
}

fn class(character: char) -> Class {
    if character.is_alphanumeric() || character == '_' {
        Class::Word
    } else if character.is_whitespace() {
        Class::Space
    } else {
        Class::Symbol
    }
}

#[derive(Clone, Debug)]
struct Snapshot {
    text: String,
    anchor: usize,
    caret: usize,
}

#[derive(Clone, Debug)]
pub struct Edit {
    key: u64,
    text: String,
    anchor: usize,
    caret: usize,
    active_at: f32,
    clicks: u32,
    last_click: Option<(f32, Vec2)>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    multiline: bool,
}

impl Edit {
    pub fn new(key: u64, text: &str, time: f32) -> Self {
        Self {
            key,
            text: text.to_string(),
            anchor: 0,
            caret: text.chars().count(),
            active_at: time,
            clicks: 0,
            last_click: None,
            undo: Vec::new(),
            redo: Vec::new(),
            multiline: false,
        }
    }

    pub fn multiline(key: u64, text: &str, time: f32) -> Self {
        Self {
            multiline: true,
            ..Self::new(key, text, time)
        }
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn anchor(&self) -> usize {
        self.anchor
    }

    pub fn length(&self) -> usize {
        self.text.chars().count()
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        (self.caret != self.anchor)
            .then(|| (self.caret.min(self.anchor), self.caret.max(self.anchor)))
    }

    pub fn caret_visible(&self, time: f32) -> bool {
        (time - self.active_at).rem_euclid(BLINK_PERIOD) < BLINK_DUTY
    }

    fn offset(&self, index: usize) -> usize {
        let index = index.min(self.length());
        let mut offset = self.text.len();
        for (count, (position, _)) in self.text.char_indices().enumerate() {
            if count == index {
                offset = position;
                break;
            }
        }
        offset
    }

    pub fn place(&mut self, caret: usize, extend: bool, time: f32) {
        self.caret = caret.min(self.length());
        if !extend {
            self.anchor = self.caret;
        }
        self.active_at = time;
    }

    pub fn select(&mut self, range: (usize, usize), time: f32) {
        self.anchor = range.0.min(self.length());
        self.caret = range.1.min(self.length());
        self.active_at = time;
    }

    pub fn select_all(&mut self, time: f32) {
        self.anchor = 0;
        self.caret = self.length();
        self.active_at = time;
    }

    pub fn click(&mut self, point: Vec2, time: f32) -> u32 {
        let count = match self.last_click {
            Some((last, at))
                if time - last <= MULTI_CLICK_SECONDS
                    && (point - at).length() <= MULTI_CLICK_SLOP =>
            {
                self.clicks % 3 + 1
            }
            _ => 1,
        };
        self.last_click = Some((time, point));
        self.clicks = count;
        count
    }

    pub fn word(&self, caret: usize) -> (usize, usize) {
        let characters = self.text.chars().collect::<Vec<_>>();
        if characters.is_empty() {
            return (0, 0);
        }
        let index = caret.min(characters.len() - 1);
        let kind = class(characters[index]);
        let mut start = index;
        while start > 0 && class(characters[start - 1]) == kind {
            start -= 1;
        }
        let mut end = index + 1;
        while end < characters.len() && class(characters[end]) == kind {
            end += 1;
        }
        (start, end)
    }

    fn previous_word(&self, caret: usize) -> usize {
        let characters = self.text.chars().collect::<Vec<_>>();
        let mut index = caret.min(characters.len());
        while index > 0 && class(characters[index - 1]) == Class::Space {
            index -= 1;
        }
        if index == 0 {
            return 0;
        }
        let kind = class(characters[index - 1]);
        while index > 0 && class(characters[index - 1]) == kind {
            index -= 1;
        }
        index
    }

    fn next_word(&self, caret: usize) -> usize {
        let characters = self.text.chars().collect::<Vec<_>>();
        let mut index = caret.min(characters.len());
        while index < characters.len() && class(characters[index]) == Class::Space {
            index += 1;
        }
        if index == characters.len() {
            return index;
        }
        let kind = class(characters[index]);
        while index < characters.len() && class(characters[index]) == kind {
            index += 1;
        }
        index
    }

    fn line_span(&self, line: usize) -> (usize, usize) {
        let characters = self.text.chars().collect::<Vec<_>>();
        let mut start = 0;
        let mut current = 0;
        for (index, character) in characters.iter().enumerate() {
            if current == line {
                if *character == '\n' {
                    return (start, index);
                }
            } else if *character == '\n' {
                current += 1;
                start = index + 1;
            }
        }
        (start, characters.len())
    }

    fn caret_line(&self) -> (usize, usize) {
        let characters = self.text.chars().collect::<Vec<_>>();
        let caret = self.caret.min(characters.len());
        let mut line = 0;
        let mut start = 0;
        for (index, character) in characters.iter().enumerate().take(caret) {
            if *character == '\n' {
                line += 1;
                start = index + 1;
            }
        }
        (line, caret - start)
    }

    fn vertical(&mut self, delta: i32, extend: bool, time: f32) {
        let (line, column) = self.caret_line();
        let characters = self.text.chars().collect::<Vec<_>>();
        let count = characters
            .iter()
            .filter(|character| **character == '\n')
            .count()
            + 1;
        let target = (line as i32 + delta).clamp(0, count as i32 - 1) as usize;
        let (start, end) = self.line_span(target);
        self.place((start + column).min(end), extend, time);
    }

    fn home(&mut self, extend: bool, time: f32) {
        if self.multiline {
            let (start, _) = self.line_span(self.caret_line().0);
            self.place(start, extend, time);
        } else {
            self.place(0, extend, time);
        }
    }

    fn end(&mut self, extend: bool, time: f32) {
        if self.multiline {
            let (_, end) = self.line_span(self.caret_line().0);
            self.place(end, extend, time);
        } else {
            self.place(self.length(), extend, time);
        }
    }

    fn remember(&mut self) {
        self.undo.push(self.snapshot());
        if self.undo.len() > 128 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            anchor: self.anchor,
            caret: self.caret,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.text = snapshot.text;
        self.anchor = snapshot.anchor;
        self.caret = snapshot.caret;
    }

    fn undo(&mut self, time: f32) {
        let Some(snapshot) = self.undo.pop() else {
            return;
        };
        self.redo.push(self.snapshot());
        self.restore(snapshot);
        self.active_at = time;
    }

    fn redo(&mut self, time: f32) {
        let Some(snapshot) = self.redo.pop() else {
            return;
        };
        self.undo.push(self.snapshot());
        self.restore(snapshot);
        self.active_at = time;
    }

    pub fn insert(&mut self, text: &str, time: f32) {
        let text = if self.multiline {
            text.replace(['\r', '\t'], " ")
        } else {
            text.replace(['\n', '\r', '\t'], " ")
        };
        if text.is_empty() {
            return;
        }
        self.replace(&text, time);
    }

    pub fn poll(
        &mut self,
        input: &Input,
        clipboard: &mut dyn Clipboard,
        time: f32,
    ) -> Option<Outcome> {
        for text in input.texts() {
            self.insert(text, time);
        }
        let modifiers = input.modifiers();
        for event in input.keys() {
            if !event.pressed {
                continue;
            }
            let Some(key) = event.key else {
                continue;
            };
            if (modifiers.control || modifiers.logo)
                && let Key::Character(character) = key
            {
                match character.to_ascii_lowercase() {
                    'a' => self.select_all(time),
                    'c' => self.copy(clipboard),
                    'x' => self.cut(clipboard, time),
                    'v' => self.paste(clipboard, time),
                    'z' if modifiers.shift => self.redo(time),
                    'z' => self.undo(time),
                    'y' => self.redo(time),
                    _ => {}
                }
                continue;
            }
            match key {
                Key::Character(character) => {
                    if character.is_control() {
                        continue;
                    }
                    self.insert(&character.to_string(), time);
                }
                Key::Backspace if modifiers.control || modifiers.logo => {
                    self.erase_word_before(time);
                }
                Key::Backspace => self.erase_before(time),
                Key::Delete if modifiers.control || modifiers.logo => self.erase_word_after(time),
                Key::Delete => self.erase_after(time),
                Key::Left if modifiers.control || modifiers.logo => {
                    let target = self.previous_word(self.caret);
                    self.place(target, modifiers.shift, time);
                }
                Key::Left => self.step(-1, modifiers.shift, time),
                Key::Right if modifiers.control || modifiers.logo => {
                    let target = self.next_word(self.caret);
                    self.place(target, modifiers.shift, time);
                }
                Key::Right => self.step(1, modifiers.shift, time),
                Key::Home => self.home(modifiers.shift, time),
                Key::End => self.end(modifiers.shift, time),
                Key::Up if self.multiline => self.vertical(-1, modifiers.shift, time),
                Key::Down if self.multiline => self.vertical(1, modifiers.shift, time),
                Key::Enter if self.multiline => self.insert("\n", time),
                Key::Enter | Key::Tab => return Some(Outcome::Commit),
                Key::Escape => return Some(Outcome::Cancel),
                Key::Space | Key::Up | Key::Down | Key::PageUp | Key::PageDown => {}
            }
        }
        None
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.text[self.offset(start)..self.offset(end)].to_string())
    }

    fn copy(&self, clipboard: &mut dyn Clipboard) {
        let Some(text) = self.selected_text() else {
            return;
        };
        clipboard.write(&text);
    }

    fn cut(&mut self, clipboard: &mut dyn Clipboard, time: f32) {
        let Some(text) = self.selected_text() else {
            return;
        };
        clipboard.write(&text);
        self.replace("", time);
    }

    fn paste(&mut self, clipboard: &mut dyn Clipboard, time: f32) {
        if let Some(text) = clipboard.read() {
            self.insert(&text, time);
        }
    }

    fn replace(&mut self, text: &str, time: f32) {
        self.remember();
        let (start, end) = match self.selection() {
            Some((start, end)) => (start, end),
            None => (self.caret, self.caret),
        };
        let start_offset = self.offset(start);
        let end_offset = self.offset(end);
        self.text.replace_range(start_offset..end_offset, text);
        self.caret = start + text.chars().count();
        self.anchor = self.caret;
        self.active_at = time;
    }

    fn erase_before(&mut self, time: f32) {
        if self.selection().is_some() {
            self.replace("", time);
        } else if self.caret > 0 {
            self.remember();
            let end = self.offset(self.caret);
            let start = self.offset(self.caret - 1);
            self.text.replace_range(start..end, "");
            self.caret -= 1;
            self.anchor = self.caret;
            self.active_at = time;
        }
    }

    fn erase_after(&mut self, time: f32) {
        if self.selection().is_some() {
            self.replace("", time);
        } else if self.caret < self.length() {
            self.remember();
            let start = self.offset(self.caret);
            let end = self.offset(self.caret + 1);
            self.text.replace_range(start..end, "");
            self.active_at = time;
        }
    }

    fn erase_word_before(&mut self, time: f32) {
        if self.selection().is_some() {
            self.replace("", time);
            return;
        }
        let start = self.previous_word(self.caret);
        if start < self.caret {
            self.remember();
            let start_offset = self.offset(start);
            let end_offset = self.offset(self.caret);
            self.text.replace_range(start_offset..end_offset, "");
            self.caret = start;
            self.anchor = start;
            self.active_at = time;
        }
    }

    fn erase_word_after(&mut self, time: f32) {
        if self.selection().is_some() {
            self.replace("", time);
            return;
        }
        let end = self.next_word(self.caret);
        if self.caret < end {
            self.remember();
            let start_offset = self.offset(self.caret);
            let end_offset = self.offset(end);
            self.text.replace_range(start_offset..end_offset, "");
            self.active_at = time;
        }
    }

    fn step(&mut self, delta: i32, extend: bool, time: f32) {
        let caret = if extend {
            self.caret
        } else if let Some((start, end)) = self.selection() {
            if delta < 0 { start } else { end }
        } else {
            self.caret
        };
        let caret = (caret as i32 + delta).clamp(0, self.length() as i32) as usize;
        self.place(caret, extend, time);
    }
}
