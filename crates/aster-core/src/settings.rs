use crate::app::{App, Effect};
use crate::config::Config;
use crate::i18n::{t, tf};
use crate::input::{Key, Mods};
use crate::keymap::{format_binding, Action, Chord};
use aster_gfx::render::Rect;
use toml::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    Toggle,
    Choice(&'static [(&'static str, &'static str)]),
    Schemes,
    Profiles,
    Number { min: f64, max: f64, step: f64 },
    Text,
    Run(Action),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setting {
    pub section: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub control: Control,
}

const fn item(section: &'static str, key: &'static str, label: &'static str, hint: &'static str, control: Control) -> Setting {
    Setting { section, key, label, hint, control }
}

const TERMINAL: &str = "Terminal";
const INPUT: &str = "Clipboard and mouse";
const SEARCH: &str = "Search";
const APPEARANCE: &str = "Appearance";
const COLORS: &str = "Color scheme";
const DOCK: &str = "Docked window";
const VAULT: &str = "Vault";
const SSH: &str = "SSH";
const SYSTEM: &str = "Application";

const CURSORS: &[(&str, &str)] = &[("block", "Block"), ("beam", "Beam"), ("underline", "Underline")];
const RIGHT_CLICK: &[(&str, &str)] = &[("off", "Nothing"), ("menu", "Context menu"), ("paste", "Paste"), ("clipboard", "Copy or paste")];
const SCHEME_MODES: &[(&str, &str)] = &[("dark", "Dark"), ("light", "Light"), ("auto", "Follow the system")];
const BELLS: &[(&str, &str)] = &[("off", "Off"), ("visual", "Flash the pane"), ("audible", "System sound")];
const IDENTIFICATION: &[(&str, &str)] = &[("aster", "Aster"), ("xterm", "Compatibility")];
const TAB_BARS: &[(&str, &str)] = &[("always", "Always"), ("auto", "With two or more tabs"), ("never", "Never")];
const TAB_WIDTHS: &[(&str, &str)] = &[("fit", "Fit"), ("fixed", "Fixed")];
const TAB_POSITIONS: &[(&str, &str)] = &[("top", "Top"), ("bottom", "Bottom"), ("left", "Left"), ("right", "Right")];
const BACKDROPS: &[(&str, &str)] = &[("none", "None"), ("acrylic", "Acrylic"), ("mica", "Mica")];
const SSH_CLIENTS: &[(&str, &str)] = &[("native", "Built in"), ("system", "System OpenSSH")];
const LANGUAGES: &[(&str, &str)] = &[("auto", "Automatic"), ("en", "English"), ("ru", "Russian")];
const EDGES: &[(&str, &str)] = &[("off", "Off"), ("top", "Top"), ("bottom", "Bottom"), ("left", "Left"), ("right", "Right")];
const SHARE: Control = Control::Number { min: 0.1, max: 1.0, step: 0.05 };

pub const SETTINGS: &[Setting] = &[
    item(TERMINAL, "terminal.font", "Font", "Monospace font family for the terminal", Control::Text),
    item(TERMINAL, "terminal.font-size", "Font size", "Size in points before zoom", Control::Number { min: 6.0, max: 48.0, step: 1.0 }),
    item(TERMINAL, "terminal.line-padding", "Line padding", "Extra pixels between lines", Control::Number { min: 0.0, max: 20.0, step: 1.0 }),
    item(TERMINAL, "terminal.padding", "Text padding", "Space between the text and the edges of a pane", Control::Number { min: 0.0, max: 64.0, step: 1.0 }),
    item(TERMINAL, "terminal.cursor", "Cursor shape", "Shape of the text cursor", Control::Choice(CURSORS)),
    item(TERMINAL, "terminal.cursor-blink", "Blinking cursor", "The cursor blinks while the window has focus", Control::Toggle),
    item(TERMINAL, "terminal.scrollback-lines", "Scrollback", "Lines kept above the screen", Control::Number { min: 0.0, max: 1_000_000.0, step: 1000.0 }),
    item(TERMINAL, "terminal.draw-bold-text-in-bright-colors", "Bold text in bright colors", "Bold text uses the bright palette colors", Control::Toggle),
    item(TERMINAL, "terminal.profile", "Default profile", "Shell for new tabs and windows", Control::Profiles),
    item(TERMINAL, "terminal.identification", "Terminal identification", "Report the Aster name and version to new shells", Control::Choice(IDENTIFICATION)),
    item(TERMINAL, "terminal.recover-tabs", "Restore tabs", "Reopen tabs, splits and their folders on start", Control::Toggle),
    item(TERMINAL, "terminal.confirm-close", "Confirm closing running programs", "Ask before closing a tab, pane or window where a program is still running", Control::Toggle),
    item(TERMINAL, "terminal.focus-follows-mouse", "Focus follows the mouse", "Moving the pointer over a pane makes it the active one", Control::Toggle),
    item(TERMINAL, "terminal.bell", "Bell", "What happens when a program rings the bell", Control::Choice(BELLS)),
    item(TERMINAL, "terminal.scroll-on-input", "Scroll on input", "Typing scrolls back to the bottom", Control::Toggle),
    item(INPUT, "terminal.copy-on-select", "Copy on select", "Selected text goes to the clipboard", Control::Toggle),
    item(INPUT, "terminal.copy-as-html", "Copy as HTML", "Include text colors and bold weight when copying a selection", Control::Toggle),
    item(INPUT, "terminal.right-click", "Right click", "What the right mouse button does", Control::Choice(RIGHT_CLICK)),
    item(INPUT, "terminal.paste-on-middle-click", "Paste on middle click", "The middle mouse button pastes", Control::Toggle),
    item(INPUT, "terminal.bracketed-paste", "Bracketed paste", "Programs can tell pasted text from typing", Control::Toggle),
    item(INPUT, "terminal.trim-whitespace-on-paste", "Trim pasted text", "Remove trailing whitespace from pasted text", Control::Toggle),
    item(INPUT, "terminal.warn-on-multiline-paste", "Confirm multiline paste", "Ask before pasting several lines into a program that would run each one", Control::Toggle),
    item(INPUT, "terminal.sudo-passwords", "Saved passwords at prompts", "At a sudo or other password prompt, Ctrl+Shift+Enter types a saved password or saves one that worked", Control::Toggle),
    item(INPUT, "terminal.word-separator", "Word separators", "Characters that end a word on double click", Control::Text),
    item(SEARCH, "terminal.search-options.case-sensitive", "Match case", "Search tells upper and lower case apart", Control::Toggle),
    item(SEARCH, "terminal.search-options.regex", "Regular expressions", "Search queries are regular expressions", Control::Toggle),
    item(SEARCH, "terminal.search-options.whole-word", "Whole words", "Search matches whole words only", Control::Toggle),
    item(APPEARANCE, "appearance.opacity", "Opacity", "Background opacity of the terminal", Control::Number { min: 0.2, max: 1.0, step: 0.05 }),
    item(APPEARANCE, "terminal.minimum-contrast-ratio", "Minimum contrast ratio", "Adjust text color for readability; 1 disables the adjustment", Control::Number { min: 1.0, max: 21.0, step: 0.5 }),
    item(APPEARANCE, "appearance.backdrop", "Backdrop", "System material behind the window on Windows 11", Control::Choice(BACKDROPS)),
    item(APPEARANCE, "appearance.ui-font", "Interface font", "Font of tabs and menus; empty uses the system font", Control::Text),
    item(APPEARANCE, "appearance.density", "Interface density", "Scale tabs, menus, settings and padding", Control::Number { min: 0.6, max: 1.4, step: 0.1 }),
    item(APPEARANCE, "terminal.fallback-font", "Fallback font", "Font to try for missing terminal characters before system fallback", Control::Text),
    item(APPEARANCE, "terminal.ligatures", "Font ligatures", "Allow the terminal font to join adjacent characters", Control::Toggle),
    item(APPEARANCE, "appearance.tab-bar", "Tab bar", "Hide the tab bar and window buttons to leave only the terminal; Ctrl+Shift+B toggles it", Control::Choice(TAB_BARS)),
    item(APPEARANCE, "appearance.tab-index", "Show tab numbers", "Display the tab number before its title", Control::Toggle),
    item(APPEARANCE, "appearance.tab-close-buttons", "Show close buttons on tabs", "Show a close button on unpinned tabs", Control::Toggle),
    item(APPEARANCE, "appearance.tabs-in-fullscreen", "Keep the tab bar in full screen", "Use the usual tab bar visibility in full screen", Control::Toggle),
    item(APPEARANCE, "appearance.tab-width", "Tab width", "Choose the width of horizontal tabs", Control::Choice(TAB_WIDTHS)),
    item(APPEARANCE, "appearance.tab-bar-position", "Tab bar position", "Choose the edge of the window for the tab bar", Control::Choice(TAB_POSITIONS)),
    item(APPEARANCE, "appearance.mru-tabs", "Switch tabs in order of recent use", "Hold Ctrl and press Tab to choose a recently used tab", Control::Toggle),
    item(APPEARANCE, "appearance.cycle-tabs", "Cycle tabs", "Switching past the last tab wraps around", Control::Toggle),
    item(APPEARANCE, "appearance.animations", "Animations", "Show tab name toasts, visual bells and cursor blink", Control::Toggle),
    item(APPEARANCE, "appearance.last-tab-closes-window", "Last tab closes the window", "Closing the only tab closes its window", Control::Toggle),
    item(APPEARANCE, "appearance.tray", "Notification area icon", "Keep Aster running after its last window closes on Windows. Has no effect on Linux.", Control::Toggle),
    item(COLORS, "appearance.color-scheme-mode", "Theme", "Dark, light or the system setting", Control::Choice(SCHEME_MODES)),
    item(COLORS, "appearance.color-scheme", "Dark color scheme", "Colors of the dark theme", Control::Schemes),
    item(COLORS, "appearance.light-color-scheme", "Light color scheme", "Colors of the light theme", Control::Schemes),
    item(DOCK, "appearance.dock", "Dock to edge", "Pin the window to a screen edge", Control::Choice(EDGES)),
    item(DOCK, "appearance.dock-fill", "Size", "Share of the screen across the edge", SHARE),
    item(DOCK, "appearance.dock-space", "Length", "Share of the screen along the edge", SHARE),
    item(DOCK, "appearance.dock-always-on-top", "Always on top", "The docked window stays above other windows", Control::Toggle),
    item(DOCK, "appearance.dock-hide-on-blur", "Hide on focus loss", "The docked window hides when another window is activated", Control::Toggle),
    item(DOCK, "appearance.global-hotkey", "Global hotkey", "Shows and hides the window from anywhere, for example Ctrl-`", Control::Text),
    item(VAULT, "vault.remember-minutes", "Stay unlocked for (minutes)", "Zero keeps the vault unlocked until Aster exits", Control::Number { min: 0.0, max: 1440.0, step: 5.0 }),
    item(SSH, "ssh.client", "SSH client", "Built-in client, or the ssh program installed in the system", Control::Choice(SSH_CLIENTS)),
    item(SSH, "ssh.remember-passwords", "Offer to save passwords", "After a typed password or key passphrase works, offer to save it", Control::Toggle),
    item(SSH, "ssh.forward-agent", "Forward the SSH agent", "Let servers use your local SSH agent; ForwardAgent in ~/.ssh/config decides per host", Control::Toggle),
    item(SSH, "ssh.share-connections", "Share connections", "New tabs to a host that is already open reuse its login", Control::Toggle),
    item(SSH, "ssh.clear-after-connect", "Clear after login", "Clear connection messages before the remote shell starts", Control::Toggle),
    item(SSH, "ssh.x11-display", "Local X11 display", "For X11 forwarding. Leave empty to use DISPLAY", Control::Text),
    item(SYSTEM, "appearance.language", "Language", "Language of menus, settings and hints; automatic follows the system", Control::Choice(LANGUAGES)),
    item(SYSTEM, "system.explorer-menu", "Explorer menu", "Add \"Open in Aster\" to the folder menu of Windows Explorer", Control::Toggle),
    item(SYSTEM, "action.open-log", "Log", "Open the folder with the log file", Control::Run(Action::OpenLog)),
    item(SYSTEM, "action.open-config-file", "Config file", "Open the config file in the system editor", Control::Run(Action::OpenConfigFile)),
    item(SYSTEM, "action.report-problem", "Report a problem", "Open a prefilled issue with the version and system", Control::Run(Action::ReportProblem)),
];

pub fn get(config: &Config, key: &str) -> Option<Value> {
    let root = Value::try_from(config).ok()?;
    let mut at = &root;
    for part in key.split('.') {
        at = at.get(part)?;
    }
    Some(at.clone())
}

pub fn set(config: &Config, key: &str, value: Value) -> Result<Config, String> {
    let mut root = Value::try_from(config).map_err(|e| e.to_string())?;
    let mut at = &mut root;
    for part in key.split('.') {
        at = at.as_table_mut().ok_or_else(|| format!("{key} is not a table path"))?.entry(part).or_insert_with(|| Value::Table(Default::default()));
    }
    *at = value;
    root.try_into().map_err(|e: toml::de::Error| e.message().to_owned())
}

pub fn number_text(v: f64) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded}")
    }
}

fn found(hay: &str, query: &str) -> bool {
    let hay = hay.to_lowercase();
    query.to_lowercase().split_whitespace().all(|w| hay.contains(w))
}

pub const SHORTCUTS: &str = "Keyboard shortcuts";
pub const PROFILES: &str = "Profiles";
pub const PAGES: [&str; 11] = [SYSTEM, APPEARANCE, COLORS, PROFILES, TERMINAL, INPUT, SEARCH, DOCK, SHORTCUTS, VAULT, SSH];
pub const FIELDS: [&str; 9] = ["Name", "Command", "Arguments", "Start folder", "SSH destination", "Serial port", "Telnet or TCP address", "Group", "Color scheme"];

const TEMPLATES: [(&str, &str, usize, &str); 5] = [
    ("local", "Local shell", 1, ""),
    ("ssh", "SSH connection", 4, ""),
    ("serial", "Serial port", 5, ""),
    ("telnet", "Telnet", 6, "telnet://"),
    ("tcp", "TCP", 6, "tcp://"),
];

pub(crate) fn profile_templates() -> Vec<crate::overlay::Pick> {
    TEMPLATES.iter().map(|(id, name, _, _)| crate::overlay::Pick { value: (*id).to_owned(), label: t(name).to_owned(), ..Default::default() }).collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileRow {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub custom: bool,
    pub default: bool,
    pub hidden: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemeRow {
    pub name: String,
    pub custom: bool,
    pub foreground: u32,
    pub background: u32,
    pub colors: [u32; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultIssue {
    Locked, WrongPassphrase, Corrupted, Unsupported, SaveFailed, EmptyPassphrase, AlreadyConfigured, NotConfigured, Unavailable,
}

impl VaultIssue {
    pub fn message(self) -> &'static str {
        match self {
            Self::Locked => t("The vault is locked"),
            Self::WrongPassphrase => t("Wrong master passphrase"),
            Self::Corrupted => t("The vault is corrupted"),
            Self::Unsupported => t("Unsupported vault format"),
            Self::SaveFailed => t("Could not save the vault"),
            Self::EmptyPassphrase => t("Enter a master passphrase"),
            Self::AlreadyConfigured => t("The vault is already configured"),
            Self::NotConfigured => t("The vault is not configured"),
            Self::Unavailable => t("The vault is unavailable"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VaultView {
    pub configured: bool,
    pub unlocked: bool,
    pub entries: Vec<(String, String)>,
    pub error: Option<VaultIssue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VaultRow {
    Create, Unlock, Entry(usize), Lock, Change, Erase,
}

impl VaultRow {
    fn edit(self) -> Option<InlineEdit> {
        let step = match self {
            Self::Create => PassphraseStep::Create,
            Self::Unlock => PassphraseStep::Unlock,
            Self::Change => PassphraseStep::Current,
            _ => return None,
        };
        Some(InlineEdit::Passphrase { step, value: String::new() })
    }

    fn text(self) -> (&'static str, &'static str) {
        match self {
            Self::Create => ("Set a master passphrase", "Encrypt new passwords in the config instead of using the system credential store"),
            Self::Unlock => ("Unlock the vault", "The vault is locked"),
            Self::Lock => ("Lock now", "Require the master passphrase to access saved passwords"),
            Self::Change => ("Change the master passphrase", "Enter the current passphrase, then the new one"),
            Self::Erase => ("Erase the vault", "Delete all vault entries and use the system credential store again"),
            Self::Entry(_) => ("", ""),
        }
    }

    pub(crate) fn labels(self, view: &VaultView) -> (&str, &str) {
        if let Self::Entry(i) = self {
            let (kind, key) = &view.entries[i];
            return (key, if kind == "password" { t("password") } else { kind });
        }
        let (label, hint) = self.text();
        (t(label), t(hint))
    }
}

impl VaultView {
    fn row(&self, index: usize) -> Option<VaultRow> {
        if !self.configured {
            return (index == 0).then_some(VaultRow::Create);
        }
        if !self.unlocked {
            return (index == 0).then_some(VaultRow::Unlock);
        }
        match index.checked_sub(self.entries.len()) {
            None => Some(VaultRow::Entry(index)),
            Some(k) => [VaultRow::Lock, VaultRow::Change, VaultRow::Erase].get(k).copied(),
        }
    }
}

#[derive(Clone, PartialEq)]
enum PassphraseStep {
    Create, Unlock, Current, New { old: String },
}

#[derive(Clone, PartialEq)]
enum InlineEdit {
    Setting(String),
    Passphrase { step: PassphraseStep, value: String },
    Waiting,
    Interrupted,
}

impl InlineEdit {
    fn buffer(&mut self) -> Option<&mut String> {
        match self {
            Self::Setting(value) | Self::Passphrase { value, .. } => Some(value),
            Self::Waiting | Self::Interrupted => None,
        }
    }

    fn visible(&self) -> String {
        match self {
            Self::Setting(value) => value.clone(),
            Self::Passphrase { value, .. } => "•".repeat(value.chars().count()),
            Self::Waiting | Self::Interrupted => String::new(),
        }
    }
}

impl std::fmt::Debug for InlineEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.visible())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileEditor {
    pub original: Option<String>,
    pub source: Option<String>,
    pub fields: [String; 9],
    pub focus: usize,
    pub error: Option<String>,
}

pub fn split_args(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                    any = false;
                }
            }
            (None, c) => current.push(c),
        }
    }
    if any || !current.is_empty() {
        out.push(current);
    }
    out
}

pub fn join_args(args: &[String]) -> String {
    args.iter().map(|a| if a.is_empty() || a.contains(char::is_whitespace) { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ")
}

fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s = s.trim_matches('-').to_owned();
    if s.is_empty() {
        "profile".into()
    } else {
        s
    }
}

pub fn shortcut(item: usize) -> Option<Action> {
    item.checked_sub(SETTINGS.len()).and_then(|k| Action::all().nth(k))
}

pub fn section_of(item: usize) -> &'static str {
    match shortcut(item) {
        Some(_) => SHORTCUTS,
        None => SETTINGS[item].section,
    }
}

fn item_matches(item: usize, query: &str) -> bool {
    match shortcut(item) {
        Some(a) => found(&format!("{SHORTCUTS} {} hotkey {} {} {}", t(SHORTCUTS), a.source_label(), a.label(), a.id()), query),
        None => {
            let s = &SETTINGS[item];
            found(&format!("{} {} {} {} {} {} {}", s.section, t(s.section), s.label, t(s.label), s.hint, t(s.hint), s.key), query)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsPage {
    pub query: String,
    pub shown: Vec<usize>,
    pub selected: usize,
    pub scroll: f32,
    editing: Option<InlineEdit>,
    pub capture: Option<Action>,
    pub notice: Option<String>,
    pub profiles: Vec<ProfileRow>,
    pub(crate) vault: VaultView,
    confirming_vault: Option<VaultRow>,
    pub schemes: Vec<SchemeRow>,
    pub editor: Option<ProfileEditor>,
    pub deleting: Option<String>,
    pub page: usize,
}

impl SettingsPage {
    pub fn new() -> Self {
        let mut page = Self::default();
        page.refilter();
        page
    }

    pub(crate) fn refilter(&mut self) {
        let base = self.profile_base();
        let mut shown: Vec<_> = (0..SETTINGS.len()).filter(|&i| item_matches(i, &self.query)).collect();
        let vault_rows = (0..).map_while(|k| self.vault.row(k).map(|row| (k, row))).filter_map(|(k, row)| {
            let (source, hint) = row.text();
            let (label, detail) = row.labels(&self.vault);
            found(&format!("{VAULT} {} {source} {hint} {label} {detail}", t(VAULT)), &self.query).then_some(self.vault_base() + k)
        });
        let before_vault = shown.iter().position(|&i| SETTINGS[i].section == VAULT).unwrap_or(shown.len());
        shown.splice(before_vault..before_vault, vault_rows);
        let profiles = (0..self.profiles.len()).filter(|&k| {
            let p = &self.profiles[k];
            found(&format!("{PROFILES} {} profile shell {} {} {}", t(PROFILES), p.name, p.detail, p.id), &self.query)
        });
        let new_row = found(&format!("{PROFILES} {} new profile add create {}", t(PROFILES), t("New profile")), &self.query).then_some(base + self.profiles.len());
        let shortcuts = (SETTINGS.len()..base).filter(|&i| item_matches(i, &self.query));
        let scheme_base = self.scheme_base();
        let schemes = self.schemes.iter().enumerate().filter(|(_, row)| {
            self.query.is_empty() || found(&row.name, &self.query)
        }).map(|(k, _)| scheme_base + k);
        let after_colors = shown.iter().rposition(|&i| SETTINGS.get(i).is_some_and(|s| s.section == COLORS)).map_or(shown.len(), |n| n + 1);
        shown.splice(after_colors..after_colors, schemes);
        shown.extend(profiles.map(|k| base + k).chain(new_row).chain(shortcuts));
        self.shown = if self.query.trim().is_empty() {
            let page = PAGES[self.page.min(PAGES.len() - 1)];
            shown.into_iter().filter(|&i| self.section(i) == page).collect()
        } else {
            shown
        };
        self.confirming_vault = None;
        self.selected = 0;
        self.scroll = 0.0;
    }

    pub fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    pub(crate) fn show_page(&mut self, page: usize) {
        self.page = page.min(PAGES.len() - 1);
        self.query.clear();
        self.editing = None;
        self.notice = None;
        self.refilter();
    }

    pub fn current(&self) -> Option<&'static Setting> {
        self.shown.get(self.selected).and_then(|&i| SETTINGS.get(i))
    }

    pub fn current_shortcut(&self) -> Option<Action> {
        self.shown.get(self.selected).and_then(|&i| shortcut(i))
    }

    fn profile_base(&self) -> usize {
        SETTINGS.len() + Action::all().count()
    }

    pub fn profile_at(&self, item: usize) -> Option<&ProfileRow> {
        item.checked_sub(self.profile_base()).and_then(|k| self.profiles.get(k))
    }

    pub fn is_new_profile(&self, item: usize) -> bool {
        item == self.profile_base() + self.profiles.len()
    }

    fn scheme_base(&self) -> usize {
        self.profile_base() + self.profiles.len() + 1
    }

    pub fn scheme_at(&self, item: usize) -> Option<&SchemeRow> {
        item.checked_sub(self.scheme_base()).and_then(|k| self.schemes.get(k))
    }

    pub(crate) fn scheme_item(&self, name: &str) -> Option<usize> {
        self.schemes.iter().position(|s| s.name.eq_ignore_ascii_case(name)).map(|k| self.scheme_base() + k)
    }

    fn vault_base(&self) -> usize {
        self.scheme_base() + self.schemes.len()
    }

    pub(crate) fn vault_at(&self, item: usize) -> Option<VaultRow> {
        self.vault.row(item.checked_sub(self.vault_base())?)
    }

    pub(crate) fn editing(&self) -> bool {
        self.editing.is_some()
    }

    pub(crate) fn editing_passphrase(&self) -> bool {
        matches!(self.editing, Some(InlineEdit::Passphrase { .. } | InlineEdit::Waiting | InlineEdit::Interrupted))
    }

    pub(crate) fn edit_text(&self) -> String {
        self.editing.as_ref().map(InlineEdit::visible).unwrap_or_default()
    }

    pub(crate) fn vault_placeholder(&self) -> &'static str {
        match &self.editing {
            Some(InlineEdit::Waiting) => t("Please wait…"),
            Some(InlineEdit::Passphrase { step: PassphraseStep::Current, .. }) => t("Current passphrase"),
            Some(InlineEdit::Passphrase { step: PassphraseStep::New { .. }, .. }) => t("New passphrase"),
            _ => t("Master passphrase"),
        }
    }

    pub(crate) fn cancel_vault_input(&mut self) {
        if self.editing_passphrase() {
            self.editing = None;
        }
        if self.confirming_vault.take().is_some() {
            self.notice = None;
        }
    }

    pub(crate) fn vault_notice(&self) -> Option<&str> {
        if matches!(self.editing, Some(InlineEdit::Interrupted)) {
            return Some(t("Vault state changed. Press Esc to leave this field."));
        }
        self.notice.as_deref().or_else(|| self.vault.error.map(VaultIssue::message))
    }

    pub fn section(&self, item: usize) -> &'static str {
        if self.vault_at(item).is_some() {
            VAULT
        } else if self.scheme_at(item).is_some() {
            COLORS
        } else if self.profile_at(item).is_some() || self.is_new_profile(item) {
            PROFILES
        } else {
            section_of(item)
        }
    }

    pub fn current_item(&self) -> Option<usize> {
        self.shown.get(self.selected).copied()
    }

    fn step(&mut self, delta: isize) {
        if self.shown.is_empty() {
            return;
        }
        let last = self.shown.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowKind {
    Section(&'static str),
    Item(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub kind: RowKind,
    pub rect: Rect,
    pub control: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsGeometry {
    pub page: Rect,
    pub title: Rect,
    pub field: Rect,
    pub close: Rect,
    pub list: Rect,
    pub rows: Vec<Row>,
    pub pages: Vec<Rect>,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Whole,
    Less,
    More,
}

pub fn control_width(control: Control) -> f32 {
    match control {
        Control::Toggle => 40.0,
        Control::Number { .. } => 132.0,
        Control::Choice(_) | Control::Schemes | Control::Profiles => 220.0,
        Control::Text => 260.0,
        Control::Run(_) => 120.0,
    }
}

pub fn geometry(content: Rect, s: f32, page: &SettingsPage) -> SettingsGeometry {
    let total = (1080.0 * s).min(content.w - 32.0 * s).max(240.0 * s).round();
    let x0 = (content.x + (content.w - total) / 2.0).round();
    let side_w = if total >= 640.0 * s { (200.0 * s).round() } else { 0.0 };
    let gap = if side_w > 0.0 { (24.0 * s).round() } else { 0.0 };
    let x = x0 + side_w + gap;
    let width = total - side_w - gap;
    let top = content.y + (18.0 * s).round();
    let header_h = (36.0 * s).round();
    let close_w = header_h;
    let close = Rect::new(x0 + total - close_w, top, close_w, header_h);
    let field_w = (300.0 * s).min(width * 0.5).round();
    let field = Rect::new(close.x - field_w - (8.0 * s).round(), top, field_w, header_h);
    let title = Rect::new(x0, top, (field.x - x0 - 8.0 * s).max(0.0), header_h);
    let list_top = top + header_h + (14.0 * s).round();
    let list = Rect::new(x, list_top, width, (content.bottom() - list_top - 12.0 * s).max(0.0));
    let page_h = (34.0 * s).round();
    let pages = if side_w > 0.0 { (0..PAGES.len()).map(|k| Rect::new(x0, list_top + k as f32 * page_h, side_w, page_h - (2.0 * s).round())).collect() } else { Vec::new() };
    let section_h = (38.0 * s).round();
    let item_h = (58.0 * s).round();
    let control_h = (30.0 * s).round();
    let mut rows = Vec::new();
    let mut y = list.y - page.scroll;
    let mut section = "";
    for (n, &i) in page.shown.iter().enumerate() {
        if page.section(i) != section {
            section = page.section(i);
            rows.push(Row { kind: RowKind::Section(section), rect: Rect::new(x, y, width, section_h), control: Rect::default() });
            y += section_h;
        }
        let rect = Rect::new(x, y, width, item_h);
        let (cw, ch) = match SETTINGS.get(i).map(|s| s.control) {
            Some(Control::Toggle) => ((control_width(Control::Toggle) * s).round(), (22.0 * s).round()),
            Some(c) => ((control_width(c) * s).min(width * 0.45).round(), control_h),
            None if page.scheme_at(i).is_some() => ((300.0 * s).min(width * 0.45).round(), (36.0 * s).round()),
            None => ((240.0 * s).min(width * 0.45).round(), control_h),
        };
        let control = Rect::new(rect.right() - cw - (14.0 * s).round(), rect.y + ((item_h - ch) / 2.0).round(), cw, ch);
        rows.push(Row { kind: RowKind::Item(n), rect, control });
        y += item_h;
    }
    let height = y + page.scroll - list.y;
    SettingsGeometry { page: content, title, field, close, list, rows, pages, height }
}

pub fn part_at(control: Control, r: Rect, s: f32, x: f32) -> Part {
    let edge = (30.0 * s).round();
    match control {
        Control::Number { .. } | Control::Choice(_) | Control::Schemes | Control::Profiles if x < r.x + edge => Part::Less,
        Control::Number { .. } | Control::Choice(_) | Control::Schemes | Control::Profiles if x >= r.right() - edge => Part::More,
        _ => Part::Whole,
    }
}

impl App {
    pub fn vault_state(&mut self, view: VaultView) {
        let waiting = self.settings.as_ref().is_some_and(|page| matches!(page.editing, Some(InlineEdit::Waiting)));
        if self.vault == view && !waiting {
            return;
        }
        self.refresh_prompt_secrets(&view);
        for page in self.settings.iter_mut().chain(&mut self.settings_stash) {
            let current = page.current_item();
            let old_row = current.and_then(|item| page.vault_at(item));
            let old_entry = match old_row {
                Some(VaultRow::Entry(i)) => Some(page.vault.entries[i].clone()),
                _ => None,
            };
            let scroll = page.scroll;
            let state_changed = page.vault.configured != view.configured || page.vault.unlocked != view.unlocked;
            if matches!(page.editing, Some(InlineEdit::Waiting)) {
                if state_changed || view.error.is_some() || old_row == Some(VaultRow::Change) {
                    page.editing = if !state_changed && view.error.is_some() { old_row.and_then(VaultRow::edit) } else { None };
                }
            } else if state_changed && page.editing_passphrase() {
                page.editing = Some(InlineEdit::Interrupted);
            }
            if old_row.is_some() {
                page.notice = None;
            }
            page.vault = view.clone();
            page.refilter();
            let at = page.shown.iter().position(|&item| match old_row {
                Some(VaultRow::Entry(_)) => matches!(page.vault_at(item), Some(VaultRow::Entry(k)) if page.vault.entries.get(k) == old_entry.as_ref()),
                Some(row) => page.vault_at(item) == Some(row),
                None => Some(item) == current,
            });
            if let Some(n) = at {
                page.selected = n;
                page.scroll = scroll;
            }
        }
        self.vault = view;
        self.reveal_selected();
        self.dirty = true;
    }

    pub fn settings_open(&self) -> bool {
        self.settings.is_some()
    }

    pub(crate) fn open_profile_settings(&mut self) {
        self.show_profile_settings(None);
    }

    pub(crate) fn select_profile_settings(&mut self, id: &str) {
        self.show_profile_settings(Some(id));
    }

    fn show_profile_settings(&mut self, select: Option<&str>) {
        self.open_settings();
        let profiles = self.profile_rows();
        if let Some(page) = self.settings.as_mut() {
            page.profiles = profiles;
            page.editor = None;
            page.capture = None;
            page.deleting = None;
            page.show_page(PAGES.iter().position(|p| *p == PROFILES).unwrap_or(0));
            if let Some(at) = page.shown.iter().position(|&i| match select {
                Some(id) => page.profile_at(i).is_some_and(|p| p.id == id),
                None => page.profile_at(i).is_some() || page.is_new_profile(i),
            }) {
                page.selected = at;
            }
        }
        self.reveal_selected();
        self.dirty = true;
    }

    pub(crate) fn fresh_settings_page(&self) -> SettingsPage {
        let rgb = |c: aster_vt::Rgb| u32::from_be_bytes([0, c.r, c.g, c.b]);
        let schemes = self.available_schemes().map(|(scheme, custom)| SchemeRow {
            name: scheme.name,
            custom,
            foreground: rgb(scheme.palette.foreground),
            background: rgb(scheme.palette.background),
            colors: scheme.palette.ansi.map(rgb),
        }).collect();
        let mut page = SettingsPage { profiles: self.profile_rows(), schemes, vault: self.vault.clone(), ..SettingsPage::default() };
        page.refilter();
        page
    }

    fn show_settings_page(&mut self, k: usize) {
        let current = self.current_scheme().to_owned();
        let Some(page) = self.settings.as_mut() else { return };
        page.show_page(k);
        if PAGES[page.page] == COLORS {
            if let Some(at) = page.scheme_item(&current).and_then(|item| page.shown.iter().position(|&i| i == item)) {
                page.selected = at;
            }
        }
        self.reveal_selected();
    }

    pub(crate) fn open_settings(&mut self) {
        use crate::app::{Tab, SETTINGS_PANE};
        self.overlay = None;
        match self.tabs.iter().position(|t| t.focus == SETTINGS_PANE) {
            Some(i) => self.active = i,
            None => {
                let at = self.tab_insert_index(false);
                self.tabs.insert(at, Tab::new(crate::panes::Node::Leaf(SETTINGS_PANE), SETTINGS_PANE));
                self.active = at;
            }
        }
        if self.settings.is_none() {
            let page = self.settings_stash.take().unwrap_or_else(|| self.fresh_settings_page());
            self.settings = Some(page);
        }
        self.dirty = true;
    }

    fn profile_rows(&self) -> Vec<ProfileRow> {
        let default = self.default_profile().map(|p| p.id);
        self.profiles
            .iter()
            .map(|p| ProfileRow {
                id: p.id.clone(),
                name: p.name.clone(),
                detail: match (&p.ssh, &p.serial, &p.socket) {
                    _ if p.layout.is_some() => t("Layout").to_owned(),
                    (Some(dest), _, _) => format!("ssh {dest}"),
                    (None, Some(spec), _) => format!("serial {spec}"),
                    (None, None, Some(address)) => address.clone(),
                    (None, None, None) => std::iter::once(p.command.clone())
                        .chain(p.args.iter().enumerate().map(|(i, a)| if i > 0 && p.args[i - 1].eq_ignore_ascii_case("-EncodedCommand") { "\u{2026}".to_owned() } else { a.clone() }))
                        .collect::<Vec<_>>()
                        .join(" "),
                },
                custom: self.config.profiles.iter().any(|c| c.id == p.id),
                default: default.as_deref() == Some(p.id.as_str()),
                hidden: self.config.profile_hidden(p),
            })
            .collect()
    }

    fn refresh_profiles(&mut self, select: Option<&str>) {
        let rows = self.profile_rows();
        let Some(page) = self.settings.as_mut() else { return };
        page.profiles = rows;
        let query = page.query.clone();
        page.refilter();
        page.query = query;
        if let Some(id) = select {
            if let Some(at) = page.shown.iter().position(|&i| page.profile_at(i).is_some_and(|p| p.id == id)) {
                page.selected = at;
            }
        }
        self.reveal_selected();
    }

    fn open_editor(&mut self, item: usize) {
        let Some(page) = self.settings.as_ref() else { return };
        if page.is_new_profile(item) {
            return self.open_overlay(crate::overlay::OverlayKind::ProfileTemplates);
        }
        let editor = if let Some(row) = page.profile_at(item) {
            let Some(p) = self.profiles.iter().find(|p| p.id == row.id) else { return };
            ProfileEditor {
                original: row.custom.then(|| p.id.clone()),
                source: Some(p.id.clone()),
                fields: [
                    if row.custom { p.name.clone() } else { tf("{name} (copy)", &[("name", &p.name)]) },
                    p.command.clone(),
                    join_args(&p.args),
                    p.cwd.clone().unwrap_or_default(),
                    p.ssh.clone().unwrap_or_default(),
                    p.serial.clone().unwrap_or_default(),
                    p.socket.clone().unwrap_or_default(),
                    p.group.clone().unwrap_or_default(),
                    p.scheme.clone().unwrap_or_default(),
                ],
                focus: 0,
                error: None,
            }
        } else {
            return;
        };
        if let Some(page) = self.settings.as_mut() {
            page.editor = Some(editor);
        }
        self.dirty = true;
    }

    pub(crate) fn edit_profile_template(&mut self, id: &str) {
        let Some(&(_, name, focus, prefix)) = TEMPLATES.iter().find(|&&(key, _, _, _)| key == id) else { return };
        let mut editor = ProfileEditor { focus, ..Default::default() };
        editor.fields[0] = t(name).to_owned();
        editor.fields[focus] = prefix.to_owned();
        if id == "local" {
            if let Some(shell) = self.profiles.iter().find(|p| p.builtin && p.is_local() && !p.command.is_empty() && !p.elevated) {
                editor.fields[1] = shell.command.clone();
                editor.fields[2] = join_args(&shell.args);
                editor.source = Some(shell.id.clone());
            }
        }
        if let Some(page) = self.settings.as_mut() {
            page.editor = Some(editor);
        }
    }

    pub(crate) fn custom_profile_id(&self, name: &str) -> String {
        let base = format!("custom:{}", slug(name));
        if self.profiles.iter().any(|p| p.id == base) {
            (2..).map(|n| format!("{base}-{n}")).find(|id| self.profiles.iter().all(|p| &p.id != id)).unwrap_or(base)
        } else {
            base
        }
    }

    fn save_editor(&mut self) {
        let Some(editor) = self.settings.as_ref().and_then(|p| p.editor.clone()) else { return };
        let [name, command, args, cwd, ssh, serial, socket, group, scheme] = editor.fields.map(|f| f.trim().to_owned());
        let fail = |app: &mut App, error: &str| {
            if let Some(e) = app.settings.as_mut().and_then(|p| p.editor.as_mut()) {
                e.error = Some(error.to_owned());
            }
            app.dirty = true;
        };
        if name.is_empty() {
            return fail(self, t("The profile needs a name"));
        }
        let source = editor.source.as_ref().and_then(|id| self.profiles.iter().find(|p| &p.id == id));
        if command.is_empty() && ssh.is_empty() && serial.is_empty() && socket.is_empty() && source.is_none_or(|p| p.layout.is_none()) {
            return fail(self, t("Give a command to run, an SSH destination, a serial port or a network address"));
        }
        let socket = match (!socket.is_empty()).then(|| aster_proto::address::parse(&socket)) {
            None => None,
            Some(Ok(address)) => Some(address.display()),
            Some(Err(_)) => return fail(self, t("The network address should look like telnet://host:23 or tcp://host:port")),
        };
        let scheme = if scheme.is_empty() {
            None
        } else if let Some(name) = self.scheme_names().into_iter().find(|name| name.eq_ignore_ascii_case(&scheme)) {
            Some(name)
        } else {
            return fail(self, t("There is no color scheme with this name"));
        };
        let id = editor.original.clone().unwrap_or_else(|| self.custom_profile_id(&name));
        let ssh = (!ssh.is_empty()).then_some(ssh);
        let (command, args) = match (&ssh, command.is_empty()) {
            (Some(dest), true) => ("ssh".to_owned(), vec![dest.clone()]),
            _ => (command, split_args(&args)),
        };
        let mut profile = crate::profile::Profile { id: id.clone(), name, command, args, cwd: (!cwd.is_empty()).then_some(cwd), ssh: ssh.clone(), serial: (!serial.is_empty()).then_some(serial), socket, group: (!group.is_empty()).then_some(group), scheme, builtin: false, hold: false, elevated: false, ..source.cloned().unwrap_or_default() };
        if ssh.is_some() && profile.icon.is_none() && profile.group.is_none() {
            profile.icon = Some("ssh".into());
        }
        let mut config = self.config.clone();
        match config.profiles.iter_mut().find(|p| p.id == id) {
            Some(existing) => *existing = profile,
            None => config.profiles.push(profile),
        }
        self.set_config(config);
        self.push_save_config();
        if let Some(page) = self.settings.as_mut() {
            page.editor = None;
        }
        self.refresh_profiles(Some(&id));
    }

    fn make_default(&mut self, id: &str) {
        let mut config = self.config.clone();
        config.terminal.profile = id.to_owned();
        if config != self.config {
            self.set_config(config);
            self.push_save_config();
        }
        self.refresh_profiles(Some(id));
    }

    fn toggle_profile_hidden(&mut self, row: ProfileRow) {
        let mut config = self.config.clone();
        if row.custom {
            let Some(profile) = config.profiles.iter_mut().find(|p| p.id == row.id) else { return };
            profile.hidden = !row.hidden;
        } else if row.hidden {
            config.hidden_profiles.retain(|id| id != &row.id);
        } else {
            config.hidden_profiles.push(row.id.clone());
        }
        self.set_config(config);
        self.push_save_config();
    }

    fn delete_profile(&mut self, id: &str) {
        let mut config = self.config.clone();
        config.profiles.retain(|p| p.id != id);
        if config.terminal.profile == id {
            config.terminal.profile.clear();
        }
        self.set_config(config);
        self.push_save_config();
        self.refresh_profiles(None);
    }

    fn editor_key(&mut self, key: Key, mods: Mods) {
        let Some(editor) = self.settings.as_mut().and_then(|p| p.editor.as_mut()) else { return };
        self.dirty = true;
        let last = FIELDS.len() - 1;
        match key {
            Key::Escape => {
                if let Some(p) = self.settings.as_mut() {
                    p.editor = None;
                }
            }
            Key::Enter if mods.ctrl || editor.focus == last => self.save_editor(),
            Key::Char('S') if mods.ctrl => self.save_editor(),
            Key::Enter | Key::Down => editor.focus = (editor.focus + 1).min(last),
            Key::Tab if mods.shift => editor.focus = editor.focus.saturating_sub(1),
            Key::Tab => editor.focus = (editor.focus + 1) % FIELDS.len(),
            Key::Up => editor.focus = editor.focus.saturating_sub(1),
            Key::Backspace => {
                let field = &mut editor.fields[editor.focus];
                if mods.ctrl {
                    field.clear();
                } else {
                    field.pop();
                }
            }
            Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
            _ => {}
        }
    }

    pub(crate) fn close_settings(&mut self) {
        self.commit_edit();
        self.settings = None;
        self.settings_stash = None;
        if let Some(i) = self.tabs.iter().position(|t| t.focus == crate::app::SETTINGS_PANE) {
            self.remove_tab(i);
        }
        self.dirty = true;
    }

    pub fn settings_geometry(&self) -> Option<SettingsGeometry> {
        self.settings.as_ref().map(|p| geometry(self.chrome.content, self.ui(), p))
    }

    pub fn setting_options(&self, control: Control) -> Vec<(String, String)> {
        match control {
            Control::Choice(list) => list.iter().map(|(v, l)| ((*v).to_owned(), t(l).to_owned())).collect(),
            Control::Schemes => self.scheme_names().into_iter().map(|n| (n.clone(), n)).collect(),
            Control::Profiles => std::iter::once((String::new(), t("First available").to_owned())).chain(self.profiles.iter().map(|p| (p.id.clone(), p.name.clone()))).collect(),
            _ => Vec::new(),
        }
    }

    pub fn setting_text(&self, setting: &Setting) -> String {
        let value = get(&self.config, setting.key);
        match (setting.control, &value) {
            (Control::Number { .. }, Some(Value::Integer(n))) => n.to_string(),
            (Control::Number { .. }, Some(Value::Float(f))) => number_text(*f),
            (Control::Choice(_) | Control::Schemes | Control::Profiles, Some(Value::String(v))) => {
                self.setting_options(setting.control).into_iter().find(|(o, _)| o.eq_ignore_ascii_case(v)).map(|(_, l)| l).unwrap_or_else(|| v.clone())
            }
            (_, Some(Value::String(v))) => v.clone(),
            (_, Some(Value::Boolean(b))) => b.to_string(),
            _ => String::new(),
        }
    }

    pub fn setting_on(&self, setting: &Setting) -> bool {
        matches!(get(&self.config, setting.key), Some(Value::Boolean(true)))
    }

    pub fn shortcut_text(&self, action: Action) -> String {
        let all: Vec<String> = self.keymap.bindings_of(action).map(format_binding).collect();
        if all.is_empty() {
            t("None").to_owned()
        } else {
            all.join(", ")
        }
    }

    fn set_shortcuts(&mut self, action: Action, bindings: Option<Vec<String>>) {
        let mut config = self.config.clone();
        match bindings {
            Some(b) => {
                config.hotkeys.insert(action.id(), b);
            }
            None => {
                config.hotkeys.remove(&action.id());
            }
        }
        if config != self.config {
            self.set_config(config);
            self.push_save_config();
        }
    }

    fn assign_shortcut(&mut self, action: Action, chord: Chord) {
        let text = chord.to_string();
        let mut taken = Vec::new();
        let mut config = self.config.clone();
        for other in Action::all().filter(|o| *o != action) {
            let current: Vec<String> = self.keymap.bindings_of(other).map(format_binding).collect();
            if current.contains(&text) {
                config.hotkeys.insert(other.id(), current.into_iter().filter(|b| *b != text).collect());
                taken.push(other.label());
            }
        }
        config.hotkeys.insert(action.id(), vec![text.clone()]);
        if config != self.config {
            self.set_config(config);
            self.push_save_config();
        }
        if let Some(page) = self.settings.as_mut() {
            page.capture = None;
            page.notice = (!taken.is_empty()).then(|| tf("{shortcut} was taken from {actions}", &[("shortcut", &text), ("actions", &taken.join(", "))]));
        }
    }

    fn capture_key(&mut self, key: Key, mods: Mods) {
        let Some(action) = self.settings.as_ref().and_then(|p| p.capture) else { return };
        match key {
            Key::Escape if mods == Mods::NONE => {
                if let Some(p) = self.settings.as_mut() {
                    p.capture = None;
                }
            }
            Key::Other => {}
            _ => self.assign_shortcut(action, Chord { key, mods }),
        }
        self.dirty = true;
    }

    fn apply_setting(&mut self, key: &str, value: Value) {
        if let Ok(config) = set(&self.config, key, value) {
            if config != self.config {
                self.set_config(config);
                self.push_save_config();
            }
        }
        self.dirty = true;
    }

    fn adjust_setting(&mut self, setting: &Setting, delta: i32) {
        match setting.control {
            Control::Toggle => {
                let on = if delta == 0 { !self.setting_on(setting) } else { delta > 0 };
                self.apply_setting(setting.key, Value::Boolean(on));
            }
            Control::Choice(_) | Control::Schemes | Control::Profiles => {
                let options = self.setting_options(setting.control);
                if options.is_empty() {
                    return;
                }
                let current = match get(&self.config, setting.key) {
                    Some(Value::String(v)) => options.iter().position(|(o, _)| o.eq_ignore_ascii_case(&v)),
                    _ => None,
                };
                let n = options.len() as i32;
                let next = match current {
                    Some(at) => (at as i32 + if delta < 0 { -1 } else { 1 }).rem_euclid(n),
                    None => 0,
                } as usize;
                self.apply_setting(setting.key, Value::String(options[next].0.clone()));
            }
            Control::Number { step, .. } => {
                if delta == 0 {
                    return self.begin_edit();
                }
                let now = match get(&self.config, setting.key) {
                    Some(Value::Integer(n)) => n as f64,
                    Some(Value::Float(f)) => f,
                    _ => return,
                };
                self.set_number(setting, now + step * delta as f64);
            }
            Control::Text => self.begin_edit(),
            Control::Run(action) => self.run(action),
        }
    }

    fn set_number(&mut self, setting: &Setting, wanted: f64) {
        let Control::Number { min, max, step } = setting.control else { return };
        let snapped = ((wanted / step).round() * step).clamp(min, max);
        let value = match get(&self.config, setting.key) {
            Some(Value::Integer(_)) => Value::Integer(snapped.round() as i64),
            _ => Value::Float((snapped * 1000.0).round() / 1000.0),
        };
        self.apply_setting(setting.key, value);
    }

    fn begin_edit(&mut self) {
        let Some(setting) = self.settings.as_ref().and_then(|p| p.current()) else { return };
        let text = match setting.control {
            Control::Number { .. } | Control::Text => self.setting_text(setting),
            _ => return,
        };
        if let Some(p) = self.settings.as_mut() {
            p.editing = Some(InlineEdit::Setting(text));
        }
        self.dirty = true;
    }

    fn commit_edit(&mut self) {
        let Some(page) = self.settings.as_mut() else { return };
        let Some(InlineEdit::Setting(text)) = page.editing.take() else { return };
        let Some(setting) = page.current() else { return };
        match setting.control {
            Control::Text => self.apply_setting(setting.key, Value::String(text.trim().to_owned())),
            Control::Number { .. } => {
                if let Ok(v) = text.trim().replace(',', ".").parse::<f64>() {
                    self.set_number(setting, v);
                }
            }
            _ => {}
        }
        self.dirty = true;
    }

    fn activate_vault_row(&mut self, row: VaultRow) {
        let Some(page) = self.settings.as_mut() else { return };
        match row {
            VaultRow::Lock => self.push(Effect::VaultLock),
            VaultRow::Erase => self.confirm_vault_delete(row),
            _ => {
                page.confirming_vault = None;
                page.notice = None;
                page.editing = row.edit();
                self.vault_input_id = self.vault_input_id.wrapping_add(1);
            }
        }
    }

    fn submit_passphrase(&mut self) {
        let Some(page) = self.settings.as_mut() else { return };
        let Some(InlineEdit::Passphrase { step, value }) = page.editing.take() else { return };
        if value.is_empty() {
            page.notice = Some(VaultIssue::EmptyPassphrase.message().to_owned());
            page.editing = Some(InlineEdit::Passphrase { step, value });
            return;
        }
        self.vault_input_id = self.vault_input_id.wrapping_add(1);
        let effect = match step {
            PassphraseStep::Create => Effect::VaultCreate { passphrase: value },
            PassphraseStep::Unlock => Effect::VaultUnlock { passphrase: value },
            PassphraseStep::Current => {
                page.editing = Some(InlineEdit::Passphrase { step: PassphraseStep::New { old: value }, value: String::new() });
                return;
            }
            PassphraseStep::New { old } => Effect::VaultChange { old, new: value },
        };
        page.editing = Some(InlineEdit::Waiting);
        self.push(effect);
    }

    fn confirm_vault_delete(&mut self, row: VaultRow) {
        let Some(page) = self.settings.as_mut() else { return };
        let confirmed = page.confirming_vault == Some(row);
        let effect = match row {
            VaultRow::Entry(i) => {
                let (kind, key) = &page.vault.entries[i];
                if confirmed {
                    Some(Effect::VaultRemove { kind: kind.clone(), key: key.clone() })
                } else {
                    page.notice = Some(tf("Press Del again to delete {name}", &[("name", key)]));
                    None
                }
            }
            VaultRow::Erase if confirmed => Some(Effect::VaultErase),
            VaultRow::Erase => {
                page.notice = Some(t("Confirm again to permanently erase the vault").to_owned());
                None
            }
            _ => return,
        };
        page.confirming_vault = (!confirmed).then_some(row);
        if let Some(effect) = effect {
            page.notice = None;
            self.push(effect);
        }
    }

    fn reveal_selected(&mut self) {
        let Some(geo) = self.settings_geometry() else { return };
        let Some(page) = self.settings.as_mut() else { return };
        let Some(row) = geo.rows.iter().find(|r| r.kind == RowKind::Item(page.selected)) else { return };
        let header = geo.rows.iter().position(|r| r.kind == RowKind::Item(page.selected)).filter(|&i| i > 0 && matches!(geo.rows[i - 1].kind, RowKind::Section(_))).map(|i| geo.rows[i - 1].rect.h).unwrap_or(0.0);
        let top = row.rect.y - header;
        if top < geo.list.y {
            page.scroll -= geo.list.y - top;
        } else if row.rect.bottom() > geo.list.bottom() {
            page.scroll += row.rect.bottom() - geo.list.bottom();
        }
        page.scroll = page.scroll.clamp(0.0, (geo.height - geo.list.h).max(0.0));
    }

    pub(crate) fn settings_key(&mut self, key: Key, mods: Mods) {
        let Some(page) = self.settings.as_mut() else { return };
        self.dirty = true;
        if page.capture.is_some() {
            return self.capture_key(key, mods);
        }
        if page.editor.is_some() {
            return self.editor_key(key, mods);
        }
        if matches!(page.editing, Some(InlineEdit::Interrupted | InlineEdit::Waiting)) {
            if matches!(key, Key::Escape | Key::Tab) || key == Key::Enter && matches!(page.editing, Some(InlineEdit::Interrupted)) {
                page.editing = None;
            }
            return;
        }
        if !page.editing() {
            if let Some(row) = page.current_item().and_then(|item| page.vault_at(item)) {
                match key {
                    Key::Delete if matches!(row, VaultRow::Entry(_) | VaultRow::Erase) => return self.confirm_vault_delete(row),
                    Key::Enter => return self.activate_vault_row(row),
                    _ => {}
                }
            }
            page.confirming_vault = None;
        }
        if let Some(item) = page.current_item() {
            let row = page.profile_at(item).cloned();
            if row.is_some() || page.is_new_profile(item) {
                if key != Key::Delete {
                    page.deleting = None;
                }
                match (key, row) {
                    (Key::Char('H'), Some(r)) if mods == Mods::NONE => {
                        self.swallow_chord = Some(key);
                        return self.toggle_profile_hidden(r);
                    }
                    (Key::Enter, Some(r)) if mods.ctrl => return self.make_default(&r.id),
                    (Key::Enter, _) => return self.open_editor(item),
                    (Key::Delete, Some(r)) if r.custom => {
                        if page.deleting.as_deref() == Some(r.id.as_str()) {
                            page.deleting = None;
                            return self.delete_profile(&r.id);
                        }
                        page.notice = Some(tf("Press Del again to delete {name}", &[("name", &r.name)]));
                        page.deleting = Some(r.id);
                        return;
                    }
                    (Key::Delete, Some(_)) => {
                        page.notice = Some(t("Detected profiles cannot be deleted; copy one with Enter instead").to_owned());
                        return;
                    }
                    _ => {}
                }
            }
        }
        if key != Key::Other {
            page.notice = None;
        }
        if page.editing.is_some() {
            match key {
                Key::Escape => page.editing = None,
                Key::Enter if matches!(page.editing, Some(InlineEdit::Passphrase { .. })) => self.submit_passphrase(),
                Key::Enter | Key::Tab => self.commit_edit(),
                Key::Backspace => {
                    if let Some(t) = page.editing.as_mut().and_then(InlineEdit::buffer) {
                        if mods.ctrl {
                            t.clear();
                        } else {
                            t.pop();
                        }
                    }
                }
                Key::Char('V') if mods.ctrl => {
                    let effect = if page.editing_passphrase() { Effect::RequestVaultPaste(self.vault_input_id) } else { Effect::RequestPaste };
                    self.push(effect);
                }
                _ => {}
            }
            return;
        }
        let current = page.current();
        if let Some(action) = page.current_shortcut() {
            match key {
                Key::Enter => {
                    page.capture = Some(action);
                    return;
                }
                Key::Delete if mods.shift => return self.set_shortcuts(action, None),
                Key::Delete => return self.set_shortcuts(action, Some(Vec::new())),
                _ => {}
            }
        }
        match key {
            Key::Escape if !page.query.is_empty() => {
                page.query.clear();
                page.refilter();
            }
            Key::Escape => return self.close_settings(),
            Key::Up => page.step(-1),
            Key::Down | Key::Tab => page.step(1),
            Key::PageUp if mods.ctrl => {
                let k = (page.page + PAGES.len() - 1) % PAGES.len();
                return self.show_settings_page(k);
            }
            Key::PageDown if mods.ctrl => {
                let k = (page.page + 1) % PAGES.len();
                return self.show_settings_page(k);
            }
            Key::PageUp => page.step(-6),
            Key::PageDown => page.step(6),
            Key::Home if mods.ctrl => page.step(isize::MIN / 2),
            Key::End if mods.ctrl => page.step(isize::MAX / 2),
            Key::Backspace => {
                if mods.ctrl {
                    page.query.clear();
                } else {
                    page.query.pop();
                }
                page.refilter();
            }
            Key::Enter => {
                if let Some(s) = current {
                    self.adjust_setting(s, 0);
                } else if let Some(scheme) = page.current_item().and_then(|i| page.scheme_at(i)) {
                    let name = scheme.name.clone();
                    self.apply_scheme(name);
                }
            }
            Key::Left | Key::Right => {
                if let Some(s) = current {
                    self.adjust_setting(s, if key == Key::Left { -1 } else { 1 });
                }
            }
            Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
            _ => {}
        }
        self.reveal_selected();
    }

    pub(crate) fn settings_text(&mut self, text: &str) {
        let Some(page) = self.settings.as_mut() else { return };
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        if let Some(editor) = page.editor.as_mut() {
            editor.fields[editor.focus].push_str(&clean);
            self.dirty = true;
            return;
        }
        match page.editing.as_mut() {
            Some(edit) => {
                if let Some(buffer) = edit.buffer() {
                    buffer.push_str(&clean);
                }
            }
            None => {
                page.query.push_str(&clean);
                page.refilter();
            }
        }
        self.dirty = true;
    }

    pub(crate) fn settings_click(&mut self, x: f32, y: f32) {
        let Some(geo) = self.settings_geometry() else { return };
        self.dirty = true;
        if let Some(page) = self.settings.as_mut() {
            if page.confirming_vault.is_some() && !geo.rows.iter().any(|row| row.kind == RowKind::Item(page.selected) && row.rect.contains(x, y)) {
                page.cancel_vault_input();
            }
        }
        if geo.close.contains(x, y) {
            return self.close_settings();
        }
        if geo.field.contains(x, y) {
            return self.commit_edit();
        }
        if let Some(k) = geo.pages.iter().position(|r| r.contains(x, y)) {
            self.commit_edit();
            return self.show_settings_page(k);
        }
        if !geo.list.contains(x, y) {
            return self.commit_edit();
        }
        let Some(row) = geo.rows.iter().find(|r| r.rect.contains(x, y)).copied() else { return self.commit_edit() };
        let RowKind::Item(n) = row.kind else { return self.commit_edit() };
        let editing_same = self.settings.as_ref().is_some_and(|p| p.selected == n && p.editing.is_some());
        if editing_same && self.settings.as_ref().is_some_and(|p| matches!(p.editing, Some(InlineEdit::Passphrase { .. } | InlineEdit::Waiting))) {
            return;
        }
        let control = self.settings.as_ref().and_then(|p| SETTINGS.get(p.shown[n])).map(|s| s.control);
        if editing_same && row.control.contains(x, y) && control.is_some_and(|c| part_at(c, row.control, self.ui(), x) == Part::Whole) {
            return;
        }
        self.commit_edit();
        let Some(page) = self.settings.as_mut() else { return };
        if page.selected != n {
            page.confirming_vault = None;
        }
        page.selected = n;
        page.notice = None;
        let item = page.shown[n];
        if let Some(row) = page.vault_at(item) {
            return self.activate_vault_row(row);
        }
        if let Some(scheme) = page.scheme_at(item) {
            let name = scheme.name.clone();
            return self.apply_scheme(name);
        }
        if page.profile_at(item).is_some() || page.is_new_profile(item) {
            return self.open_editor(item);
        }
        if let Some(action) = shortcut(page.shown[n]) {
            page.capture = Some(action);
            return;
        }
        let setting = &SETTINGS[page.shown[n]];
        match setting.control {
            Control::Toggle => self.adjust_setting(setting, 0),
            control if row.control.contains(x, y) => match part_at(control, row.control, self.ui(), x) {
                Part::Less => self.adjust_setting(setting, -1),
                Part::More => self.adjust_setting(setting, 1),
                Part::Whole => self.adjust_setting(setting, 0),
            },
            _ => {}
        }
    }

    pub(crate) fn settings_wheel(&mut self, dy: f32) {
        let Some(geo) = self.settings_geometry() else { return };
        let step = 60.0 * self.ui();
        let Some(page) = self.settings.as_mut() else { return };
        page.scroll = (page.scroll - dy * step).clamp(0.0, (geo.height - geo.list.h).max(0.0));
        self.dirty = true;
    }
}

pub fn explorer_label() -> &'static str {
    t("Open in Aster")
}

pub fn tray_menu_labels() -> [&'static str; 2] {
    [t("New window"), t("Quit")]
}

#[cfg(test)]
#[path = "../test/settings.rs"]
mod tests;
