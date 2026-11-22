use crate::profile::{Profile, ProfileDefaults, ProfileGroup};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub use aster_proto::ssh::WindowsAgent;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Beam,
    Underline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RightClick {
    Off,
    Menu,
    Paste,
    #[default]
    Clipboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LinkModifier {
    #[default]
    None,
    Ctrl,
    Shift,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Bell {
    Off,
    #[default]
    Visual,
    Audible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Identification {
    #[default]
    Aster,
    Xterm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SchemeMode {
    #[default]
    Dark,
    Light,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SshClient {
    #[default]
    Native,
    System,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Ssh {
    pub client: SshClient,
    pub share_connections: bool,
    pub remember_passwords: bool,
    pub forward_agent: bool,
    pub warn_on_close: bool,
    pub verify_host_keys: bool,
    pub windows_agent: WindowsAgent,
    pub agent_pipe: String,
    pub clear_after_connect: bool,
    pub x11_display: String,
}

impl Default for Ssh {
    fn default() -> Self {
        Self { client: SshClient::Native, share_connections: true, remember_passwords: true, forward_agent: false, warn_on_close: true,
            verify_host_keys: true, windows_agent: WindowsAgent::Auto, agent_pipe: String::new(), clear_after_connect: true, x11_display: String::new() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    #[default]
    None,
    Acrylic,
    Mica,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Dock {
    #[default]
    Off,
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DockScreen {
    #[default]
    Current,
    Primary,
    Number(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct DockSettings {
    #[serde(deserialize_with = "read_dock_screen")]
    pub screen: String,
}

fn read_dock_screen<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Number(u32),
        Text(String),
    }
    Ok(match Value::deserialize(deserializer)? {
        Value::Number(number) => number.to_string(),
        Value::Text(text) => text,
    })
}

impl Default for DockSettings {
    fn default() -> Self {
        Self { screen: "current".into() }
    }
}

impl DockSettings {
    pub fn selection(&self) -> DockScreen {
        let value = self.screen.trim();
        if value.eq_ignore_ascii_case("primary") {
            DockScreen::Primary
        } else if let Ok(number) = value.parse::<std::num::NonZeroU32>() {
            DockScreen::Number(number.get())
        } else {
            DockScreen::Current
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SearchOptions {
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Terminal {
    pub font: String,
    pub font_weight: u16,
    pub font_weight_bold: u16,
    pub fallback_font: String,
    pub ligatures: bool,
    pub minimum_contrast_ratio: f32,
    pub font_size: f32,
    pub line_padding: f32,
    pub padding: f32,
    pub scrollback_lines: usize,
    pub cursor: CursorStyle,
    pub cursor_blink: bool,
    pub alt_is_meta: bool,
    pub copy_on_select: bool,
    pub copy_as_html: bool,
    pub right_click: RightClick,
    pub link_modifier: LinkModifier,
    pub paste_on_middle_click: bool,
    pub bracketed_paste: bool,
    pub trim_whitespace_on_paste: bool,
    pub paste_newlines_as_spaces: bool,
    pub warn_on_multiline_paste: bool,
    pub sudo_passwords: bool,
    pub bell: Bell,
    pub bell_attention: bool,
    pub draw_bold_text_in_bright_colors: bool,
    pub word_separator: String,
    pub scroll_on_input: bool,
    pub profile: String,
    #[serde(rename = "env")]
    pub environment: BTreeMap<String, String>,
    pub refresh_environment: bool,
    pub set_comspec: bool,
    pub identification: Identification,
    pub search_options: SearchOptions,
    pub recover_tabs: bool,
    pub persistent_sessions: bool,
    pub open_on_start: bool,
    pub confirm_close: bool,
    pub close_window_with_last_tab: bool,
    pub recent_profiles: usize,
}

impl Default for Terminal {
    fn default() -> Self {
        let mut terminal = Self {
            font: "monospace".into(),
            font_weight: 400,
            font_weight_bold: 700,
            fallback_font: String::new(),
            ligatures: false,
            minimum_contrast_ratio: 1.0,
            font_size: 14.0,
            line_padding: 0.0,
            padding: 6.0,
            scrollback_lines: 25_000,
            cursor: CursorStyle::Block,
            cursor_blink: true,
            alt_is_meta: true,
            copy_on_select: true,
            copy_as_html: false,
            right_click: RightClick::Clipboard,
            link_modifier: LinkModifier::None,
            paste_on_middle_click: false,
            bracketed_paste: true,
            trim_whitespace_on_paste: true,
            paste_newlines_as_spaces: false,
            warn_on_multiline_paste: true,
            sudo_passwords: true,
            bell: Bell::Visual,
            bell_attention: true,
            draw_bold_text_in_bright_colors: true,
            word_separator: " ()[]{}'\"".into(),
            scroll_on_input: true,
            profile: String::new(),
            environment: BTreeMap::new(),
            refresh_environment: true,
            set_comspec: false,
            identification: Identification::Aster,
            search_options: SearchOptions::default(),
            recover_tabs: true,
            persistent_sessions: false,
            open_on_start: true,
            confirm_close: true,
            close_window_with_last_tab: true,
            recent_profiles: 3,
        };
        crate::config_defaults::apply_os_defaults(&mut terminal);
        terminal
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Panes {
    pub resize_step: f32,
    pub focus_follows_mouse: bool,
}

impl Default for Panes {
    fn default() -> Self {
        Self { resize_step: 0.1, focus_follows_mouse: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Notifications {
    pub command_finished: bool,
    pub activity: bool,
    pub activity_silence_seconds: u32,
}

impl Default for Notifications {
    fn default() -> Self {
        Self { command_finished: false, activity: false, activity_silence_seconds: 10 }
    }
}

impl Panes {
    pub fn resize_step(self) -> f32 {
        if self.resize_step.is_nan() { Self::default().resize_step } else { self.resize_step.clamp(0.1, 0.9) }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabBar {
    #[default]
    Always,
    Auto,
    Never,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabWidth {
    #[default]
    Fit,
    Fixed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabBarPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Appearance {
    pub color_scheme_mode: SchemeMode,
    pub color_scheme: String,
    pub light_color_scheme: String,
    pub cycle_tabs: bool,
    pub animations: bool,
    pub tray: bool,
    pub ui_font: String,
    pub ui_from_scheme: bool,
    pub background_image: String,
    pub background_image_opacity: f32,
    pub background_image_fit: crate::background::Fit,
    pub density: f32,
    pub opacity: f32,
    pub backdrop: Backdrop,
    pub dock: Dock,
    pub dock_fill: f32,
    pub dock_space: f32,
    pub dock_always_on_top: bool,
    pub dock_hide_on_blur: bool,
    pub global_hotkey: String,
    pub language: String,
    pub tab_bar: TabBar,
    pub tab_index: bool,
    pub tab_icon: bool,
    pub pane_titles: bool,
    pub profile_sidebar: bool,
    pub profile_sidebar_width: f32,
    pub tab_close_buttons: bool,
    pub tab_menu_button: bool,
    pub tabs_in_fullscreen: bool,
    pub tab_width: TabWidth,
    pub tab_bar_position: TabBarPosition,
    pub mru_tabs: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            color_scheme_mode: SchemeMode::Dark,
            color_scheme: "Aster Dark".into(),
            light_color_scheme: "Aster Light".into(),
            cycle_tabs: true,
            animations: true,
            tray: false,
            ui_font: String::new(),
            ui_from_scheme: true,
            background_image: String::new(),
            background_image_opacity: 0.25,
            background_image_fit: Default::default(),
            density: 1.0,
            opacity: 1.0,
            backdrop: Backdrop::None,
            dock: Dock::Off,
            dock_fill: 0.5,
            dock_space: 1.0,
            dock_always_on_top: true,
            dock_hide_on_blur: false,
            global_hotkey: "Ctrl-`".into(),
            language: "auto".into(),
            tab_bar: TabBar::Always,
            tab_index: false,
            tab_icon: true,
            pane_titles: false,
            profile_sidebar: false,
            profile_sidebar_width: crate::profile_sidebar::DEFAULT_WIDTH,
            tab_close_buttons: true,
            tab_menu_button: true,
            tabs_in_fullscreen: false,
            tab_width: TabWidth::Fit,
            tab_bar_position: TabBarPosition::Top,
            mru_tabs: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct CustomScheme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_text: Option<String>,
    pub colors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct System {
    pub explorer_menu: bool,
    pub autostart: bool,
    pub url_scheme: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Updates {
    pub check_automatically: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Picker {
    pub show_builtin_profiles: bool,
    pub quick_connect: crate::quick_connect::Provider,
}

impl Default for Picker {
    fn default() -> Self {
        Self { show_builtin_profiles: true, quick_connect: crate::quick_connect::Provider::default() }
    }
}

impl Default for Updates {
    fn default() -> Self {
        Self { check_automatically: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Vault {
    pub sealed: String,
    pub remember_minutes: u32,
    pub encrypt_config: bool,
}

impl Default for Vault {
    fn default() -> Self {
        Self { sealed: String::new(), remember_minutes: 15, encrypt_config: false }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct Palette {
    pub hidden_commands: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Colors {
    pub generate_256: bool,
    pub harmonious: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Extensions {
    pub index_url: String,
    pub disabled: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    #[serde(skip)]
    pub version: u32,
    pub palette: Palette,
    pub terminal: Terminal,
    pub appearance: Appearance,
    pub colors: Colors,
    pub panes: Panes,
    pub notifications: Notifications,
    pub dock: DockSettings,
    pub vault: Vault,
    pub sync: crate::sync::Preferences,
    pub ssh: Ssh,
    pub system: System,
    pub updates: Updates,
    pub extensions: Extensions,
    pub picker: Picker,
    pub hotkeys: BTreeMap<String, Vec<String>>,
    pub profiles: Vec<Profile>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hidden_profiles: Vec<String>,
    pub color_schemes: Vec<CustomScheme>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<ProfileGroup>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub profile_defaults: BTreeMap<String, ProfileDefaults>,
}

impl Default for Config {
    fn default() -> Self {
        let terminal = Terminal::default();
        let appearance = Appearance::default();
        let system = System::default();
        Self {
            version: crate::config_schema::CURRENT,
            terminal,
            appearance,
            colors: Colors::default(),
            panes: Panes::default(),
            notifications: Notifications::default(),
            dock: DockSettings::default(),
            vault: Vault::default(),
            sync: crate::sync::Preferences::default(),
            ssh: Ssh::default(),
            system,
            updates: Updates::default(),
            extensions: Extensions::default(),
            picker: Picker::default(),
            palette: Palette::default(),
            hotkeys: BTreeMap::new(),
            profiles: Vec::new(),
            hidden_profiles: Vec::new(),
            color_schemes: Vec::new(),
            groups: Vec::new(),
            profile_defaults: BTreeMap::new(),
        }
    }
}

fn format_error(text: &str, e: toml::de::Error) -> String {
    let Some(prefix) = e.span().and_then(|span| text.get(..span.start)) else { return e.message().to_owned() };
    let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or_default().chars().count() + 1;
    crate::i18n::tf("Line {line}, column {column}: {error}", &[("line", &line.to_string()), ("column", &column.to_string()), ("error", e.message())])
}

impl Config {
    pub(crate) fn profile_in_picker(&self, profile: &Profile) -> bool {
        !self.profile_hidden(profile) && (self.picker.show_builtin_profiles || !profile.builtin || profile.id.starts_with("ssh:") || profile.id.starts_with("serial:"))
    }

    pub fn profile_hidden(&self, profile: &Profile) -> bool {
        if profile.builtin {
            self.hidden_profiles.contains(&profile.id)
        } else {
            profile.hidden
        }
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let value: toml::Value = toml::from_str(text).map_err(|e| format_error(text, e))?;
        Self::from_value(text, value)
    }

    pub(crate) fn from_value(text: &str, value: toml::Value) -> Result<Config, String> {
        use crate::config_schema::{decode_newer, migrate, version, CURRENT};
        let from = version(&value).ok_or_else(|| crate::i18n::t("The config version must be a nonnegative integer").to_owned())?;
        let mut config = if from > CURRENT {
            decode_newer(value)
        } else {
            migrate(value, from).try_into::<Self>().map_err(|e| {
                toml::from_str::<Self>(text).err().map_or_else(|| e.message().to_owned(), |e| format_error(text, e))
            })?
        };
        config.version = from.max(CURRENT);
        Ok(config)
    }

    pub fn read_only(&self) -> bool {
        self.version > crate::config_schema::CURRENT
    }

    pub fn read_only_message(&self) -> String {
        crate::i18n::tf("Config version {version} is newer. Settings are read-only.", &[("version", &self.version.to_string())])
    }

    pub fn replace_vault_seal(text: &str, sealed: &str) -> Option<String> {
        crate::config_schema::replace_vault_seal(text, sealed)
    }

    pub fn to_text(&self) -> String {
        crate::config_defaults::overrides(self).and_then(|table| toml::to_string_pretty(&table))
            .map(|text| self.versioned_text(&text)).unwrap_or_default()
    }

    pub fn defaults_text() -> String {
        let defaults = Self::default();
        toml::to_string_pretty(&defaults).map(|text| defaults.versioned_text(&text)).unwrap_or_default()
    }

    fn versioned_text(&self, body: &str) -> String {
        if body.is_empty() { format!("version = {}\n", self.version) } else { format!("version = {}\n\n{body}", self.version) }
    }
}

#[cfg(test)]
#[path = "../test/config.rs"]
mod tests;
