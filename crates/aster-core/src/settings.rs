use crate::vault_settings::{AfterUnlock, DialogKind, VaultDialog};
use crate::app::{App, Drag, Effect};
use crate::config::Config;
use crate::i18n::{t, tf};
use crate::input::{Key, Mods};
use crate::keymap::{format_binding, Action, Chord};
use crate::profiles_page::{self, GroupEditor, Header, TreeKind, TreeRow};
use crate::profiles_page::editor::ProfileEditor;
use crate::settings_colors::{SchemeBar, SchemeTab};
use std::collections::BTreeSet;
use aster_gfx::render::Rect;
use toml::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    Toggle,
    Choice(&'static [(&'static str, &'static str)]),
    Segments(&'static [(&'static str, &'static str)]),
    Schemes,
    Profiles,
    Number { min: f64, max: f64, step: f64 },
    Slider { min: f64, max: f64, step: f64 },
    Color,
    Text,
    StringList,
    Font { monospace: bool },
    Run(&'static Action),
    ReadOnly(Info),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Info {
    ConfigPath,
    ConfigStatus,
    SshProfiles,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setting {
    pub section: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub control: Control,
    pub restart: bool,
}

const fn item(section: &'static str, key: &'static str, label: &'static str, hint: &'static str, control: Control) -> Setting {
    Setting { section, key, label, hint, control, restart: false }
}

const fn restart(setting: Setting) -> Setting {
    Setting { restart: true, ..setting }
}

#[derive(Clone, PartialEq)]
pub struct RestartSettings(Vec<Option<Value>>);

impl RestartSettings {
    pub fn from_config(config: &Config) -> Self {
        Self(SETTINGS.iter().filter(|s| s.restart).map(|s| get(config, s.key)).collect())
    }
}

impl Control {
    pub(crate) fn number(self) -> Option<(f64, f64, f64)> {
        match self {
            Self::Number { min, max, step } | Self::Slider { min, max, step } => Some((min, max, step)),
            _ => None,
        }
    }
}

pub(crate) fn slider_track(rect: Rect, scale: f32) -> Rect {
    Rect::new(rect.x + 7.0 * scale, rect.y + (rect.h - 4.0 * scale) / 2.0, (rect.w - 61.0 * scale).max(1.0), 4.0 * scale)
}

pub(crate) fn color_value(text: &str) -> Option<aster_gfx::render::Color> {
    let text = text.trim().strip_prefix('#')?;
    if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(text, 16).ok().map(aster_gfx::render::Color::rgb)
}

const TERMINAL: &str = "Terminal";
const INPUT: &str = "Clipboard and mouse";
const SEARCH: &str = "Search";
const APPEARANCE: &str = "Appearance";
const WINDOW: &str = "Window";
const COLORS: &str = "Color scheme";
const DOCK: &str = "Docked window";
const VAULT: &str = "Vault";
const SSH: &str = "SSH";
pub(crate) const SYSTEM: &str = "Application";
pub(crate) const CONFIG_FILE: &str = "Config file";

const CURSORS: &[(&str, &str)] = &[("block", "Block"), ("beam", "Beam"), ("underline", "Underline")];
const LINK_MODIFIERS: &[(&str, &str)] = &[("none", "None"), ("ctrl", "Ctrl"), ("shift", "Shift")];
const RIGHT_CLICK: &[(&str, &str)] = &[("off", "Nothing"), ("menu", "Context menu"), ("paste", "Paste"), ("clipboard", "Copy or paste")];
const SCHEME_MODES: &[(&str, &str)] = &[("auto", "Follow the system"), ("dark", "Always dark"), ("light", "Always light")];
const BACKGROUND_FITS: &[(&str, &str)] = &[("cover", "Cover pane"), ("contain", "Fit inside pane"), ("stretch", "Stretch to pane"), ("tile", "Tile image")];
const BELLS: &[(&str, &str)] = &[("off", "Off"), ("visual", "Flash the pane"), ("audible", "System sound")];
const IDENTIFICATION: &[(&str, &str)] = &[("aster", "Aster"), ("xterm", "Compatibility")];
const TAB_BARS: &[(&str, &str)] = &[("always", "Always"), ("auto", "With two or more tabs"), ("never", "Never")];
const TAB_WIDTHS: &[(&str, &str)] = &[("fit", "Fit"), ("fixed", "Fixed")];
const TAB_POSITIONS: &[(&str, &str)] = &[("top", "Top"), ("bottom", "Bottom"), ("left", "Left"), ("right", "Right")];
const BACKDROPS: &[(&str, &str)] = &[("none", "None"), ("acrylic", "Acrylic"), ("mica", "Mica")];
const QUICK_CONNECT: &[(&str, &str)] = &[("ssh", "SSH"), ("telnet", "Telnet"), ("tcp", "TCP"), ("serial", "Serial port")];
const SSH_CLIENTS: &[(&str, &str)] = &[("native", "Built in"), ("system", "System OpenSSH")];
#[cfg(windows)]
const SSH_AGENTS: &[(&str, &str)] = &[("auto", "Automatic"), ("pageant", "Pageant"), ("named-pipe", "Named pipe")];
const LANGUAGES: &[(&str, &str)] = &[("auto", "Automatic"), ("en", "English"), ("ru", "Russian")];
const EDGES: &[(&str, &str)] = &[("off", "Off"), ("top", "Top"), ("bottom", "Bottom"), ("left", "Left"), ("right", "Right")];
const SHARE: Control = Control::Slider { min: 0.1, max: 1.0, step: 0.05 };

pub const SETTINGS: &[Setting] = &[
    item(SHORTCUTS, "palette.hidden-commands", "Hidden palette commands", "Comma-separated action IDs to hide from the command palette", Control::StringList),
    item(PROFILES, "terminal.profile", "Default profile for new tabs", "Shell for new tabs and windows", Control::Profiles),
    item(PROFILES, "terminal.recent-profiles", "Recent profiles", "Number of recent profiles to show; zero hides the list", Control::Number { min: 0.0, max: 100.0, step: 1.0 }),
    item(PROFILES, "picker.show-builtin-profiles", "Show built-in profiles", "Include built-in shells in the profile picker", Control::Toggle),
    item(PROFILES, "picker.quick-connect", "Default quick connect provider", "Protocol used when an address has no prefix", Control::Choice(QUICK_CONNECT)),
    item(TERMINAL, "terminal.scrollback-lines", "Scrollback", "Lines kept above the screen", Control::Number { min: 0.0, max: 1_000_000.0, step: 1000.0 }),
    item(TERMINAL, "terminal.draw-bold-text-in-bright-colors", "Bold text in bright colors", "Bold text uses the bright palette colors", Control::Toggle),
    item(TERMINAL, "terminal.env", "Environment variables", "Set key=value entries for all local shells", Control::Run(&Action::EditEnvironment)),
    #[cfg(windows)]
    item(TERMINAL, "terminal.refresh-environment", "Refresh environment", "Read Windows environment variables for every new local session", Control::Toggle),
    #[cfg(windows)]
    item(TERMINAL, "terminal.set-comspec", "Aster environment marker", "Set ASTER=1 in new local shells and preserve COMSPEC", Control::Toggle),
    item(TERMINAL, "terminal.identification", "Terminal identification", "Report the Aster name and version to new shells", Control::Choice(IDENTIFICATION)),
    item(TERMINAL, "terminal.recover-tabs", "Restore tabs", "Reopen tabs, splits and their folders on start", Control::Toggle),
    item(TERMINAL, "terminal.confirm-close", "Confirm closing running programs", "Ask before closing a tab, pane or window where a program is still running", Control::Toggle),
    item(TERMINAL, "terminal.bell", "Bell", "What happens when a program rings the bell", Control::Segments(BELLS)),
    item(TERMINAL, "terminal.alt-is-meta", "Alt sends Meta", "Prefix letters typed with Alt with Escape", Control::Toggle),
    item(TERMINAL, "terminal.bell-attention", "Request attention on bell", "Flash the taskbar when an unfocused window rings", Control::Toggle),
    restart(item(TERMINAL, "terminal.open-on-start", "Open a terminal on start", "Open the default profile when no session is restored", Control::Toggle)),
    item(TERMINAL, "terminal.scroll-on-input", "Scroll on input", "Typing scrolls back to the bottom", Control::Toggle),
    item(INPUT, "terminal.copy-on-select", "Copy on select", "Selected text goes to the clipboard", Control::Toggle),
    item(INPUT, "terminal.copy-as-html", "Copy as HTML", "Include text colors and bold weight when copying a selection", Control::Toggle),
    item(INPUT, "terminal.link-modifier", "Modifier for opening links", "Hold this key when clicking a link", Control::Choice(LINK_MODIFIERS)),
    item(INPUT, "terminal.paste-newlines-as-spaces", "Paste line breaks as spaces", "Replace each line break with a space before pasting", Control::Toggle),
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
    item(APPEARANCE, "terminal.font", "Font", "Monospace font family for the terminal", Control::Font { monospace: true }),
    item(APPEARANCE, "terminal.font-size", "Font size", "Size in points before zoom", Control::Number { min: 6.0, max: 48.0, step: 1.0 }),
    item(APPEARANCE, "terminal.font-weight", "Font weight", "Weight of regular terminal text", Control::Number { min: 100.0, max: 900.0, step: 100.0 }),
    item(APPEARANCE, "terminal.font-weight-bold", "Bold font weight", "Weight of bold terminal text", Control::Number { min: 100.0, max: 900.0, step: 100.0 }),
    item(APPEARANCE, "terminal.line-padding", "Line padding", "Extra pixels between lines", Control::Number { min: 0.0, max: 20.0, step: 1.0 }),
    item(APPEARANCE, "terminal.padding", "Text padding", "Space between the text and the edges of a pane", Control::Number { min: 0.0, max: 64.0, step: 1.0 }),
    item(APPEARANCE, "terminal.cursor", "Cursor shape", "Shape of the text cursor", Control::Segments(CURSORS)),
    item(APPEARANCE, "terminal.cursor-blink", "Blinking cursor", "The cursor blinks while the window has focus", Control::Toggle),
    item(APPEARANCE, "terminal.minimum-contrast-ratio", "Minimum contrast ratio", "Adjust text color for readability; 1 disables the adjustment", Control::Number { min: 1.0, max: 21.0, step: 0.5 }),
    item(APPEARANCE, "appearance.ui-font", "Interface font", "Font of tabs and menus; empty uses the system font", Control::Font { monospace: false }),
    item(APPEARANCE, "appearance.ui-from-scheme", "Interface colors from the scheme", "Use the terminal scheme for tabs, settings, menus and dialogs", Control::Toggle),
    item(APPEARANCE, "terminal.fallback-font", "Fallback font", "Font to try for missing terminal characters before system fallback", Control::Font { monospace: true }),
    item(APPEARANCE, "terminal.ligatures", "Font ligatures", "Allow the terminal font to join adjacent characters", Control::Toggle),
    item(APPEARANCE, "appearance.background-image", "Background image", "Path to a PNG or JPEG image", Control::Text),
    item(APPEARANCE, "appearance.background-image-opacity", "Background image opacity", "Blend the image with the terminal background", Control::Number { min: 0.0, max: 1.0, step: 0.01 }),
    item(APPEARANCE, "appearance.background-image-fit", "Background image fit", "Choose how the image fills each pane", Control::Choice(BACKGROUND_FITS)),
    item(WINDOW, "appearance.opacity", "Opacity", "Background opacity of the terminal", Control::Slider { min: 0.2, max: 1.0, step: 0.05 }),
    item(WINDOW, "appearance.backdrop", "Backdrop", "System material behind the window on Windows 11", Control::Choice(BACKDROPS)),
    item(WINDOW, "appearance.density", "Interface density", "Scale tabs, menus, settings and padding", Control::Number { min: 0.6, max: 1.4, step: 0.1 }),
    item(WINDOW, "appearance.tab-bar", "Tab bar", "Ctrl+Shift+B shows or hides tabs and window buttons", Control::Segments(TAB_BARS)),
    item(WINDOW, "appearance.tab-index", "Show tab numbers", "Display the tab number before its title", Control::Toggle),
    item(WINDOW, "appearance.tab-icon", "Show tab icons", "Show a profile icon on each tab", Control::Toggle),
    item(WINDOW, "appearance.tab-close-buttons", "Show close buttons on tabs", "Show a close button on unpinned tabs", Control::Toggle),
    item(WINDOW, "appearance.tab-menu-button", "Show tab menu button", "Show a button for the active tab menu", Control::Toggle),
    item(WINDOW, "appearance.pane-titles", "Always show pane titles", "Show a header even on panes without a custom name", Control::Toggle),
    item(WINDOW, "panes.resize-step", "Pane resize step", "Share of the split moved by a resize shortcut", Control::Number { min: 0.1, max: 0.9, step: 0.1 }),
    item(WINDOW, "panes.focus-follows-mouse", "Focus follows the mouse", "Moving the pointer over a pane makes it the active one", Control::Toggle),
    item(WINDOW, "appearance.profile-sidebar", "Profile sidebar", "Show the profile tree next to the terminal", Control::Toggle),
    item(WINDOW, "appearance.profile-sidebar-width", "Profile sidebar width", "Width of the profile tree in logical pixels", Control::Number { min: crate::profile_sidebar::MIN_WIDTH as f64, max: crate::profile_sidebar::MAX_WIDTH as f64, step: 16.0 }),
    item(WINDOW, "appearance.tabs-in-fullscreen", "Keep the tab bar in full screen", "Use the usual tab bar visibility in full screen", Control::Toggle),
    item(WINDOW, "appearance.tab-width", "Tab width", "Choose the width of horizontal tabs", Control::Segments(TAB_WIDTHS)),
    item(WINDOW, "appearance.tab-bar-position", "Tab bar position", "Choose the edge of the window for the tab bar", Control::Segments(TAB_POSITIONS)),
    item(WINDOW, "appearance.mru-tabs", "Switch tabs in order of recent use", "Hold Ctrl and press Tab to choose a recently used tab", Control::Toggle),
    item(WINDOW, "appearance.cycle-tabs", "Cycle tabs", "Switching past the last tab wraps around", Control::Toggle),
    item(WINDOW, "appearance.animations", "Animations", "Show tab name toasts, visual bells and cursor blink", Control::Toggle),
    item(WINDOW, "terminal.close-window-with-last-tab", "Last tab closes the window", "Closing the only tab closes its window", Control::Toggle),
    item(WINDOW, "notifications.command-finished", "Notify when a command finishes", "Show system notifications for watched commands and finished sessions that stay open", Control::Toggle),
    item(WINDOW, "notifications.activity", "Notify about tab activity", "Notify once when output resumes in a background tab after a quiet period", Control::Toggle),
    item(WINDOW, "notifications.activity-silence-seconds", "Quiet period for activity notifications", "Seconds without output before new output can trigger a notification", Control::Number { min: 1.0, max: 3600.0, step: 1.0 }),
    item(WINDOW, "appearance.tray", "Notification area icon", "Keep Aster running after its last window closes on Windows. Has no effect on Linux.", Control::Toggle),
    item(COLORS, "appearance.color-scheme-mode", "Theme", "Dark, light or the system setting", Control::Segments(SCHEME_MODES)),
    item(COLORS, "appearance.color-scheme", "Dark color scheme", "Colors of the dark theme", Control::Schemes),
    item(COLORS, "appearance.light-color-scheme", "Light color scheme", "Colors of the light theme", Control::Schemes),
    item(COLORS, "colors.generate-256", "Generate a 256-color palette", "Interpolate the extended palette from the 16 base colors", Control::Toggle),
    item(COLORS, "colors.harmonious", "Harmonious palette", "Reverse the generated palette lightness on light schemes", Control::Toggle),
    item(DOCK, "appearance.dock", "Dock to edge", "Pin the window to a screen edge", Control::Choice(EDGES)),
    item(DOCK, "dock.screen", "Dock screen", "Enter current, primary, or a monitor number starting at 1", Control::Text),
    item(DOCK, "appearance.dock-fill", "Size", "Share of the screen across the edge", SHARE),
    item(DOCK, "appearance.dock-space", "Length", "Share of the screen along the edge", SHARE),
    item(DOCK, "appearance.dock-always-on-top", "Always on top", "The docked window stays above other windows", Control::Toggle),
    item(DOCK, "appearance.dock-hide-on-blur", "Hide on focus loss", "The docked window hides when another window is activated", Control::Toggle),
    item(DOCK, "appearance.global-hotkey", "Global hotkey", "Shows and hides the window from anywhere, for example Ctrl-`", Control::Text),
    item(VAULT, "vault.encrypt-config", "Encrypt the entire config", "Ask for the master passphrase when Aster starts", Control::Toggle),
    item(VAULT, "vault.remember-minutes", "Stay unlocked for (minutes)", "Zero asks for the master passphrase each time", Control::Number { min: 0.0, max: 1440.0, step: 1.0 }),
    item(SSH, "ssh.client", "SSH client", "Built-in client, or the ssh program installed in the system", Control::Choice(SSH_CLIENTS)),
    item(SSH, "ssh.remember-passwords", "Offer to save passwords", "After a typed password or key passphrase works, offer to save it", Control::Toggle),
    item(SSH, "ssh.forward-agent", "Forward the SSH agent", "Let servers use your local SSH agent; ForwardAgent in ~/.ssh/config decides per host", Control::Toggle),
    item(SSH, "ssh.share-connections", "Share connections", "New tabs to a host that is already open reuse its login", Control::Toggle),
    item(SSH, "ssh.warn-on-close", "Warn before closing active SSH connections", "Ask before closing a pane, tab or window with an SSH session", Control::Toggle),
    item(SSH, "ssh.verify-host-keys", "Verify host keys", "Check server keys against known hosts in the built-in client", Control::Toggle),
    #[cfg(windows)]
    item(SSH, "ssh.windows-agent", "SSH agent type", "Select the local agent used by the built-in SSH client on Windows", Control::Choice(SSH_AGENTS)),
    #[cfg(windows)]
    item(SSH, "ssh.agent-pipe", "SSH agent pipe path", "Leave empty to use the default named pipe", Control::Text),
    item(SSH, "ssh.clear-after-connect", "Clear after login", "Clear connection messages before the remote shell starts", Control::Toggle),
    item(SSH, "ssh.x11-display", "Local X11 display", "For X11 forwarding. Leave empty to use DISPLAY", Control::Text),
    item(SSH, "info.ssh-profiles", "Connection profiles", "Hosts, authentication, port forwards and algorithms", Control::ReadOnly(Info::SshProfiles)),
    item(SYSTEM, "action.whats-new", "What's new", "Read the notes for this version", Control::Run(&Action::WhatsNew)),
    item(SYSTEM, "action.check-for-updates", "Check for updates", "Look for a new version now", Control::Run(&Action::CheckForUpdates)),
    item(SYSTEM, "updates.check-automatically", "Check for updates automatically", "Look for a new version each time Aster starts", Control::Toggle),
    item(SYSTEM, "action.open-log", "Log", "Open the folder with the log file", Control::Run(&Action::OpenLog)),
    item(SYSTEM, "action.open-config-file", "Config file", "Open the config file in the system editor", Control::Run(&Action::OpenConfigFile)),
    item(SYSTEM, "appearance.language", "Language", "Language of menus, settings and hints; automatic follows the system", Control::Choice(LANGUAGES)),
    #[cfg(not(windows))]
    item(SYSTEM, "system.explorer-menu", "File manager menu", "Add \"Open in Aster\" to the context menu for folders", Control::Toggle),
    #[cfg(windows)]
    item(SYSTEM, "system.explorer-menu", "Explorer menu", "Add Aster commands to file and folder menus in Windows Explorer", Control::Toggle),
    item(SYSTEM, "system.autostart", "Start at sign-in", "Launch Aster automatically for your user account", Control::Toggle),
    item(SYSTEM, "action.install-launcher", "Add to the applications menu", "Create a launcher for Aster and its profiles", Control::Run(&Action::ApplicationsMenu(true))),
    item(SYSTEM, "action.remove-launcher", "Remove from the applications menu", "Remove the launcher and its icon", Control::Run(&Action::ApplicationsMenu(false))),
    #[cfg(windows)]
    item(SYSTEM, "system.url-scheme", "Aster links", "Allow aster:// links to open profiles and folders", Control::Toggle),
    #[cfg(windows)]
    item(SYSTEM, "action.start-menu-shortcut", "Start menu shortcut", "Show Aster in the Windows Start menu", Control::Run(&Action::StartMenuShortcut)),
    item(SYSTEM, "action.report-problem", "Report a problem", "Open a prefilled issue with the version and system", Control::Run(&Action::ReportProblem)),
    item(CONFIG_FILE, "info.config-path", "File path", "Location of the active configuration", Control::ReadOnly(Info::ConfigPath)),
    item(CONFIG_FILE, "action.edit-config", "Open in the editor", "Open the config file in the system editor", Control::Run(&Action::OpenConfigFile)),
    item(CONFIG_FILE, "action.open-config-folder", "Open the folder", "Open the folder containing the config file", Control::Run(&Action::OpenConfigFolder)),
    item(CONFIG_FILE, "info.config-status", "File status", "Check the configuration on disk", Control::ReadOnly(Info::ConfigStatus)),
    item(CONFIG_FILE, "action.show-default-config", "Show the defaults", "Open a copy of the default settings", Control::Run(&Action::ShowDefaultConfig)),
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
    if config.read_only() { return Err(config.read_only_message()); }
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

pub(crate) fn found(hay: &str, query: &str) -> bool {
    let hay = hay.to_lowercase();
    query.to_lowercase().split_whitespace().all(|w| hay.contains(w))
}

pub const SHORTCUTS: &str = "Keyboard shortcuts";
pub const PROFILES: &str = "Profiles";
pub const PAGES: [&str; 13] = [SYSTEM, APPEARANCE, PROFILES, TERMINAL, INPUT, COLORS, DOCK, SHORTCUTS, SEARCH, SSH, VAULT, WINDOW, CONFIG_FILE];
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileRow {
    pub group: Option<String>,
    pub badge: String,
    pub tint: u32,
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
    EntryMissing, EmptyLabel, DuplicateLabel, ExportFailed, ImportFailed, ConfigReadFailed,
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
            Self::EntryMissing => t("The secret no longer exists"),
            Self::EmptyLabel => t("Enter a label for the secret"),
            Self::DuplicateLabel => t("A key file already uses this label"),
            Self::ExportFailed => t("Could not create the export file"),
            Self::ImportFailed => t("Could not read the key file"),
            Self::ConfigReadFailed => t("Could not read the encrypted config"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultEntry {
    pub kind: String,
    pub key: String,
    pub label: String,
}

impl VaultEntry {
    pub(crate) fn kind_label(&self) -> &'static str {
        match self.kind.as_str() {
            "ssh-password" => t("SSH password"),
            "key-passphrase" => t("Key passphrase"),
            "key-file" => t("Key file"),
            _ => t("Other secret"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VaultView {
    pub configured: bool,
    pub unlocked: bool,
    pub entries: Vec<VaultEntry>,
    pub error: Option<VaultIssue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VaultRow {
    Create, Unlock, Manage, Import, Entry(usize),
}

impl VaultRow {
    fn text(self) -> (&'static str, &'static str) {
        match self {
            Self::Create => ("Set a master passphrase", "Encrypt new passwords in the config instead of using the system credential store"),
            Self::Unlock => ("Show vault contents", "The vault is locked"),
            Self::Manage => ("Manage vault", ""),
            Self::Import => ("Import key file", ""),
            Self::Entry(_) => ("", ""),
        }
    }

    pub(crate) fn labels(self, view: &VaultView) -> (&str, &str) {
        if let Self::Entry(i) = self {
            let entry = &view.entries[i];
            return (&entry.label, entry.kind_label());
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
            return [VaultRow::Unlock, VaultRow::Manage].get(index).copied();
        }
        match index {
            0 => Some(VaultRow::Manage),
            1 => Some(VaultRow::Import),
            _ => self.entries.get(index - 2).map(|_| VaultRow::Entry(index - 2)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum InlineEdit {
    Setting(String),
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
    args.iter().map(|arg| {
        if !arg.is_empty() && !arg.contains(['"', '\'']) && !arg.contains(char::is_whitespace) {
            arg.clone()
        } else if arg.contains('"') && !arg.contains('\'') {
            format!("'{arg}'")
        } else {
            format!("\"{}\"", arg.replace('"', "\"'\"'\""))
        }
    }).collect::<Vec<_>>().join(" ")
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

pub(crate) fn shortcut_edit_key(key: Key, mods: Mods) -> bool {
    matches!(key, Key::Enter | Key::Insert) && mods == Mods::NONE || key == Key::Delete && !mods.ctrl && !mods.meta && !mods.alt
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
            let aliases = if s.key == "picker.quick-connect" {
                let label = "Default quick connection";
                let hint = "Connection type used for addresses without a protocol";
                format!(" {label} {} {hint} {}", t(label), t(hint))
            } else {
                String::new()
            };
            found(&format!("{} {} {} {} {} {} {}{aliases}", s.section, t(s.section), s.label, t(s.label), s.hint, t(s.hint), s.key), query)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsPage {
    pub query: String,
    pub shown: Vec<usize>,
    pub selected: usize,
    pub scroll: f32,
    pub(crate) font_matches: Vec<String>,
    pub(crate) font_index: usize,
    pub(crate) font_replace: bool,
    pub(crate) segment_widths: std::collections::BTreeMap<&'static str, f32>,
    editing: Option<InlineEdit>,
    pub capture: Option<(Action, bool)>,
    pub(crate) profile_actions: Vec<crate::picker::ProfileAction>,
    pub(crate) shortcut_rows: std::collections::BTreeMap<usize, ShortcutLayout>,
    pub(crate) info_rows: std::collections::BTreeMap<usize, Vec<String>>,
    pub notice: Option<String>,
    pub profiles: Vec<ProfileRow>,
    pub(crate) vault: VaultView,
    launcher_installed: Option<bool>,
    pub(crate) vault_dialog: Option<VaultDialog>,
    vault_input_interrupted: bool,
    pub schemes: Vec<SchemeRow>,
    pub(crate) scheme_tab: SchemeTab,
    pub(crate) deleting_scheme: Option<String>,
    pub(crate) scheme_editor: Option<crate::scheme_editor::SchemeEditor>,
    pub(crate) editor: Option<ProfileEditor>,
    pub(crate) group_editor: Option<GroupEditor>,
    pub(crate) defaults_kind: Option<&'static str>,
    pub(crate) groups: Vec<crate::profile::ProfileGroup>,
    pub(crate) collapsed: BTreeSet<Header>,
    tree: Vec<TreeRow>,
    pub deleting: Option<String>,
    pub page: usize,
    pub(crate) profiles_advanced: bool,
    pub(crate) restart_pending: bool,
}

impl SettingsPage {
    pub(crate) fn colors_page(&self) -> bool {
        PAGES[self.page.min(PAGES.len() - 1)] == COLORS
    }

    pub fn new() -> Self {
        let mut page = Self::default();
        page.refilter();
        page
    }

    pub(crate) fn refilter(&mut self) {
        let colors_page = self.colors_page();
        let query = if colors_page { "" } else { &self.query };
        self.tree = profiles_page::tree(&self.groups, &self.profiles, &self.collapsed, query);
        let base = self.profile_base();
        let mut shown: Vec<_> = (0..SETTINGS.len()).filter(|&i| {
            let available = match SETTINGS[i].control {
                Control::Run(Action::ApplicationsMenu(install)) => self.launcher_installed == Some(!install),
                _ => true,
            };
            available && (self.vault.configured || SETTINGS[i].key != "vault.encrypt-config") && (!colors_page || self.query.trim().is_empty() && !SchemeTab::matches_setting(SETTINGS[i].key)) && item_matches(i, query)
        }).collect();
        let vault_rows = (0..).map_while(|k| self.vault.row(k).map(|row| (k, row))).filter_map(|(k, row)| {
            let (source, hint) = row.text();
            let (label, detail) = row.labels(&self.vault);
            found(&format!("{VAULT} {} {source} {hint} {label} {detail}", t(VAULT)), query).then_some(self.vault_base() + k)
        });
        let before_vault = shown.iter().position(|&i| SETTINGS[i].section == VAULT).unwrap_or(shown.len());
        shown.splice(before_vault..before_vault, vault_rows);
        let shortcuts = (SETTINGS.len()..base).filter(|&i| {
            if let Some(entry) = self.profile_action(i) {
                let action = &entry.action;
                let name = entry.name.as_deref();
                found(&format!("{SHORTCUTS} {} {PROFILES} {} {} {} {}", t(SHORTCUTS), t(PROFILES), action.source_label_for(name), action.label_for(name), action.id()), query)
            } else {
                item_matches(i, query)
            }
        });
        let scheme_base = self.scheme_base();
        let schemes = self.schemes.iter().enumerate().filter(|(_, row)| {
            self.query.is_empty() || found(&row.name, &self.query)
        }).map(|(k, _)| scheme_base + k);
        let after_colors = shown.iter().rposition(|&i| SETTINGS.get(i).is_some_and(|s| s.section == COLORS)).map_or(shown.len(), |n| n + 1);
        shown.splice(after_colors..after_colors, schemes);
        shown.extend((base..base + self.tree.len()).chain(shortcuts));
        self.deleting = None;
        self.shown = if self.searching_vault() && !self.query.trim().is_empty() {
            shown.into_iter().filter(|i| matches!(self.vault_at(*i), Some(VaultRow::Entry(_)))).collect()
        } else if colors_page || self.query.trim().is_empty() {
            let page = PAGES[self.page.min(PAGES.len() - 1)];
            shown.into_iter().filter(|&i| {
                if self.profile_action(i).is_some() { return page == SHORTCUTS; }
                if self.section(i) != page { return false; }
                if page != PROFILES { return true; }
                if let Some(row) = self.tree_at(i) {
                    !matches!(row.kind, TreeKind::NewGroup) && (matches!(row.kind, TreeKind::DefaultsHeader | TreeKind::Defaults(_)) == self.profiles_advanced)
                } else {
                    (SETTINGS[i].key != "terminal.profile") == self.profiles_advanced
                }
            }).collect()
        } else {
            shown
        };
        self.deleting_scheme = None;
        self.selected = 0;
        self.scroll = 0.0;
    }

    fn segment_width(&self, label: &'static str, scale: f32) -> f32 {
        self.segment_widths.get(label).copied().unwrap_or(64.0 * scale)
    }

    pub(crate) fn segments(&self, options: &[(&str, &'static str)], rect: Rect, scale: f32) -> Vec<Rect> {
        let total: f32 = options.iter().map(|(_, label)| self.segment_width(label, scale)).sum();
        let mut offset = 0.0;
        options.iter().map(|(_, label)| {
            let left = rect.x + (offset * rect.w / total).round();
            offset += self.segment_width(label, scale);
            let right = rect.x + (offset * rect.w / total).round();
            Rect::new(left, rect.y, right - left, rect.h)
        }).collect()
    }

    pub fn searching(&self) -> bool {
        !self.colors_page() && !self.query.trim().is_empty()
    }

    pub(crate) fn show_page(&mut self, page: usize) {
        self.page = page.min(PAGES.len() - 1);
        self.query.clear();
        self.font_matches.clear();
        self.font_replace = false;
        self.capture = None;
        self.editing = None;
        self.cancel_vault_input();
        self.notice = None;
        self.refilter();
    }

    pub fn current(&self) -> Option<&'static Setting> {
        self.shown.get(self.selected).and_then(|&i| SETTINGS.get(i))
    }

    pub fn current_shortcut(&self) -> Option<Action> {
        self.shown.get(self.selected).and_then(|&i| self.shortcut(i))
    }

    fn profile_action(&self, item: usize) -> Option<&crate::picker::ProfileAction> {
        self.profile_actions.get(item.checked_sub(SETTINGS.len() + Action::all().count())?)
    }

    pub(crate) fn shortcut(&self, item: usize) -> Option<Action> {
        shortcut(item).or_else(|| self.profile_action(item).map(|entry| entry.action.clone()))
    }

    pub(crate) fn shortcut_name(&self, item: usize) -> Option<&str> {
        self.profile_action(item)?.name.as_deref()
    }

    fn profile_base(&self) -> usize {
        SETTINGS.len() + Action::all().count() + self.profile_actions.len()
    }

    pub fn profile_at(&self, item: usize) -> Option<&ProfileRow> {
        match self.tree_at(item)?.kind {
            TreeKind::Profile(k) => self.profiles.get(k),
            _ => None,
        }
    }

    pub(crate) fn tree_at(&self, item: usize) -> Option<&TreeRow> {
        self.tree.get(item.checked_sub(self.profile_base())?)
    }

    pub fn is_new_profile(&self, item: usize) -> bool {
        self.tree_at(item).is_some_and(|r| r.kind == TreeKind::NewProfile)
    }

    fn scheme_base(&self) -> usize {
        self.profile_base() + self.tree.len()
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
        self.editing.is_some() || self.editing_passphrase()
    }

    pub(crate) fn editing_passphrase(&self) -> bool {
        self.vault_dialog.is_some() || self.vault_input_interrupted
    }

    pub(crate) fn edit_text(&self) -> String {
        self.editing.as_ref().map(|InlineEdit::Setting(text)| text.clone()).unwrap_or_default()
    }

    pub(crate) fn cancel_vault_input(&mut self) {
        self.vault_dialog = None;
        self.vault_input_interrupted = false;
    }

    pub(crate) fn searching_vault(&self) -> bool {
        PAGES[self.page] == VAULT && self.vault.unlocked
    }

    pub(crate) fn vault_notice(&self) -> Option<&str> {
        if self.vault_input_interrupted {
            return Some(t("Vault state changed. Press Esc to leave this field."));
        }
        self.notice.as_deref().or_else(|| self.vault.error.map(VaultIssue::message))
    }

    pub fn section(&self, item: usize) -> &'static str {
        if self.vault_at(item).is_some() {
            VAULT
        } else if self.scheme_at(item).is_some() {
            COLORS
        } else if self.tree_at(item).is_some() || self.profile_action(item).is_some() {
            PROFILES
        } else {
            section_of(item)
        }
    }

    pub fn current_item(&self) -> Option<usize> {
        self.shown.get(self.selected).copied()
    }

    fn step(&mut self, delta: isize) {
        if self.shown.is_empty() && !self.restart_pending {
            return;
        }
        let last = self.shown.len() as isize - 1 + isize::from(self.restart_pending);
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
        if self.current_item().and_then(|i| self.tree_at(i)).is_some_and(|r| r.kind == TreeKind::DefaultsHeader) {
            self.selected = (self.selected as isize + delta.signum()).clamp(0, last) as usize;
        }
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
    pub suggestions: Vec<(usize, Rect)>,
    pub restart: Option<Rect>,
    pub profile_tabs: Option<[Rect; 2]>,
    pub height: f32,
    pub(crate) scheme_bar: Option<SchemeBar>,
    pub(crate) font_sample: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Whole,
    Less,
    More,
}

pub fn control_width(control: Control) -> f32 {
    match control {
        Control::Segments(_) => 360.0,
        Control::Toggle => 40.0,
        Control::Number { .. } => 132.0,
        Control::Slider { .. } => 220.0,
        Control::Choice(_) | Control::Schemes | Control::Profiles => 220.0,
        Control::Text | Control::Color | Control::StringList | Control::Font { .. } => 260.0,
        Control::Run(_) => 120.0,
        Control::ReadOnly(_) => 0.0,
    }
}

pub(crate) fn column_widths(content: Rect, s: f32) -> (f32, f32, f32) {
    let total = (1080.0 * s).min(content.w - 32.0 * s).max(240.0 * s).round();
    let side_w = if total >= 640.0 * s { (200.0 * s).round() } else { 0.0 };
    let gap = if side_w > 0.0 { (24.0 * s).round() } else { 0.0 };
    (total, side_w, total - side_w - gap)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShortcutLayout {
    pub label_width: f32,
    pub chips: Vec<(Vec<String>, Rect)>,
    pub hint_top: f32,
    pub hint_lines: Vec<String>,
    pub height: f32,
}

impl ShortcutLayout {
    pub fn new(width: f32, s: f32, bindings: Vec<String>, hint: &str, mut measure: impl FnMut(&str) -> f32) -> Self {
        let pad = (14.0 * s).round();
        let inner = width - 2.0 * pad;
        let wrap = |text: &str, limit: f32, measure: &mut dyn FnMut(&str) -> f32| {
            let mut lines = Vec::new();
            let mut current = String::new();
            for word in text.split_whitespace() {
                if !current.is_empty() {
                    let next = format!("{current} {word}");
                    if measure(&next) <= limit {
                        current = next;
                        continue;
                    }
                    lines.push(std::mem::take(&mut current));
                }
                current.push_str(word);
            }
            if !current.is_empty() {
                lines.push(current);
            }
            lines
        };
        let stacked = inner < 420.0 * s;
        let label_width = if stacked { inner } else { (inner * 0.43).round() };
        let mut x = if stacked { pad } else { pad + label_width + 8.0 * s };
        let mut y = if stacked { 32.0 * s } else { 7.0 * s };
        let mut bottom = y;
        let mut chips = Vec::new();
        for text in bindings {
            let lines = if measure(&text) + 16.0 * s > inner { wrap(&text, inner - 16.0 * s, &mut measure) } else { vec![text] };
            let w = (lines.iter().map(|l| measure(l)).fold(0.0, f32::max) + 16.0 * s).ceil().min(inner);
            if x + w > width - pad && x > pad {
                x = pad;
                y = bottom.max(29.0 * s) + 4.0 * s;
            }
            let r = Rect::new(x, y, w, (6.0 + lines.len() as f32 * 16.0) * s);
            chips.push((lines, r));
            bottom = bottom.max(r.bottom());
            x = r.right() + 5.0 * s;
        }
        let hint_top = bottom + 4.0 * s;
        let hint_lines = wrap(hint, inner, &mut measure);
        let height = (hint_top + (7.0 + hint_lines.len() as f32 * 16.0) * s).ceil().max((58.0 * s).round());
        Self { label_width, chips, hint_top, hint_lines, height }
    }
}

pub fn geometry(content: Rect, s: f32, page: &SettingsPage) -> SettingsGeometry {
    let (total, side_w, width) = column_widths(content, s);
    let x0 = (content.x + (content.w - total) / 2.0).round();
    let x = x0 + total - width;
    let top = content.y + (18.0 * s).round();
    let header_h = (36.0 * s).round();
    let close_w = header_h;
    let close = Rect::new(x0 + total - close_w, top, close_w, header_h);
    let field_w = (300.0 * s).min(width * 0.5).round();
    let field = Rect::new(close.x - field_w - (8.0 * s).round(), top, field_w, header_h);
    let title = Rect::new(x0, top, (field.x - x0 - 8.0 * s).max(0.0), header_h);
    let list_top = top + header_h + (14.0 * s).round();
    let bottom = content.bottom() - 12.0 * s;
    let restart = (page.restart_pending && bottom >= list_top + 38.0 * s).then(|| {
        let w = (242.0 * s).min(width);
        Rect::new(x + width - w, bottom - 34.0 * s, w, 34.0 * s)
    });
    let profile_tabs = (!page.searching() && PAGES.get(page.page) == Some(&PROFILES)).then(|| {
        let w = (142.0 * s).min((width - 6.0 * s) * 0.5);
        [Rect::new(x, list_top, w, 30.0 * s), Rect::new(x + w + 6.0 * s, list_top, w, 30.0 * s)]
    });
    let scheme_bar = page.colors_page().then(|| SchemeBar::new(Rect::new(x, list_top, width, 0.0), s));
    let list_bottom = restart.map_or(bottom, |r| r.y - 8.0 * s);
    let sample_height = (84.0 * s).min((list_bottom - list_top).max(0.0) * 0.35).round();
    let font_sample = (!page.searching() && PAGES[page.page.min(PAGES.len() - 1)] == APPEARANCE)
        .then(|| Rect::new(x, list_top, width, sample_height));
    let rows_top = match (scheme_bar, font_sample, profile_tabs) {
        (Some(bar), _, _) => bar.current.bottom() + (6.0 * s).round(),
        (_, Some(sample), _) => sample.bottom() + 6.0 * s,
        (_, _, Some(tabs)) => tabs[0].bottom() + 4.0 * s,
        _ => list_top,
    };
    let list = Rect::new(x, rows_top, width, (list_bottom - rows_top).max(0.0));
    let page_h = (34.0 * s).min((bottom - list_top).max(0.0) / PAGES.len() as f32).floor().max(1.0);
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
            if section != PROFILES || profile_tabs.is_none() {
                let section_h = section_h + if section == SYSTEM && !page.searching() { (20.0 * s).round() } else { 0.0 };
                rows.push(Row { kind: RowKind::Section(section), rect: Rect::new(x, y, width, section_h), control: Rect::default() });
                y += section_h;
            }
        }
        let item_h = match page.tree_at(i).map(|r| &r.kind) {
            Some(TreeKind::Profile(_)) => (50.0 * s).round(),
            Some(_) => (38.0 * s).round(),
            None => page.shortcut_rows.get(&i).map_or(item_h, |r| r.height) + page.info_rows.get(&i).map_or(0, |lines| lines.len().saturating_sub(1)) as f32 * (16.0 * s).round(),
        };
        let rect = Rect::new(x, y, width, item_h);
        let (cw, ch) = match SETTINGS.get(i).map(|s| s.control) {
            Some(Control::Toggle) => ((control_width(Control::Toggle) * s).round(), (22.0 * s).round()),
            Some(Control::Segments(options)) => {
                let measured: f32 = options.iter().map(|(_, label)| page.segment_width(label, s)).sum();
                (measured.min(360.0 * s).min(width * 0.55).round(), control_h)
            }
            Some(c) => ((control_width(c) * s).min(width * 0.45).round(), control_h),
            None if page.scheme_at(i).is_some() => ((300.0 * s).min(width * 0.45).round(), (36.0 * s).round()),
            None => ((240.0 * s).min(width * 0.45).round(), control_h),
        };
        let control = Rect::new(rect.right() - cw - (14.0 * s).round(), rect.y + ((item_h - ch) / 2.0).round(), cw, ch);
        rows.push(Row { kind: RowKind::Item(n), rect, control });
        y += item_h;
    }
    let height = y + page.scroll - list.y;
    let mut suggestions = Vec::new();
    if page.editing() && matches!(page.current().map(|s| s.control), Some(Control::Font { .. })) {
        if let Some(row) = rows.iter().find(|r| r.kind == RowKind::Item(page.selected)) {
            let anchor = row.control;
            if anchor.y >= list.y && anchor.bottom() <= list.bottom() {
                let h = (26.0 * s).round();
                let below = ((list.bottom() - anchor.bottom()) / h) as usize;
                let above = ((anchor.y - list.y) / h) as usize;
                let count = page.font_matches.len();
                let flip = below < count && above > below;
                let visible = count.min(if flip { above } else { below });
                let first = (page.font_index + 1).saturating_sub(visible);
                let top = if flip { anchor.y - visible as f32 * h } else { anchor.bottom() };
                suggestions.extend((0..visible).map(|i| (first + i, Rect::new(anchor.x, top + i as f32 * h, anchor.w, h))));
            }
        }
    }
    SettingsGeometry { page: content, title, field, close, list, rows, pages, suggestions, height, scheme_bar, font_sample, restart, profile_tabs }
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
        if self.vault == view {
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
            if page.vault.unlocked && !view.unlocked {
                let interrupted = page.vault_dialog.as_ref().is_some_and(|dialog| dialog.kind.sensitive() && !dialog.waiting);
                page.cancel_vault_input();
                page.vault_input_interrupted = interrupted;
                if PAGES[page.page] == VAULT {
                    page.query.clear();
                }
            }
            if old_row.is_some() {
                page.notice = None;
            }
            page.vault = view.clone();
            page.refilter();
            let at = page.shown.iter().position(|&item| match old_row {
                Some(VaultRow::Entry(_)) => matches!(page.vault_at(item), Some(VaultRow::Entry(k)) if old_entry.as_ref().is_some_and(|old| page.vault.entries[k].key == old.key && page.vault.entries[k].kind == old.kind)),
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
            if let Some(id) = select {
                profiles_page::reveal_profile(id, &page.groups, &page.profiles, &mut page.collapsed);
                self.profile_collapsed.clone_from(&page.collapsed);
            }
            page.editor = None;
            page.group_editor = None;
            page.defaults_kind = None;
            page.capture = None;
            page.deleting = None;
            page.profiles_advanced = false;
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
        let mut page = SettingsPage { profiles: self.profile_rows(), profile_actions: self.profile_actions(), groups: self.config.groups.clone(), collapsed: self.profile_collapsed.clone(), schemes: self.scheme_rows(), vault: self.vault.clone(), launcher_installed: self.launcher_installed, restart_pending: self.restart_pending(), ..SettingsPage::default() };
        page.refilter();
        page
    }

    fn show_settings_page(&mut self, k: usize) {
        let tab = self.active_scheme_tab();
        self.config_editor.focus = None;
        self.config_editor.cancel_paste();
        let Some(page) = self.settings.as_mut() else { return };
        page.show_page(k);
        if page.colors_page() {
            self.select_scheme_tab(tab);
        }
        self.reveal_selected();
    }

    fn show_profiles_tab(&mut self, advanced: bool) {
        self.commit_edit();
        if let Some(page) = self.settings.as_mut() {
            page.profiles_advanced = advanced;
            page.refilter();
        }
        self.dirty = true;
    }

    pub(crate) fn restore_settings_page(&mut self) {
        self.settings = Some(self.settings_stash.take().unwrap_or_else(|| self.fresh_settings_page()));
        self.effects.push(Effect::CheckConfigFile);
    }

    pub fn set_config_path(&mut self, path: Option<String>) {
        self.config_path = path;
        self.dirty = true;
    }

    pub fn set_start_menu_shortcut(&mut self, exists: bool) {
        if self.start_menu_shortcut != exists {
            self.start_menu_shortcut = exists;
            self.dirty = true;
        }
    }

    pub fn start_menu_shortcut_error(&mut self, error: &str) {
        if let Some(page) = self.settings.as_mut() {
            page.notice = Some(tf("Could not update the shortcut: {error}", &[("error", error)]));
            self.dirty = true;
        }
    }

    pub fn set_config_encrypted(&mut self, encrypted: bool) {
        self.config_encrypted = encrypted;
        self.dirty = true;
    }

    pub fn set_config_status(&mut self, error: Option<String>) {
        self.config_status = Some(error.map_or(Ok(()), Err));
        self.dirty = true;
    }

    pub fn set_launcher_installed(&mut self, installed: bool) {
        if self.launcher_installed == Some(installed) {
            return;
        }
        self.launcher_installed = Some(installed);
        for page in self.settings.iter_mut().chain(self.settings_stash.iter_mut()) {
            let (selected, scroll) = (page.selected, page.scroll);
            page.launcher_installed = self.launcher_installed;
            page.refilter();
            page.selected = selected.min(page.shown.len().saturating_sub(1));
            page.scroll = scroll;
        }
        self.reveal_selected();
        self.dirty = true;
    }

    pub fn launcher_error(&mut self, error: &str) {
        if let Some(page) = self.settings.as_mut() {
            page.notice = Some(tf("Could not update the applications menu: {error}", &[("error", error)]));
            self.dirty = true;
        }
    }

    pub fn settings_file_error(&mut self, error: &str) {
        if let Some(page) = self.settings.as_mut() {
            page.notice = Some(tf("Could not open the defaults: {error}", &[("error", error)]));
            self.dirty = true;
        }
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
            self.restore_settings_page();
        }
        self.dirty = true;
    }

    pub(crate) fn custom_profile_id(&self, name: &str) -> String {
        let base = format!("custom:{}", slug(name));
        if self.profiles.iter().any(|p| p.id == base) {
            (2..).map(|n| format!("{base}-{n}")).find(|id| self.profiles.iter().all(|p| &p.id != id)).unwrap_or(base)
        } else {
            base
        }
    }

    pub(crate) fn close_settings(&mut self) {
        self.config_editor.cancel_paste();
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
            Control::Choice(list) | Control::Segments(list) => list.iter().map(|(v, l)| ((*v).to_owned(), t(l).to_owned())).collect(),
            Control::Schemes => self.scheme_names().into_iter().map(|n| (n.clone(), n)).collect(),
            Control::Profiles => {
                let fallback;
                let rows = if let Some(page) = &self.settings { &page.profiles } else { fallback = self.profile_rows(); &fallback };
                std::iter::once((String::new(), t("First available").into())).chain(rows.iter().map(|p| (p.id.clone(), p.name.clone()))).collect()
            }
            _ => Vec::new(),
        }
    }

    pub fn setting_text(&self, setting: &Setting) -> String {
        match setting.control {
            Control::ReadOnly(Info::ConfigPath) => return self.config_path.as_deref().unwrap_or_else(|| t("Config folder unavailable")).to_owned(),
            Control::ReadOnly(Info::ConfigStatus) if self.config_encrypted => return t("Encrypted").to_owned(),
            Control::ReadOnly(Info::ConfigStatus) => return match &self.config_status {
                Some(Ok(())) => t("The config file is valid"),
                Some(Err(error)) => error,
                None if self.config_path.is_none() => t("Config folder unavailable"),
                None => t("Checking the config file…"),
            }.to_owned(),
            Control::ReadOnly(Info::SshProfiles) => return t("Configure individual connections in Profiles and connections.").into(),
            _ => {}
        }
        let value = get(&self.config, setting.key);
        match (setting.control, &value) {
            (Control::Number { .. }, Some(Value::Integer(n))) => n.to_string(),
            (Control::Number { .. }, Some(Value::Float(f))) => number_text(*f),
            (Control::Choice(_) | Control::Segments(_) | Control::Schemes | Control::Profiles, Some(Value::String(v))) => {
                self.setting_options(setting.control).into_iter().find(|(o, _)| o.eq_ignore_ascii_case(v)).map(|(_, l)| l).unwrap_or_else(|| v.clone())
            }
            (Control::StringList, Some(Value::Array(items))) => items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
            (_, Some(Value::String(v))) => v.clone(),
            (_, Some(Value::Boolean(b))) => b.to_string(),
            _ => String::new(),
        }
    }

    pub fn setting_on(&self, setting: &Setting) -> bool {
        matches!(get(&self.config, setting.key), Some(Value::Boolean(true)))
    }

    pub fn set_restart_baseline(&mut self, baseline: RestartSettings) {
        self.restart_baseline = baseline;
        self.update_restart_pending();
    }

    fn restart_pending(&self) -> bool {
        !self.config.read_only() && self.restart_baseline != RestartSettings::from_config(&self.config)
    }

    pub(crate) fn update_restart_pending(&mut self) {
        let pending = self.restart_pending();
        for page in [&mut self.settings, &mut self.settings_stash].into_iter().flatten() {
            page.restart_pending = pending;
            page.selected = page.selected.min(page.shown.len().saturating_sub(usize::from(!pending)));
        }
    }

    pub fn restart_failed(&mut self, error: &str) {
        self.toast(tf("Could not restart Aster: {error}", &[("error", error)]), std::time::Duration::from_secs(6));
    }

    pub(crate) fn slider_value(&self, setting: &Setting, fallback: f64) -> f64 {
        if let Drag::SettingsSlider { item, track, x } = self.drag {
            if SETTINGS.get(item) == Some(setting) {
                return Self::slider_position(setting, track, x);
            }
        }
        match get(&self.config, setting.key) {
            Some(Value::Float(value)) if value.is_finite() => value,
            Some(Value::Integer(value)) => value as f64,
            _ => fallback,
        }
    }

    fn slider_position(setting: &Setting, track: Rect, x: f32) -> f64 {
        let (min, max, step) = setting.control.number().unwrap();
        let ratio = ((x - track.x) / track.w).clamp(0.0, 1.0) as f64;
        ((min + (max - min) * ratio) / step).round() * step
    }

    pub(crate) fn finish_settings_slider(&mut self, item: usize, track: Rect, x: f32) {
        let setting = &SETTINGS[item];
        self.set_number(setting, Self::slider_position(setting, track, x));
    }

    pub fn shortcut_text(&self, action: Action) -> String {
        let all: Vec<String> = self.keymap.bindings_of(&action).map(format_binding).collect();
        if all.is_empty() {
            t("None").to_owned()
        } else {
            all.join(", ")
        }
    }

    fn set_shortcuts(&mut self, action: Action, bindings: Option<Vec<String>>) {
        if self.config.read_only() { return self.warn_read_only_config(); }
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

    fn assign_shortcut(&mut self, action: Action, chord: Chord, append: bool) {
        if self.config.read_only() {
            if let Some(page) = self.settings.as_mut() { page.capture = None; }
            return self.warn_read_only_config();
        }
        let text = chord.to_string();
        let mut taken = Vec::new();
        let mut config = self.config.clone();
        for other in self.keymap.actions_for_chord(chord).filter(|o| **o != action).collect::<BTreeSet<_>>() {
            let current: Vec<String> = self.keymap.bindings_of(other).map(format_binding).collect();
            if current.contains(&text) {
                config.hotkeys.insert(other.id(), current.into_iter().filter(|b| *b != text).collect());
                taken.push(other.label());
            }
        }
        let mut bindings: Vec<String> = if append { self.keymap.bindings_of(&action).map(format_binding).collect() } else { Vec::new() };
        if !bindings.contains(&text) {
            bindings.push(text.clone());
            config.hotkeys.insert(action.id(), bindings);
        }
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
        let Some((action, append)) = self.settings.as_ref().and_then(|p| p.capture.clone()) else { return };
        match key {
            Key::Escape if mods == Mods::NONE => {
                if let Some(p) = self.settings.as_mut() {
                    p.capture = None;
                }
            }
            Key::Other => {}
            _ => self.assign_shortcut(action, Chord { key, mods }, append),
        }
        self.dirty = true;
    }

    fn apply_setting(&mut self, key: &str, value: Value) {
        if self.config.read_only() { return self.warn_read_only_config(); }
        if let Ok(config) = set(&self.config, key, value) {
            if config != self.config {
                self.set_config(config);
                self.push_save_config();
            }
        }
        self.dirty = true;
    }

    fn adjust_setting(&mut self, setting: &Setting, delta: i32) {
        if setting.key == "vault.encrypt-config" {
            return self.open_vault_dialog(DialogKind::Protect(!self.config.vault.encrypt_config));
        }
        match setting.control {
            Control::Toggle => {
                let on = if delta == 0 { !self.setting_on(setting) } else { delta > 0 };
                self.apply_setting(setting.key, Value::Boolean(on));
            }
            Control::Choice(_) | Control::Segments(_) | Control::Schemes | Control::Profiles => {
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
            Control::Number { step, .. } | Control::Slider { step, .. } => {
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
            Control::Text | Control::Color | Control::StringList | Control::Font { .. } => self.begin_edit(),
            Control::Run(action) => self.run(action.clone()),
            Control::ReadOnly(_) => {}
        }
    }

    fn set_number(&mut self, setting: &Setting, wanted: f64) {
        let Some((min, max, step)) = setting.control.number() else { return };
        if !wanted.is_finite() { return; }
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
            Control::Number { .. } | Control::Text | Control::Color | Control::StringList | Control::Font { .. } => self.setting_text(setting),
            Control::Slider { min, .. } => number_text(self.slider_value(setting, min)),
            _ => return,
        };
        if let Some(p) = self.settings.as_mut() {
            p.editing = Some(InlineEdit::Setting(text));
            p.font_replace = matches!(setting.control, Control::Font { .. });
            p.font_matches.clear();
            p.font_index = 0;
        }
        self.dirty = true;
    }

    pub fn set_font_families(&mut self, monospace: Vec<String>, all: Vec<String>) {
        self.monospace_fonts = monospace;
        self.all_fonts = all;
        self.filter_fonts();
        self.dirty = true;
    }

    fn filter_fonts(&mut self) {
        let Some(page) = self.settings.as_mut() else { return };
        let Some(Control::Font { monospace }) = page.current().map(|s| s.control) else { return };
        let Some(InlineEdit::Setting(text)) = &page.editing else { return };
        page.font_matches.clear();
        page.font_index = 0;
        if !page.font_replace && !text.is_empty() {
            let query = text.to_lowercase();
            let families = if monospace { &self.monospace_fonts } else { &self.all_fonts };
            page.font_matches.extend(families.iter().filter(|f| f.to_lowercase().contains(&query)).take(8).cloned());
        }
    }

    fn accept_font(&mut self, index: usize) {
        if let Some(page) = self.settings.as_mut() {
            if let Some(name) = page.font_matches.get(index) {
                page.editing = Some(InlineEdit::Setting(name.clone()));
            }
        }
        self.commit_edit();
    }

    fn commit_edit(&mut self) {
        let Some(page) = self.settings.as_mut() else { return };
        page.font_matches.clear();
        page.font_replace = false;
        let Some(InlineEdit::Setting(text)) = page.editing.take() else { return };
        let Some(setting) = page.current() else { return };
        match setting.control {
            Control::Text | Control::Font { .. } => self.apply_setting(setting.key, Value::String(text.trim().to_owned())),
            Control::StringList => {
                let values = text.split(',').map(str::trim).filter(|value| !value.is_empty()).map(|value| Value::String(value.to_owned())).collect();
                self.apply_setting(setting.key, Value::Array(values));
            }
            Control::Color => {
                let text = text.trim();
                if text.is_empty() || color_value(text).is_some() {
                    self.apply_setting(setting.key, Value::String(text.to_owned()));
                } else if let Some(page) = self.settings.as_mut() {
                    page.notice = Some(t("Enter a color as #RRGGBB").into());
                }
            }
            Control::Number { .. } | Control::Slider { .. } => {
                if let Ok(v) = text.trim().replace(',', ".").parse::<f64>() {
                    self.set_number(setting, v);
                }
            }
            _ => {}
        }
        self.dirty = true;
    }

    fn activate_vault_row(&mut self, row: VaultRow) {
        let Some(page) = self.settings.as_ref() else { return };
        let kind = match row {
            VaultRow::Create => DialogKind::Create,
            VaultRow::Unlock => DialogKind::Unlock(AfterUnlock::Close),
            VaultRow::Manage => DialogKind::Manage,
            VaultRow::Import => DialogKind::Import,
            VaultRow::Entry(i) => DialogKind::Entry(page.vault.entries[i].clone()),
        };
        self.open_vault_dialog(kind);
    }

    pub(crate) fn reveal_selected(&mut self) {
        let Some(geo) = self.settings_geometry() else { return };
        let Some(page) = self.settings.as_mut() else { return };
        let Some(row) = geo.rows.iter().find(|r| r.kind == RowKind::Item(page.selected)) else { return };
        let header = geo.rows.iter().position(|r| r.kind == RowKind::Item(page.selected)).filter(|&i| i > 0 && matches!(geo.rows[i - 1].kind, RowKind::Section(_))).map(|i| geo.rows[i - 1].rect.h).unwrap_or(0.0);
        let top = row.rect.y - header;
        let old_scroll = page.scroll;
        if top < geo.list.y {
            page.scroll -= geo.list.y - top;
        } else if row.rect.bottom() > geo.list.bottom() {
            page.scroll += row.rect.bottom() - geo.list.bottom();
        }
        if page.colors_page() && page.scroll > old_scroll {
            let list_top = geo.list.y + page.scroll - old_scroll;
            if let Some(partial) = geo.rows.iter().find(|r| r.rect.y < list_top && list_top < r.rect.bottom() && r.rect.bottom() <= row.rect.y) {
                page.scroll += partial.rect.bottom() - list_top;
            }
        }
        page.scroll = page.scroll.clamp(0.0, (geo.height - geo.list.h).max(0.0));
    }

    pub(crate) fn settings_key(&mut self, key: Key, mods: Mods) {
        if self.config_editor_key(key, mods) {
            self.dirty = true;
            return;
        }
        let Some(page) = self.settings.as_mut() else { return };
        self.dirty = true;
        if page.capture.is_some() {
            return self.capture_key(key, mods);
        }
        if page.vault_input_interrupted {
            if mods == Mods::NONE && matches!(key, Key::Escape | Key::Tab | Key::Enter) {
                page.cancel_vault_input();
            }
            return;
        }
        if page.vault_dialog.is_some() {
            return self.vault_dialog_key(key, mods);
        }
        if page.editor.is_some() || page.group_editor.is_some() {
            return self.profile_modal_key(key, mods);
        }
        if page.scheme_editor.is_some() {
            return self.scheme_editor_key(key, mods);
        }
        if self.scheme_bar_key(key, mods) {
            return;
        }
        if key == Key::F(2) && mods == Mods::NONE && self.open_scheme_editor() {
            return;
        }
        let Some(page) = self.settings.as_mut() else { return };
        match (key, page.current_item().filter(|_| !page.editing()).and_then(|item| page.vault_at(item))) {
            (Key::Delete, Some(VaultRow::Entry(i))) => {
                let entry = page.vault.entries[i].clone();
                return self.open_vault_dialog(DialogKind::Delete(entry));
            }
            (Key::Enter, Some(row)) => return self.activate_vault_row(row),
            _ => {}
        }
        if !page.editing() && page.current_item().and_then(|i| page.tree_at(i)).is_some() && self.profile_tree_key(key, mods) {
            return;
        }
        let Some(page) = self.settings.as_mut() else { return };
        if key != Key::Other {
            page.notice = None;
        }
        if page.editing.is_some() {
            match key {
                Key::Escape => {
                    page.editing = None;
                    page.font_matches.clear();
                    page.font_replace = false;
                }
                Key::Up | Key::Down if !page.font_matches.is_empty() => {
                    page.font_index = if key == Key::Up { page.font_index.saturating_sub(1) } else { (page.font_index + 1).min(page.font_matches.len() - 1) };
                }
                Key::Enter if !page.font_matches.is_empty() => {
                    let index = page.font_index;
                    self.accept_font(index);
                }
                Key::Enter | Key::Tab => self.commit_edit(),
                Key::Backspace => {
                    let replace = std::mem::take(&mut page.font_replace);
                    if let Some(InlineEdit::Setting(t)) = page.editing.as_mut() {
                        if mods.ctrl || replace {
                            t.clear();
                        } else {
                            t.pop();
                        }
                    }
                    self.filter_fonts();
                }
                Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
                _ => {}
            }
            return;
        }
        let current = page.current();
        if let Some(action) = page.current_shortcut().filter(|_| shortcut_edit_key(key, mods)) {
            match key {
                Key::Enter | Key::Insert => {
                    page.capture = Some((action, key == Key::Insert));
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
                if page.restart_pending && page.selected == page.shown.len() {
                    self.push(Effect::Restart);
                } else if let Some(s) = current {
                    self.adjust_setting(s, 0);
                } else if let Some(scheme) = page.current_item().and_then(|i| page.scheme_at(i)) {
                    let name = scheme.name.clone();
                    self.apply_settings_scheme(name);
                }
            }
            Key::Left | Key::Right if mods.ctrl && !page.searching() && PAGES[page.page] == PROFILES => {
                return self.show_profiles_tab(key == Key::Right);
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
        if self.vault_dialog_text(text) { return; }
        let Some(page) = self.settings.as_ref() else { return };
        if page.vault_input_interrupted || page.deleting_scheme.is_some() { return; }
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        if self.config_editor_text(&clean) { return; }
        let Some(page) = self.settings.as_mut() else { return };
        if let Some(editor) = page.scheme_editor.as_mut() {
            editor.insert(&clean);
            self.dirty = true;
            return;
        }
        if let Some(editor) = page.editor.as_mut() {
            editor.insert(&clean);
            self.dirty = true;
            return;
        }
        if let Some(group) = page.group_editor.as_mut() {
            if matches!(group.focus, 0 | 2 | 3) {
                group.fields[group.focus].push_str(&clean);
                group.error.clear();
            }
            self.dirty = true;
            return;
        }
        match page.editing.as_mut() {
            Some(InlineEdit::Setting(buffer)) => {
                if std::mem::take(&mut page.font_replace) { buffer.clear(); }
                buffer.push_str(&clean);
                self.filter_fonts();
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
        if self.vault_dialog_click(geo.page, x, y) || self.profile_modal_click(geo.page, x, y) || self.scheme_editor_click(geo.page, x, y) { return; }
        if geo.scheme_bar.is_some_and(|bar| self.scheme_edit_click(bar, x, y)) { return; }
        if geo.scheme_bar.is_some_and(|bar| self.scheme_bar_click(bar, x, y)) { return; }
        if let Some(page) = self.settings.as_mut() {
            page.capture = None;
            page.vault_input_interrupted = false;
        }
        if let Some(&(index, _)) = geo.suggestions.iter().find(|(_, r)| r.contains(x, y)) {
            return self.accept_font(index);
        }
        if geo.close.contains(x, y) {
            return self.close_settings();
        }
        if geo.field.contains(x, y) {
            self.config_editor.focus = None;
            self.config_editor.cancel_paste();
            return self.commit_edit();
        }
        if geo.restart.is_some_and(|r| r.contains(x, y)) {
            self.commit_edit();
            if self.restart_pending() { self.push(Effect::Restart); }
            return;
        }
        if let Some(k) = geo.pages.iter().position(|r| r.contains(x, y)) {
            self.commit_edit();
            return self.show_settings_page(k);
        }
        if let Some(k) = geo.profile_tabs.and_then(|tabs| tabs.iter().position(|r| r.contains(x, y))) {
            return self.show_profiles_tab(k == 1);
        }
        if !geo.list.contains(x, y) {
            return self.commit_edit();
        }
        if self.config_editor_click(x, y) { return; }
        let Some(row) = geo.rows.iter().find(|r| r.rect.contains(x, y)).copied() else { return self.commit_edit() };
        let RowKind::Item(n) = row.kind else { return self.commit_edit() };
        let scale = self.ui();
        let editing_same = self.settings.as_ref().is_some_and(|p| p.selected == n && p.editing.is_some());
        let control = self.settings.as_ref().and_then(|p| SETTINGS.get(p.shown[n])).map(|s| s.control);
        if editing_same && row.control.contains(x, y) && control.is_some_and(|c| part_at(c, row.control, self.ui(), x) == Part::Whole) {
            return;
        }
        self.commit_edit();
        let Some(page) = self.settings.as_mut() else { return };
        page.selected = n;
        page.notice = None;
        let item = page.shown[n];
        if let Some(row) = page.vault_at(item) {
            return self.activate_vault_row(row);
        }
        if let Some(scheme) = page.scheme_at(item) {
            let name = scheme.name.clone();
            return self.apply_settings_scheme(name);
        }
        if page.tree_at(item).is_some() {
            page.deleting = None;
            return self.profile_tree_click(item, row.rect, x, y);
        }
        if let Some(action) = page.shortcut(page.shown[n]) {
            page.capture = Some((action, false));
            return;
        }
        let setting = &SETTINGS[page.shown[n]];
        if matches!(setting.control, Control::Slider { .. }) && row.control.contains(x, y) {
            self.drag = Drag::SettingsSlider { item: page.shown[n], track: slider_track(row.control, scale), x };
            return;
        }
        if let Control::Segments(options) = setting.control {
            let segments = page.segments(options, row.control, scale);
            if let Some(index) = segments.iter().position(|rect| rect.contains(x, y)) {
                self.apply_setting(setting.key, Value::String(options[index].0.to_owned()));
            }
            return;
        }
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
        if self.vault_dialog_wheel(dy) || self.config_editor_wheel(dy) || self.profile_modal_wheel(dy) { return; }
        let Some(geo) = self.settings_geometry() else { return };
        if self.scheme_editor_wheel(geo.page, dy) { return; }
        let step = 60.0 * self.ui();
        let Some(page) = self.settings.as_mut() else { return };
        page.scroll = (page.scroll - dy * step).clamp(0.0, (geo.height - geo.list.h).max(0.0));
        self.dirty = true;
    }
}

pub fn explorer_label() -> &'static str {
    t("Open in Aster")
}

pub fn explorer_paste_label() -> &'static str {
    t("Paste path")
}

pub(crate) const TRAY_MENU: [(&str, Action); 2] = [("New window", Action::NewWindow), ("Quit", Action::Quit)];

pub fn restart_unsaved() -> &'static str {
    t("Save the settings before restarting")
}

pub fn tray_menu_labels() -> [&'static str; 2] {
    TRAY_MENU.map(|(label, _)| t(label))
}

#[cfg(test)]
#[path = "../test/settings.rs"]
mod tests;
