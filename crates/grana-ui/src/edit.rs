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
pub struct Edit {
    key: u64,
    text: String,
    anchor: usize,
    caret: usize,
    active_at: f32,
    clicks: u32,
    last_click: Option<(f32, Vec2)>,
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

    pub fn insert(&mut self, text: &str, time: f32) {
        let text = text.replace(['\n', '\r', '\t'], " ");
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
            if modifiers.control || modifiers.logo {
                let Key::Character(character) = key else {
                    continue;
                };
                match character.to_ascii_lowercase() {
                    'a' => self.select_all(time),
                    'c' => self.copy(clipboard),
                    'x' => self.cut(clipboard, time),
                    'v' => self.paste(clipboard, time),
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
                Key::Backspace => self.erase_before(time),
                Key::Delete => self.erase_after(time),
                Key::Left => self.step(-1, modifiers.shift, time),
                Key::Right => self.step(1, modifiers.shift, time),
                Key::Home => self.jump(0, modifiers.shift, time),
                Key::End => self.jump(self.length(), modifiers.shift, time),
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
            let start = self.offset(self.caret);
            let end = self.offset(self.caret + 1);
            self.text.replace_range(start..end, "");
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

    fn jump(&mut self, target: usize, extend: bool, time: f32) {
        self.place(target, extend, time);
    }
}
