use crate::input::{Key, Mods, MouseButton, WheelDirection};
use crate::picker::ProfileSection;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    CommandSelector,
    ProfileSelector,
    ToggleFullscreen,
    NewTab,
    NewWindow,
    CloseTab,
    DuplicateTab,
    SaveAsProfile,
    SaveLayout,
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
    RenamePane,
    ChangePaneProfile,
    ToggleProfileSidebar,
    RearrangePanes,
    PaneToTab,
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
    SearchSessions,
    QuickSelect,
    QuickOpen,
    PreviousPrompt,
    NextPrompt,
    CopyCommandOutput,
    CommandHistory,
    NewWorkspace,
    RenameWorkspace,
    SwitchWorkspace,
    ManageSnippets,
    SaveSelectionAsSnippet,
    RerunTask,
    Settings,
    RenameTab,
    PinTab,
    CloseOtherTabs,
    CloseTabsToRight,
    CloseTabsToLeft,
    OpenFiles,
    SftpTransfers,
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
    OpenRepository,
    HelpTranslate,
    DebugCopyState,
    DebugSaveOutput,
    DebugRecordFixture,
    DebugSaveFixture,
    ExportBuffer,
    NotifyCommand,
    OpenLog,
    StartMenuShortcut,
    InstallForUser,
    DefaultTerminal(bool),
    EditEnvironment,
    OpenConfigFile,
    OpenConfigFolder,
    ShowDefaultConfig,
    ApplicationsMenu(bool),
    WhatsNew,
    CheckForUpdates,
    ToggleTabBar,
    ManageProfiles,
    ClearRecentProfiles,
    MinimizeWindow,
    ToggleMaximize,
    CloseWindow,
    Quit,
    OpenProfile(String),
    SelectProfileSection(ProfileSection),
    SelectProfileGroup(String),
    Extension { extension: String, command: String },
}

const NAMED: &[(Action, &str, &str)] = &[
    (Action::CommandSelector, "command-selector", "Show command selector"),
    (Action::ProfileSelector, "profile-selector", "Profiles and connections"),
    (Action::ToggleFullscreen, "toggle-fullscreen", "Toggle fullscreen"),
    (Action::NewTab, "new-tab", "New tab"),
    (Action::NewWindow, "new-window", "New window"),
    (Action::CloseTab, "close-tab", "Close tab"),
    (Action::DuplicateTab, "duplicate-tab", "Duplicate tab"),
    (Action::SaveAsProfile, "save-as-profile", "Save as a profile"),
    (Action::SaveLayout, "split-layout", "Save the layout as a profile"),
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
    (Action::RenamePane, "rename-pane", "Rename pane"),
    (Action::ChangePaneProfile, "change-pane-profile", "Change pane profile"),
    (Action::ToggleProfileSidebar, "toggle-profile-sidebar", "Toggle profile sidebar"),
    (Action::RearrangePanes, "rearrange-panes", "Rearrange panes"),
    (Action::PaneToTab, "pane-to-tab", "Move pane to a new tab"),
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
    (Action::SearchSessions, "search-sessions", "Search open sessions"),
    (Action::QuickSelect, "quick-select", "Quick select from output"),
    (Action::QuickOpen, "quick-open", "Quick open from output"),
    (Action::PreviousPrompt, "previous-prompt", "Go to previous shell prompt"),
    (Action::NextPrompt, "next-prompt", "Go to next shell prompt"),
    (Action::CopyCommandOutput, "copy-command-output", "Copy last command output"),
    (Action::CommandHistory, "command-history", "Command history"),
    (Action::NewWorkspace, "new-workspace", "New workspace"),
    (Action::RenameWorkspace, "rename-workspace", "Rename workspace"),
    (Action::SwitchWorkspace, "switch-workspace", "Switch workspace"),
    (Action::ManageSnippets, "manage-snippets", "Manage snippets"),
    (Action::SaveSelectionAsSnippet, "save-selection-as-snippet", "Save selection as snippet"),
    (Action::RerunTask, "rerun-task", "Rerun task"),
    (Action::Settings, "settings", "Open settings"),
    (Action::RenameTab, "rename-tab", "Rename tab"),
    (Action::PinTab, "pin-tab", "Pin or unpin the tab"),
    (Action::CloseOtherTabs, "close-other-tabs", "Close other tabs"),
    (Action::CloseTabsToRight, "close-tabs-to-the-right", "Close tabs to the right"),
    (Action::CloseTabsToLeft, "close-tabs-to-the-left", "Close tabs to the left"),
    (Action::OpenFiles, "open-files", "Remote files (SFTP)"),
    (Action::SftpTransfers, "sftp-transfers", "SFTP transfers"),
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
    (Action::OpenRepository, "open-repository", "GitHub"),
    (Action::HelpTranslate, "help-translate", "Help translate Aster"),
    (Action::DebugCopyState, "debug-copy-state", "Copy the terminal state for a bug report"),
    (Action::DebugSaveOutput, "debug-save-output", "Save the raw output of the session"),
    (Action::DebugRecordFixture, "debug-record-fixture", "Open a tab to record a terminal fixture"),
    (Action::DebugSaveFixture, "debug-save-fixture", "Save the terminal replay fixture"),
    (Action::ExportBuffer, "export-buffer", "Save the buffer to a file"),
    (Action::NotifyCommand, "notify-command", "Notify when the command finishes"),
    (Action::OpenLog, "open-log", "Open the log folder"),
    (Action::EditEnvironment, "edit-environment", "Edit environment variables"),
    #[cfg(windows)]
    (Action::StartMenuShortcut, "start-menu-shortcut", "Toggle Start menu shortcut"),
    #[cfg(windows)]
    (Action::InstallForUser, "install-for-user", "Install for this user"),
    #[cfg(windows)]
    (Action::DefaultTerminal(true), "set-default-terminal", "Make Aster the default terminal"),
    #[cfg(windows)]
    (Action::DefaultTerminal(false), "restore-default-terminal", "Restore the previous default terminal"),
    (Action::OpenConfigFile, "open-config-file", "Open the config file in an editor"),
    (Action::OpenConfigFolder, "open-config-folder", "Open the config folder"),
    (Action::ShowDefaultConfig, "show-default-config", "Show the defaults"),
    (Action::WhatsNew, "whats-new", "What's new"),
    (Action::CheckForUpdates, "check-for-updates", "Check for updates"),
    (Action::ToggleTabBar, "toggle-tab-bar", "Show or hide the tab bar"),
    (Action::ManageProfiles, "manage-profiles", "Manage profiles"),
    (Action::ClearRecentProfiles, "clear-recent-profiles", "Clear recent profiles"),
    (Action::MinimizeWindow, "minimize-window", "Minimize window"),
    (Action::ToggleMaximize, "toggle-maximize", "Maximize or restore window"),
    (Action::CloseWindow, "close-window", "Close window"),
    (Action::Quit, "quit", "Quit"),
    (Action::ApplicationsMenu(true), "add-to-applications-menu", "Add to the applications menu"),
    (Action::ApplicationsMenu(false), "remove-from-applications-menu", "Remove from the applications menu"),
];

impl Action {
    pub fn works_beside_settings(&self) -> bool {
        use Action::*;
        matches!(
            self,
            Tab(_) | NextTab | PreviousTab | NextMruTab | PreviousMruTab | MoveTabLeft | MoveTabRight | NewTab | NewWindow | NewAdminTab | CloseTab | PinTab | ReopenTab | ToggleLastTab | ProfileSelector | CommandSelector | ToggleFullscreen | ToggleTabBar | ToggleProfileSidebar | SelectColorScheme | OpenProfile(_) | SelectProfileSection(_) | SelectProfileGroup(_) | ManageProfiles | ClearRecentProfiles | MinimizeWindow | ToggleMaximize | CloseWindow | Quit
        )
    }

    pub fn all() -> impl Iterator<Item = Action> {
        NAMED.iter().map(|(a, _, _)| a).filter(|a| a.available()).cloned().chain((1..=20).map(Action::Tab))
    }

    fn available(&self) -> bool {
        cfg!(windows) || *self != Self::Quit
    }

    pub fn id(&self) -> String {
        match self {
            Action::Tab(n) => format!("tab-{n}"),
            Action::OpenProfile(id) => format!("profile.{id}"),
            Action::SelectProfileSection(section) => format!("profile-selector.{}", section.id()),
            Action::SelectProfileGroup(id) => format!("group-selector.{id}"),
            Action::Extension { extension, command } => format!("extension.{extension}:{command}"),
            a => NAMED.iter().find(|(x, _, _)| x == a).map_or("", |(_, id, _)| id).to_owned(),
        }
    }

    fn label_template(&self) -> &'static str {
        match self {
            Action::Tab(_) => "Switch to tab {n}",
            Action::OpenProfile(_) => "Open profile {n}",
            Action::SelectProfileSection(_) | Action::SelectProfileGroup(_) => "Show profiles in {n}",
            a => NAMED.iter().find(|(x, _, _)| x == a).map_or("", |(_, _, l)| l),
        }
    }

    fn label_argument(&self, name: Option<&str>, translated: bool) -> String {
        match self {
            Action::Tab(n) => n.to_string(),
            Action::OpenProfile(id) | Action::SelectProfileGroup(id) => name.unwrap_or(id).to_owned(),
            Action::SelectProfileSection(section) => {
                let label = section.source_label();
                if translated { crate::i18n::t(label) } else { label }.to_owned()
            }
            _ => String::new(),
        }
    }

    pub fn label(&self) -> String {
        self.label_for(None)
    }

    pub fn source_label(&self) -> String {
        self.source_label_for(None)
    }

    pub(crate) fn label_for(&self, name: Option<&str>) -> String {
        if let Self::Extension { command, .. } = self { return name.unwrap_or(command).to_owned(); }
        crate::i18n::tf(self.label_template(), &[("n", &self.label_argument(name, true))])
    }

    pub(crate) fn source_label_for(&self, name: Option<&str>) -> String {
        if let Self::Extension { command, .. } = self { return name.unwrap_or(command).to_owned(); }
        self.label_template().replace("{n}", &self.label_argument(name, false))
    }

    pub fn from_id(id: &str) -> Option<Action> {
        if let Some((kind, target)) = id.split_once('.') {
            if target.trim().is_empty() {
                return None;
            }
            return match kind {
                "profile" => Some(Action::OpenProfile(target.to_owned())),
                "profile-selector" => ProfileSection::from_id(target).map(Action::SelectProfileSection),
                "group-selector" => Some(Action::SelectProfileGroup(target.to_owned())),
                "extension" => target.split_once(':').filter(|(extension, command)| aster_ext::valid_id(extension) && !command.is_empty())
                    .map(|(extension, command)| Action::Extension { extension: extension.to_owned(), command: command.to_owned() }),
                _ => None,
            };
        }
        if let Some(n) = id.strip_prefix("tab-").and_then(|n| n.parse::<u8>().ok()) {
            return (1..=20).contains(&n).then_some(Action::Tab(n));
        }
        NAMED.iter().find(|(a, x, _)| *x == id && a.available()).map(|(a, _, _)| a.clone())
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
        "mouseleft" => Key::Mouse(MouseButton::Left),
        "mouseright" => Key::Mouse(MouseButton::Right),
        "mousemiddle" => Key::Mouse(MouseButton::Middle),
        "mouseback" => Key::Mouse(MouseButton::Back),
        "mouseforward" => Key::Mouse(MouseButton::Forward),
        "wheelup" => Key::Wheel(WheelDirection::Up),
        "wheeldown" => Key::Wheel(WheelDirection::Down),
        "wheelleft" => Key::Wheel(WheelDirection::Left),
        "wheelright" => Key::Wheel(WheelDirection::Right),
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

fn modifiers(text: &str) -> Option<Mods> {
    let mut mods = Mods::NONE;
    if !text.is_empty() {
        for name in text.split(['-', '+']) {
            match name.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods.ctrl = true,
                "shift" => mods.shift = true,
                "alt" | "option" | "⌥" => mods.alt = true,
                "meta" | "cmd" | "win" | "super" | "⌘" => mods.meta = true,
                _ => return None,
            }
        }
    }
    Some(mods)
}

impl Chord {
    pub fn parse(text: &str) -> Option<Chord> {
        let text = text.trim();
        if let Some(mods) = modifiers(text).filter(Mods::any) {
            return Some(Chord { key: Key::ModifiersOnly, mods });
        }
        let (prefix, key) = if matches!(text, "-" | "+") {
            ("", text)
        } else if let Some(prefix) = text.strip_suffix("--").or_else(|| text.strip_suffix("+-")) {
            (prefix, "-")
        } else if let Some(prefix) = text.strip_suffix("++").or_else(|| text.strip_suffix("-+")) {
            (prefix, "+")
        } else {
            text.rsplit_once(['-', '+']).unwrap_or(("", text))
        };
        Some(Chord { key: key_name(key)?, mods: modifiers(prefix)? })
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.mods;
        let mut separator = "";
        for (on, name) in [(m.ctrl, "Ctrl"), (m.meta, "Meta"), (m.alt, "Alt"), (m.shift, "Shift")] {
            if on {
                write!(f, "{separator}{name}")?;
                separator = "-";
            }
        }
        if self.key == Key::ModifiersOnly {
            return Ok(());
        }
        write!(f, "{separator}")?;
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
            Key::Mouse(button) => write!(f, "Mouse{button:?}"),
            Key::Wheel(direction) => write!(f, "Wheel{direction:?}"),
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
    defaults_for(cfg!(windows))
}

fn defaults_for(windows: bool) -> Vec<(Action, &'static [&'static str])> {
    use Action::*;
    let common: &[(Action, &[&str])] = &[
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
        (PaneNavPrevious, &["Ctrl-Alt-["]),
        (PaneNavNext, &["Ctrl-Alt-]"]),
        (PaneMaximize, &["Ctrl-Alt-Enter"]),
        (PaneResizeLeft, &["Alt-Shift-Left"]),
        (PaneResizeRight, &["Alt-Shift-Right"]),
        (PaneResizeUp, &["Alt-Shift-Up"]),
        (PaneResizeDown, &["Alt-Shift-Down"]),
        (CtrlC, &["Ctrl-C"]),
        (Copy, &["Ctrl-Shift-C"]),
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
        (SearchSessions, &["Ctrl-Alt-F"]),
        (QuickSelect, &["Ctrl-Shift-G"]),
        (QuickOpen, &["Ctrl-Shift-H"]),
        (CommandHistory, &["Ctrl-Shift-U"]),
        (SftpTransfers, &["Ctrl-Shift-Y"]),
        (Settings, &["Ctrl-,"]),
        (ToggleTabBar, &["Ctrl-Shift-B"]),
        (OpenFiles, &["Ctrl-Shift-O"]),
        (FillPassword, &["Ctrl-Shift-Enter"]),
    ];
    let platform: &[(Action, &[&str])] = if windows {
        &[
            (PaneNavUp, &["Ctrl-Alt-Up"]),
            (PaneNavDown, &["Ctrl-Alt-Down"]),
            (PaneNavLeft, &["Ctrl-Alt-Left"]),
            (PaneNavRight, &["Ctrl-Alt-Right"]),
            (Paste, &["Ctrl-Shift-V", "Shift-Insert"]),
        ]
    } else {
        &[
            (PaneNavUp, &["Alt-Shift-K"]),
            (PaneNavDown, &["Alt-Shift-J"]),
            (PaneNavLeft, &["Alt-Shift-H"]),
            (PaneNavRight, &["Alt-Shift-L"]),
            (Paste, &["Ctrl-Shift-V"]),
        ]
    };
    common.iter().chain(platform).cloned().collect()
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
        Self::for_platform(overrides, mru_tabs, cfg!(windows))
    }

    pub(crate) fn with_extensions(overrides: &BTreeMap<String, Vec<String>>, mru_tabs: bool, commands: Vec<(Action, Option<&str>)>) -> Self {
        let mut overrides = overrides.clone();
        overrides.retain(|id, _| !id.starts_with("extension.") || commands.iter().any(|(action, _)| action.id() == *id));
        let mut keymap = Self::with_mru(&overrides, mru_tabs);
        for (action, shortcut) in commands {
            if overrides.contains_key(&action.id()) { continue; }
            let Some(binding) = shortcut.and_then(parse_binding) else { continue };
            if !keymap.bindings.iter().any(|(bound, _)| bound.starts_with(&binding) || binding.starts_with(bound)) {
                keymap.bindings.push((binding, action));
            }
        }
        keymap
    }

    fn for_platform(overrides: &BTreeMap<String, Vec<String>>, mru_tabs: bool, windows: bool) -> Self {
        let mut map: BTreeMap<Action, Vec<Binding>> = BTreeMap::new();
        for (action, keys) in defaults_for(windows) {
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
        let mut custom = BTreeMap::new();
        for (id, keys) in overrides {
            if let Some(action) = Action::from_id(id) {
                map.remove(&action);
                custom.insert(action, keys.iter().filter_map(|k| parse_binding(k)).collect::<Vec<_>>());
            }
        }
        let bindings = custom.into_iter().chain(map).flat_map(|(a, bs)| bs.into_iter().map(move |b| (b, a.clone()))).collect();
        Self { bindings, pending: Vec::new() }
    }

    pub fn bindings_of<'a>(&'a self, action: &'a Action) -> impl Iterator<Item = &'a Binding> {
        self.bindings.iter().filter(move |(_, a)| a == action).map(|(b, _)| b)
    }

    pub(crate) fn actions_for_chord(&self, chord: Chord) -> impl Iterator<Item = &Action> {
        self.bindings.iter().filter(move |(binding, _)| binding.as_slice() == [chord]).map(|(_, action)| action)
    }

    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }

    pub(crate) fn accepts(&self, chord: Chord) -> bool {
        self.bindings.iter().any(|(binding, _)| {
            binding.first() == Some(&chord)
                || binding.get(self.pending.len()) == Some(&chord) && binding.starts_with(&self.pending)
        })
    }

    pub fn press(&mut self, chord: Chord) -> Outcome {
        self.pending.push(chord);
        if let Some((_, a)) = self.bindings.iter().find(|(b, _)| *b == self.pending) {
            let a = a.clone();
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
