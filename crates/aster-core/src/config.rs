use crate::profile::Profile;
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
pub enum Bell {
    Off,
    #[default]
    Visual,
    Audible,
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
}

impl Default for Ssh {
    fn default() -> Self {
        Self { client: SshClient::Native, share_connections: true }
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
    pub font_size: f32,
    pub line_padding: f32,
    pub scrollback_lines: usize,
    pub cursor: CursorStyle,
    pub cursor_blink: bool,
    pub copy_on_select: bool,
    pub right_click: RightClick,
    pub paste_on_middle_click: bool,
    pub bracketed_paste: bool,
    pub trim_whitespace_on_paste: bool,
    pub warn_on_multiline_paste: bool,
    pub bell: Bell,
    pub draw_bold_text_in_bright_colors: bool,
    pub word_separator: String,
    pub scroll_on_input: bool,
    pub profile: String,
    pub environment: BTreeMap<String, String>,
    pub search_options: SearchOptions,
    pub recover_tabs: bool,
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            font: "Cascadia Mono".into(),
            font_size: 14.0,
            line_padding: 0.0,
            scrollback_lines: 25_000,
            cursor: CursorStyle::Block,
            cursor_blink: true,
            copy_on_select: true,
            right_click: RightClick::Clipboard,
            paste_on_middle_click: false,
            bracketed_paste: true,
            trim_whitespace_on_paste: true,
            warn_on_multiline_paste: true,
            bell: Bell::Visual,
            draw_bold_text_in_bright_colors: true,
            word_separator: " ()[]{}'\"".into(),
            scroll_on_input: true,
            profile: String::new(),
            environment: BTreeMap::new(),
            search_options: SearchOptions::default(),
            recover_tabs: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Appearance {
    pub color_scheme_mode: SchemeMode,
    pub color_scheme: String,
    pub light_color_scheme: String,
    pub cycle_tabs: bool,
    pub last_tab_closes_window: bool,
    pub ui_font: String,
    pub opacity: f32,
    pub backdrop: Backdrop,
    pub dock: Dock,
    pub dock_fill: f32,
    pub dock_space: f32,
    pub dock_always_on_top: bool,
    pub dock_hide_on_blur: bool,
    pub global_hotkey: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            color_scheme_mode: SchemeMode::Dark,
            color_scheme: "Aster Dark".into(),
            light_color_scheme: "Aster Light".into(),
            cycle_tabs: true,
            last_tab_closes_window: true,
            ui_font: String::new(),
            opacity: 1.0,
            backdrop: Backdrop::None,
            dock: Dock::Off,
            dock_fill: 0.5,
            dock_space: 1.0,
            dock_always_on_top: true,
            dock_hide_on_blur: false,
            global_hotkey: "Ctrl-`".into(),
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    pub terminal: Terminal,
    pub appearance: Appearance,
    pub ssh: Ssh,
    pub hotkeys: BTreeMap<String, Vec<String>>,
    pub profiles: Vec<Profile>,
    pub color_schemes: Vec<CustomScheme>,
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn to_text(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "../test/config.rs"]
mod tests;
