#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum WheelDirection {
    Up,
    Down,
    Left,
    Right,
}

impl WheelDirection {
    pub(crate) fn from_delta(dx: f32, dy: f32) -> Option<Self> {
        if dx.abs() > dy.abs() {
            Some(if dx > 0.0 { Self::Right } else { Self::Left })
        } else if dy != 0.0 {
            Some(if dy > 0.0 { Self::Up } else { Self::Down })
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { ctrl: false, shift: false, alt: false, meta: false };

    pub fn any(&self) -> bool {
        self.ctrl || self.shift || self.alt || self.meta
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Backspace,
    Delete,
    Insert,
    Tab,
    Space,
    F(u8),
    Char(char),
    NumPlus,
    NumMinus,
    NumStar,
    NumSlash,
    Apps,
    Control,
    Shift,
    Alt,
    Meta,
    Mouse(MouseButton),
    Wheel(WheelDirection),
    ModifiersOnly,
    Other,
}

impl Key {
    pub fn is_modifier(self) -> bool {
        matches!(self, Self::Control | Self::Shift | Self::Alt | Self::Meta)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    pub cursor: Option<usize>,
}

impl Preedit {
    pub fn new(text: String, cursor: Option<usize>) -> Self {
        let cursor = cursor.filter(|&c| text.is_char_boundary(c));
        Self { text, cursor }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Resized { width: u32, height: u32 },
    ScaleChanged(f32),
    MouseMove { x: f32, y: f32 },
    MouseDown { x: f32, y: f32, button: MouseButton, clicks: u8 },
    MouseUp { x: f32, y: f32, button: MouseButton },
    Modifiers(Mods),
    Wheel { dx: f32, dy: f32 },
    MouseLeave,
    Key { key: Key, mods: Mods, pressed: bool },
    Char(char),
    AltChar(char),
    Pasted(String),
    VaultPasted { input: u64, text: String },
    Preedit(Preedit),
    Focus(bool),
    ThemeChanged { dark: bool },
    DroppedFiles(Vec<String>),
    CloseRequested,
    Timer,
    SessionEnding,
    GlobalHotkey,
    NotificationClicked(u64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorIcon {
    Default,
    Text,
    Hand,
    ResizeHorizontal,
    ResizeVertical,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitArea {
    Client,
    Caption,
    Minimize,
    Maximize,
    Close,
}
