use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Primary,
    Secondary,
    Middle,
}

impl Button {
    fn index(self) -> usize {
        match self {
            Self::Primary => 0,
            Self::Secondary => 1,
            Self::Middle => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Escape,
    Enter,
    Tab,
    Space,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
    Character(char),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub logo: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Moved(Vec2),
    Down(Button),
    Up(Button),
    Leave,
    Wheel(Vec2),
    Modifiers(Modifiers),
    Key {
        key: Key,
        pressed: bool,
        repeat: bool,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ButtonState {
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KeyEvent {
    pub key: Option<Key>,
    pub pressed: bool,
    pub repeat: bool,
}

pub struct Input {
    position: Vec2,
    inside: bool,
    delta: Vec2,
    wheel: Vec2,
    buttons: [ButtonState; 3],
    modifiers: Modifiers,
    events: Vec<Event>,
    keys: Vec<KeyEvent>,
    viewport: Vec2,
    scale: f32,
    time: f64,
    elapsed: f32,
}

impl Input {
    pub fn new() -> Self {
        Self {
            position: Vec2::ZERO,
            inside: false,
            delta: Vec2::ZERO,
            wheel: Vec2::ZERO,
            buttons: [ButtonState::default(); 3],
            modifiers: Modifiers::default(),
            events: Vec::new(),
            keys: Vec::new(),
            viewport: Vec2::ZERO,
            scale: 1.0,
            time: 0.0,
            elapsed: 0.0,
        }
    }

    pub fn push(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn begin(&mut self, viewport: Vec2, scale: f32, time: f64) -> f32 {
        self.viewport = viewport;
        self.scale = scale;
        let elapsed = if self.time > 0.0 {
            (time - self.time).clamp(0.0, 0.25) as f32
        } else {
            0.0
        };
        self.time = time;
        self.elapsed = elapsed;
        self.delta = Vec2::ZERO;
        self.wheel = Vec2::ZERO;
        self.keys.clear();
        for button in self.buttons.iter_mut() {
            button.pressed = false;
            button.released = false;
        }
        for event in std::mem::take(&mut self.events) {
            match event {
                Event::Moved(position) => {
                    if self.inside {
                        self.delta += position - self.position;
                    }
                    self.position = position;
                    self.inside = true;
                }
                Event::Down(button) => {
                    let state = &mut self.buttons[button.index()];
                    state.down = true;
                    state.pressed = true;
                }
                Event::Up(button) => {
                    let state = &mut self.buttons[button.index()];
                    state.down = false;
                    state.released = true;
                }
                Event::Leave => self.inside = false,
                Event::Wheel(delta) => self.wheel += delta,
                Event::Modifiers(modifiers) => self.modifiers = modifiers,
                Event::Key {
                    key,
                    pressed,
                    repeat,
                } => self.keys.push(KeyEvent {
                    key: Some(key),
                    pressed,
                    repeat,
                }),
            }
        }
        elapsed
    }

    pub fn position(&self) -> Vec2 {
        self.position
    }

    pub fn inside(&self) -> bool {
        self.inside
    }

    pub fn delta(&self) -> Vec2 {
        self.delta
    }

    pub fn wheel(&self) -> Vec2 {
        self.wheel
    }

    pub fn button(&self, button: Button) -> ButtonState {
        self.buttons[button.index()]
    }

    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    pub fn keys(&self) -> &[KeyEvent] {
        &self.keys
    }

    pub fn viewport(&self) -> Vec2 {
        self.viewport
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}
