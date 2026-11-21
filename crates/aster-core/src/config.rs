use crate::profile::{Profile, ProfileDefaults, ProfileGroup};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    pub clear_after_connect: bool,
    pub x11_display: String,
}

impl Default for Ssh {
    fn default() -> Self {
        Self { client: SshClient::Native, share_connections: true, remember_passwords: true, forward_agent: false, clear_after_connect: true, x11_display: String::new() }
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
    pub environment: BTreeMap<String, String>,
    pub identification: Identification,
    pub search_options: SearchOptions,
    pub recover_tabs: bool,
    pub open_on_start: bool,
    pub confirm_close: bool,
    pub focus_follows_mouse: bool,
    pub recent_profiles: usize,
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            font: "Cascadia Mono".into(),
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
            identification: Identification::Aster,
            search_options: SearchOptions::default(),
            recover_tabs: true,
            open_on_start: true,
            confirm_close: true,
            focus_follows_mouse: false,
            recent_profiles: 3,
        }
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
    pub last_tab_closes_window: bool,
    pub tray: bool,
    pub ui_font: String,
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
    pub tab_close_buttons: bool,
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
            last_tab_closes_window: true,
            tray: false,
            ui_font: String::new(),
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
            tab_close_buttons: true,
            tabs_in_fullscreen: false,
            tab_width: TabWidth::Fit,
            tab_bar_position: TabBarPosition::Top,
            mru_tabs: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CustomScheme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: Option<String>,
    pub colors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct System {
    pub explorer_menu: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Vault {
    pub sealed: String,
    pub remember_minutes: u32,
}

impl Default for Vault {
    fn default() -> Self {
        Self { sealed: String::new(), remember_minutes: 15 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    pub terminal: Terminal,
    pub appearance: Appearance,
    pub vault: Vault,
    pub ssh: Ssh,
    pub system: System,
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

impl Config {
    pub fn profile_hidden(&self, profile: &Profile) -> bool {
        if profile.builtin {
            self.hidden_profiles.contains(&profile.id)
        } else {
            profile.hidden
        }
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        toml::from_str(text).map_err(|e| {
            let Some(prefix) = e.span().and_then(|span| text.get(..span.start)) else { return e.message().to_owned() };
            let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
            let column = prefix.rsplit('\n').next().unwrap_or_default().chars().count() + 1;
            crate::i18n::tf("Line {line}, column {column}: {error}", &[("line", &line.to_string()), ("column", &column.to_string()), ("error", e.message())])
        })
    }

    pub fn to_text(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "../test/config.rs"]
mod tests;
