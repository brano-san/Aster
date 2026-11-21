use crate::input::{Key, Mods};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    CommandSelector,
    ProfileSelector,
    ToggleFullscreen,
    NewTab,
    NewWindow,
    CloseTab,
    DuplicateTab,
    NextTab,
    PreviousTab,
    NextMruTab,
    PreviousMruTab,
    MoveTabLeft,
    MoveTabRight,
    Tab(u8),
    SplitRight,
    SplitBottom,
    SplitLeft,
    SplitTop,
    PaneNavUp,
    PaneNavDown,
    PaneNavLeft,
    PaneNavRight,
    PaneNavPrevious,
    PaneNavNext,
    PaneMaximize,
    ClosePane,
    Copy,
    Paste,
    SelectAll,
    Clear,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    CtrlC,
    DeletePreviousWord,
    DeleteNextWord,
    DeleteLine,
    InsertNewLine,
    ScrollToTop,
    ScrollToBottom,
    ScrollPageUp,
    ScrollPageDown,
    ScrollUp,
    ScrollDown,
    SelectColorScheme,
    Search,
    Settings,
    RenameTab,
    PinTab,
    CloseOtherTabs,
    CloseTabsToRight,
    CloseTabsToLeft,
    OpenFiles,
    SerialSpeed,
    SerialBreak,
    FillPassword,
    NewAdminTab,
    DuplicateAsAdmin,
    ToggleLastTab,
    ReopenTab,
    PaneResizeLeft,
    PaneResizeRight,
    PaneResizeUp,
    PaneResizeDown,
    ExplodeTab,
    CombineTabs,
    BroadcastToTab,
    BroadcastToAllTabs,
    SetTabColor,
    ReportProblem,
    DebugCopyState,
    DebugSaveOutput,
    ExportBuffer,
    NotifyCommand,
    OpenLog,
    OpenConfigFile,
    ToggleTabBar,
}

const NAMED: &[(Action, &str, &str)] = &[
    (Action::CommandSelector, "command-selector", "Show command selector"),
    (Action::ProfileSelector, "profile-selector", "Profiles and connections"),
    (Action::ToggleFullscreen, "toggle-fullscreen", "Toggle fullscreen"),
    (Action::NewTab, "new-tab", "New tab"),
    (Action::NewWindow, "new-window", "New window"),
    (Action::CloseTab, "close-tab", "Close tab"),
    (Action::DuplicateTab, "duplicate-tab", "Duplicate tab"),
    (Action::NextTab, "next-tab", "Next tab"),
    (Action::PreviousTab, "previous-tab", "Previous tab"),
    (Action::NextMruTab, "next-mru-tab", "Switch to the recently used tab"),
    (Action::PreviousMruTab, "previous-mru-tab", "Switch to the recently used tab in reverse order"),
    (Action::MoveTabLeft, "move-tab-left", "Move tab to the left"),
    (Action::MoveTabRight, "move-tab-right", "Move tab to the right"),
    (Action::SplitRight, "split-right", "Split to the right"),
    (Action::SplitBottom, "split-bottom", "Split to the bottom"),
    (Action::SplitLeft, "split-left", "Split to the left"),
    (Action::SplitTop, "split-top", "Split to the top"),
    (Action::PaneNavUp, "pane-nav-up", "Focus the pane above"),
    (Action::PaneNavDown, "pane-nav-down", "Focus the pane below"),
    (Action::PaneNavLeft, "pane-nav-left", "Focus the pane on the left"),
    (Action::PaneNavRight, "pane-nav-right", "Focus the pane on the right"),
    (Action::PaneNavPrevious, "pane-nav-previous", "Focus the previous pane"),
    (Action::PaneNavNext, "pane-nav-next", "Focus the next pane"),
    (Action::PaneMaximize, "pane-maximize", "Maximize the active pane"),
    (Action::ClosePane, "close-pane", "Close the active pane"),
    (Action::Copy, "copy", "Copy"),
    (Action::Paste, "paste", "Paste"),
    (Action::SelectAll, "select-all", "Select all"),
    (Action::Clear, "clear", "Clear terminal"),
    (Action::ZoomIn, "zoom-in", "Zoom in"),
    (Action::ZoomOut, "zoom-out", "Zoom out"),
    (Action::ResetZoom, "reset-zoom", "Reset zoom"),
    (Action::CtrlC, "ctrl-c", "Copy or interrupt"),
    (Action::DeletePreviousWord, "delete-previous-word", "Delete previous word"),
    (Action::DeleteNextWord, "delete-next-word", "Delete next word"),
    (Action::DeleteLine, "delete-line", "Delete line"),
    (Action::InsertNewLine, "insert-new-line", "Insert new line"),
    (Action::ScrollToTop, "scroll-to-top", "Scroll to top"),
    (Action::ScrollToBottom, "scroll-to-bottom", "Scroll to bottom"),
    (Action::ScrollPageUp, "scroll-page-up", "Scroll one page up"),
    (Action::ScrollPageDown, "scroll-page-down", "Scroll one page down"),
    (Action::ScrollUp, "scroll-up", "Scroll up"),
    (Action::ScrollDown, "scroll-down", "Scroll down"),
    (Action::SelectColorScheme, "select-color-scheme", "Select color scheme"),
    (Action::Search, "search", "Search"),
    (Action::Settings, "settings", "Open settings"),
    (Action::RenameTab, "rename-tab", "Rename tab"),
    (Action::PinTab, "pin-tab", "Pin or unpin the tab"),
    (Action::CloseOtherTabs, "close-other-tabs", "Close other tabs"),
    (Action::CloseTabsToRight, "close-tabs-to-the-right", "Close tabs to the right"),
    (Action::CloseTabsToLeft, "close-tabs-to-the-left", "Close tabs to the left"),
    (Action::OpenFiles, "open-files", "Remote files (SFTP)"),
    (Action::SerialSpeed, "serial-speed", "Change the serial port speed"),
    (Action::SerialBreak, "serial-break", "Send a break on the serial line"),
    (Action::FillPassword, "fill-password", "Type or save the password for this prompt"),
    (Action::NewAdminTab, "new-admin-tab", "New administrator tab"),
    (Action::DuplicateAsAdmin, "duplicate-as-admin", "Duplicate tab as administrator"),
    (Action::ToggleLastTab, "toggle-last-tab", "Switch to the last used tab"),
    (Action::ReopenTab, "reopen-tab", "Reopen closed tab"),
    (Action::PaneResizeLeft, "pane-resize-left", "Move the pane divider to the left"),
    (Action::PaneResizeRight, "pane-resize-right", "Move the pane divider to the right"),
    (Action::PaneResizeUp, "pane-resize-up", "Move the pane divider up"),
    (Action::PaneResizeDown, "pane-resize-down", "Move the pane divider down"),
    (Action::ExplodeTab, "explode-tab", "Move each pane of the tab to its own tab"),
    (Action::CombineTabs, "combine-tabs", "Combine all tabs into one"),
    (Action::BroadcastToTab, "pane-focus-all", "Type into all panes of the tab"),
    (Action::BroadcastToAllTabs, "focus-all-tabs", "Type into all tabs"),
    (Action::SetTabColor, "set-tab-color", "Set tab color"),
    (Action::ReportProblem, "report-problem", "Report a problem"),
    (Action::DebugCopyState, "debug-copy-state", "Copy the terminal state for a bug report"),
    (Action::DebugSaveOutput, "debug-save-output", "Save the raw output of the session"),
    (Action::ExportBuffer, "export-buffer", "Save the buffer to a file"),
    (Action::NotifyCommand, "notify-command", "Notify when the command finishes"),
    (Action::OpenLog, "open-log", "Open the log folder"),
    (Action::OpenConfigFile, "open-config-file", "Open the config file in an editor"),
    (Action::ToggleTabBar, "toggle-tab-bar", "Show or hide the tab bar"),
];

impl Action {
    pub fn works_beside_settings(self) -> bool {
        use Action::*;
        matches!(
            self,
            Tab(_) | NextTab | PreviousTab | NextMruTab | PreviousMruTab | MoveTabLeft | MoveTabRight | NewTab | NewWindow | NewAdminTab | CloseTab | PinTab | ReopenTab | ToggleLastTab | ProfileSelector | CommandSelector | ToggleFullscreen | ToggleTabBar | SelectColorScheme
        )
    }

    pub fn all() -> impl Iterator<Item = Action> {
        NAMED.iter().map(|(a, _, _)| *a).chain((1..=10).map(Action::Tab))
    }

    pub fn id(&self) -> String {
        match self {
            Action::Tab(n) => format!("tab-{n}"),
            a => NAMED.iter().find(|(x, _, _)| x == a).map_or("", |(_, id, _)| id).to_owned(),
        }
    }

    fn label_template(&self) -> &'static str {
        match self {
            Action::Tab(_) => "Switch to tab {n}",
            a => NAMED.iter().find(|(x, _, _)| x == a).map_or("", |(_, _, l)| l),
        }
    }

    fn label_number(&self) -> String {
        match self {
            Action::Tab(n) => n.to_string(),
            _ => String::new(),
        }
    }

    pub fn label(&self) -> String {
        crate::i18n::tf(self.label_template(), &[("n", &self.label_number())])
    }

    pub fn source_label(&self) -> String {
        self.label_template().replace("{n}", &self.label_number())
    }

    pub fn from_id(id: &str) -> Option<Action> {
        if let Some(n) = id.strip_prefix("tab-").and_then(|n| n.parse::<u8>().ok()) {
            return (1..=20).contains(&n).then_some(Action::Tab(n));
        }
        NAMED.iter().find(|(_, x, _)| *x == id).map(|(a, _, _)| *a)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub mods: Mods,
}

fn key_name(name: &str) -> Option<Key> {
    let key = match name.to_ascii_lowercase().as_str() {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "home" => Key::Home,
        "end" => Key::End,
        "enter" | "return" => Key::Enter,
        "escape" | "esc" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "insert" | "ins" => Key::Insert,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "plus" => Key::Char('='),
        "minus" => Key::Char('-'),
        f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => Key::F(f[1..].parse().ok()?),
        _ => {
            let mut chars = name.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            Key::Char(c.to_ascii_uppercase())
        }
    };
    Some(key)
}

impl Chord {
    pub fn parse(text: &str) -> Option<Chord> {
        let text = text.trim();
        let (mods_part, key_part) = match text.strip_suffix("--") {
            Some(rest) => (rest, "-"),
            None if text == "-" => ("", "-"),
            None => match text.rsplit_once('-') {
                Some((m, k)) => (m, k),
                None => ("", text),
            },
        };
        let mut mods = Mods::NONE;
        for m in mods_part.split('-').filter(|m| !m.is_empty()) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods.ctrl = true,
                "shift" => mods.shift = true,
                "alt" | "option" | "⌥" => mods.alt = true,
                "meta" | "cmd" | "win" | "super" | "⌘" => mods.meta = true,
                _ => return None,
            }
        }
        Some(Chord { key: key_name(key_part)?, mods })
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.mods;
        for (on, name) in [(m.ctrl, "Ctrl"), (m.meta, "Meta"), (m.alt, "Alt"), (m.shift, "Shift")] {
            if on {
                write!(f, "{name}-")?;
            }
        }
        match self.key {
            Key::Up => write!(f, "Up"),
            Key::Down => write!(f, "Down"),
            Key::Left => write!(f, "Left"),
            Key::Right => write!(f, "Right"),
            Key::PageUp => write!(f, "PageUp"),
            Key::PageDown => write!(f, "PageDown"),
            Key::Home => write!(f, "Home"),
            Key::End => write!(f, "End"),
            Key::Enter => write!(f, "Enter"),
            Key::Escape => write!(f, "Escape"),
            Key::Backspace => write!(f, "Backspace"),
            Key::Delete => write!(f, "Delete"),
            Key::Insert => write!(f, "Insert"),
            Key::Tab => write!(f, "Tab"),
            Key::Space => write!(f, "Space"),
            Key::F(n) => write!(f, "F{n}"),
            Key::Char(c) => write!(f, "{c}"),
            other => write!(f, "{other:?}"),
        }
    }
}

pub type Binding = Vec<Chord>;

pub fn parse_binding(text: &str) -> Option<Binding> {
    let chords: Option<Vec<Chord>> = text.split_whitespace().map(Chord::parse).collect();
    chords.filter(|c| !c.is_empty())
}

pub fn format_binding(b: &Binding) -> String {
    b.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" ")
}

pub fn defaults() -> Vec<(Action, &'static [&'static str])> {
    use Action::*;
    vec![
        (CommandSelector, &["Ctrl-Shift-P"]),
        (ProfileSelector, &["Ctrl-Shift-E"]),
        (ToggleFullscreen, &["F11", "Alt-Enter"]),
        (NewTab, &["Ctrl-Shift-T"]),
        (NewWindow, &["Ctrl-Shift-N"]),
        (CloseTab, &["Ctrl-Shift-W"]),
        (NextTab, &["Ctrl-Shift-Right", "Ctrl-Tab"]),
        (PreviousTab, &["Ctrl-Shift-Left", "Ctrl-Shift-Tab"]),
        (MoveTabLeft, &["Ctrl-Shift-PageUp"]),
        (MoveTabRight, &["Ctrl-Shift-PageDown"]),
        (Tab(1), &["Alt-1"]),
        (Tab(2), &["Alt-2"]),
        (Tab(3), &["Alt-3"]),
        (Tab(4), &["Alt-4"]),
        (Tab(5), &["Alt-5"]),
        (Tab(6), &["Alt-6"]),
        (Tab(7), &["Alt-7"]),
        (Tab(8), &["Alt-8"]),
        (Tab(9), &["Alt-9"]),
        (Tab(10), &["Alt-0"]),
        (SplitRight, &["Ctrl-Shift-S"]),
        (SplitBottom, &["Ctrl-Shift-D"]),
        (PaneNavUp, &["Ctrl-Alt-Up"]),
        (PaneNavDown, &["Ctrl-Alt-Down"]),
        (PaneNavLeft, &["Ctrl-Alt-Left"]),
        (PaneNavRight, &["Ctrl-Alt-Right"]),
        (PaneNavPrevious, &["Ctrl-Alt-["]),
        (PaneNavNext, &["Ctrl-Alt-]"]),
        (PaneMaximize, &["Ctrl-Alt-Enter"]),
        (PaneResizeLeft, &["Alt-Shift-Left"]),
        (PaneResizeRight, &["Alt-Shift-Right"]),
        (PaneResizeUp, &["Alt-Shift-Up"]),
        (PaneResizeDown, &["Alt-Shift-Down"]),
        (CtrlC, &["Ctrl-C"]),
        (Copy, &["Ctrl-Shift-C"]),
        (Paste, &["Ctrl-Shift-V", "Shift-Insert"]),
        (SelectAll, &["Ctrl-Shift-A"]),
        (ZoomIn, &["Ctrl-=", "Ctrl-Shift-="]),
        (ZoomOut, &["Ctrl--", "Ctrl-Shift--"]),
        (ResetZoom, &["Ctrl-0"]),
        (DeletePreviousWord, &["Ctrl-Backspace"]),
        (DeleteLine, &["Ctrl-Shift-Backspace"]),
        (DeleteNextWord, &["Ctrl-Delete"]),
        (InsertNewLine, &["Alt-Shift-Enter"]),
        (ScrollToTop, &["Ctrl-PageUp"]),
        (ScrollPageUp, &["Alt-PageUp"]),
        (ScrollUp, &["Ctrl-Shift-Up"]),
        (ScrollDown, &["Ctrl-Shift-Down"]),
        (ScrollPageDown, &["Alt-PageDown"]),
        (ScrollToBottom, &["Ctrl-PageDown"]),
        (Search, &["Ctrl-Shift-F"]),
        (Settings, &["Ctrl-,"]),
        (ToggleTabBar, &["Ctrl-Shift-B"]),
        (OpenFiles, &["Ctrl-Shift-O"]),
        (FillPassword, &["Ctrl-Shift-Enter"]),
    ]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Action(Action),
    Pending,
    None,
}

#[derive(Clone, Debug, Default)]
pub struct Keymap {
    bindings: Vec<(Binding, Action)>,
    pending: Vec<Chord>,
}

impl Keymap {
    pub fn new(overrides: &BTreeMap<String, Vec<String>>) -> Self {
        Self::with_mru(overrides, false)
    }

    pub fn with_mru(overrides: &BTreeMap<String, Vec<String>>, mru_tabs: bool) -> Self {
        let mut map: BTreeMap<Action, Vec<Binding>> = BTreeMap::new();
        for (action, keys) in defaults() {
            map.insert(action, keys.iter().filter_map(|k| parse_binding(k)).collect());
        }
        for (linear, recent, shortcut) in [(Action::NextTab, Action::NextMruTab, "Ctrl-Tab"), (Action::PreviousTab, Action::PreviousMruTab, "Ctrl-Shift-Tab")] {
            let binding = parse_binding(shortcut).unwrap();
            let assigned = overrides.iter().any(|(id, keys)| Action::from_id(id).is_some() && keys.iter().filter_map(|key| parse_binding(key)).any(|key| key == binding));
            if mru_tabs || assigned {
                map.entry(linear).or_default().retain(|b| *b != binding);
            }
            if mru_tabs && !assigned {
                map.entry(recent).or_default().push(binding);
            }
        }
        for (id, keys) in overrides {
            if let Some(action) = Action::from_id(id) {
                map.insert(action, keys.iter().filter_map(|k| parse_binding(k)).collect());
            }
        }
        let bindings = map.into_iter().flat_map(|(a, bs)| bs.into_iter().map(move |b| (b, a))).collect();
        Self { bindings, pending: Vec::new() }
    }

    pub fn bindings_of(&self, action: Action) -> impl Iterator<Item = &Binding> {
        self.bindings.iter().filter(move |(_, a)| *a == action).map(|(b, _)| b)
    }

    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }

    pub fn press(&mut self, chord: Chord) -> Outcome {
        self.pending.push(chord);
        if let Some((_, a)) = self.bindings.iter().find(|(b, _)| *b == self.pending) {
            let a = *a;
            self.pending.clear();
            return Outcome::Action(a);
        }
        if self.bindings.iter().any(|(b, _)| b.len() > self.pending.len() && b.starts_with(&self.pending)) {
            return Outcome::Pending;
        }
        let retry = self.pending.len() > 1;
        self.pending.clear();
        if retry {
            return self.press(chord);
        }
        Outcome::None
    }
}

#[cfg(test)]
#[path = "../test/keymap.rs"]
mod tests;
