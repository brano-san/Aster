use crate::app::{App, Effect};
use crate::config::Config;
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
const DOCK: &str = "Docked window";
const SSH: &str = "SSH";

const CURSORS: &[(&str, &str)] = &[("block", "Block"), ("beam", "Beam"), ("underline", "Underline")];
const RIGHT_CLICK: &[(&str, &str)] = &[("off", "Nothing"), ("menu", "Context menu"), ("paste", "Paste"), ("clipboard", "Copy or paste")];
const SCHEME_MODES: &[(&str, &str)] = &[("dark", "Dark"), ("light", "Light"), ("auto", "Follow the system")];
const BELLS: &[(&str, &str)] = &[("off", "Off"), ("visual", "Flash the pane"), ("audible", "System sound")];
const BACKDROPS: &[(&str, &str)] = &[("none", "None"), ("acrylic", "Acrylic"), ("mica", "Mica")];
const SSH_CLIENTS: &[(&str, &str)] = &[("native", "Built in"), ("system", "System OpenSSH")];
const EDGES: &[(&str, &str)] = &[("off", "Off"), ("top", "Top"), ("bottom", "Bottom"), ("left", "Left"), ("right", "Right")];
const SHARE: Control = Control::Number { min: 0.1, max: 1.0, step: 0.05 };

pub const SETTINGS: &[Setting] = &[
    item(TERMINAL, "terminal.font", "Font", "Monospace font family for the terminal", Control::Text),
    item(TERMINAL, "terminal.font-size", "Font size", "Size in points before zoom", Control::Number { min: 6.0, max: 48.0, step: 1.0 }),
    item(TERMINAL, "terminal.line-padding", "Line padding", "Extra pixels between lines", Control::Number { min: 0.0, max: 20.0, step: 1.0 }),
    item(TERMINAL, "terminal.cursor", "Cursor shape", "Shape of the text cursor", Control::Choice(CURSORS)),
    item(TERMINAL, "terminal.cursor-blink", "Blinking cursor", "The cursor blinks while the window has focus", Control::Toggle),
    item(TERMINAL, "terminal.scrollback-lines", "Scrollback", "Lines kept above the screen", Control::Number { min: 0.0, max: 1_000_000.0, step: 1000.0 }),
    item(TERMINAL, "terminal.draw-bold-text-in-bright-colors", "Bold text in bright colors", "Bold text uses the bright palette colors", Control::Toggle),
    item(TERMINAL, "terminal.profile", "Default profile", "Shell for new tabs and windows", Control::Profiles),
    item(TERMINAL, "terminal.recover-tabs", "Restore tabs", "Reopen tabs, splits and their folders on start", Control::Toggle),
    item(TERMINAL, "terminal.bell", "Bell", "What happens when a program rings the bell", Control::Choice(BELLS)),
    item(TERMINAL, "terminal.scroll-on-input", "Scroll on input", "Typing scrolls back to the bottom", Control::Toggle),
    item(INPUT, "terminal.copy-on-select", "Copy on select", "Selected text goes to the clipboard", Control::Toggle),
    item(INPUT, "terminal.right-click", "Right click", "What the right mouse button does", Control::Choice(RIGHT_CLICK)),
    item(INPUT, "terminal.paste-on-middle-click", "Paste on middle click", "The middle mouse button pastes", Control::Toggle),
    item(INPUT, "terminal.bracketed-paste", "Bracketed paste", "Programs can tell pasted text from typing", Control::Toggle),
    item(INPUT, "terminal.trim-whitespace-on-paste", "Trim pasted text", "Remove trailing whitespace from pasted text", Control::Toggle),
    item(INPUT, "terminal.warn-on-multiline-paste", "Confirm multiline paste", "Ask before pasting several lines into a program that would run each one", Control::Toggle),
    item(INPUT, "terminal.word-separator", "Word separators", "Characters that end a word on double click", Control::Text),
    item(SEARCH, "terminal.search-options.case-sensitive", "Match case", "Search tells upper and lower case apart", Control::Toggle),
    item(SEARCH, "terminal.search-options.regex", "Regular expressions", "Search queries are regular expressions", Control::Toggle),
    item(SEARCH, "terminal.search-options.whole-word", "Whole words", "Search matches whole words only", Control::Toggle),
    item(APPEARANCE, "appearance.color-scheme-mode", "Theme", "Dark, light or the system setting", Control::Choice(SCHEME_MODES)),
    item(APPEARANCE, "appearance.color-scheme", "Dark color scheme", "Colors of the dark theme", Control::Schemes),
    item(APPEARANCE, "appearance.light-color-scheme", "Light color scheme", "Colors of the light theme", Control::Schemes),
    item(APPEARANCE, "appearance.opacity", "Opacity", "Background opacity of the terminal", Control::Number { min: 0.2, max: 1.0, step: 0.05 }),
    item(APPEARANCE, "appearance.backdrop", "Backdrop", "System material behind the window on Windows 11", Control::Choice(BACKDROPS)),
    item(APPEARANCE, "appearance.ui-font", "Interface font", "Font of tabs and menus; empty uses the system font", Control::Text),
    item(APPEARANCE, "appearance.cycle-tabs", "Cycle tabs", "Switching past the last tab wraps around", Control::Toggle),
    item(APPEARANCE, "appearance.last-tab-closes-window", "Last tab closes the window", "Closing the only tab closes its window", Control::Toggle),
    item(DOCK, "appearance.dock", "Dock to edge", "Pin the window to a screen edge", Control::Choice(EDGES)),
    item(DOCK, "appearance.dock-fill", "Size", "Share of the screen across the edge", SHARE),
    item(DOCK, "appearance.dock-space", "Length", "Share of the screen along the edge", SHARE),
    item(DOCK, "appearance.dock-always-on-top", "Always on top", "The docked window stays above other windows", Control::Toggle),
    item(DOCK, "appearance.dock-hide-on-blur", "Hide on focus loss", "The docked window hides when another window is activated", Control::Toggle),
    item(DOCK, "appearance.global-hotkey", "Global hotkey", "Shows and hides the window from anywhere, for example Ctrl-`", Control::Text),
    item(SSH, "ssh.client", "SSH client", "Built-in client, or the ssh program installed in the system", Control::Choice(SSH_CLIENTS)),
    item(SSH, "ssh.remember-passwords", "Offer to save passwords", "After a typed password or key passphrase works, offer to keep it in the system credential store", Control::Toggle),
    item(SSH, "ssh.forward-agent", "Forward the SSH agent", "Let servers use your local SSH agent; ForwardAgent in ~/.ssh/config decides per host", Control::Toggle),
    item(SSH, "ssh.share-connections", "Share connections", "New tabs to a host that is already open reuse its login", Control::Toggle),
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
pub const FIELDS: [&str; 5] = ["Name", "Command", "Arguments", "Start folder", "SSH destination"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileRow {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub custom: bool,
    pub default: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileEditor {
    pub original: Option<String>,
    pub source: Option<String>,
    pub fields: [String; 5],
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
        Some(a) => found(&format!("{SHORTCUTS} hotkey {} {}", a.label(), a.id()), query),
        None => {
            let s = &SETTINGS[item];
            found(&format!("{} {} {} {}", s.section, s.label, s.hint, s.key), query)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsPage {
    pub query: String,
    pub shown: Vec<usize>,
    pub selected: usize,
    pub scroll: f32,
    pub editing: Option<String>,
    pub capture: Option<Action>,
    pub notice: Option<String>,
    pub profiles: Vec<ProfileRow>,
    pub editor: Option<ProfileEditor>,
    pub deleting: Option<String>,
}

impl SettingsPage {
    pub fn new() -> Self {
        let mut page = Self::default();
        page.refilter();
        page
    }

    fn refilter(&mut self) {
        let actions = Action::all().count();
        let base = SETTINGS.len() + actions;
        let settings = (0..SETTINGS.len()).filter(|&i| item_matches(i, &self.query));
        let profiles = (0..self.profiles.len()).filter(|&k| {
            let p = &self.profiles[k];
            found(&format!("{PROFILES} profile shell {} {} {}", p.name, p.detail, p.id), &self.query)
        });
        let new_row = found(&format!("{PROFILES} new profile add create"), &self.query).then_some(base + self.profiles.len());
        let shortcuts = (SETTINGS.len()..base).filter(|&i| item_matches(i, &self.query));
        self.shown = settings.chain(profiles.map(|k| base + k)).chain(new_row).chain(shortcuts).collect();
        self.selected = 0;
        self.scroll = 0.0;
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

    pub fn section(&self, item: usize) -> &'static str {
        if self.profile_at(item).is_some() || self.is_new_profile(item) {
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
    }
}

pub fn geometry(content: Rect, s: f32, page: &SettingsPage) -> SettingsGeometry {
    let width = (860.0 * s).min(content.w - 48.0 * s).max(240.0 * s).round();
    let x = (content.x + (content.w - width) / 2.0).round();
    let top = content.y + (18.0 * s).round();
    let header_h = (36.0 * s).round();
    let close_w = header_h;
    let close = Rect::new(x + width - close_w, top, close_w, header_h);
    let field_w = (300.0 * s).min(width * 0.5).round();
    let field = Rect::new(close.x - field_w - (8.0 * s).round(), top, field_w, header_h);
    let title = Rect::new(x, top, (field.x - x - 8.0 * s).max(0.0), header_h);
    let list_top = top + header_h + (14.0 * s).round();
    let list = Rect::new(x, list_top, width, (content.bottom() - list_top - 12.0 * s).max(0.0));
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
            None => ((240.0 * s).min(width * 0.45).round(), control_h),
        };
        let control = Rect::new(rect.right() - cw - (14.0 * s).round(), rect.y + ((item_h - ch) / 2.0).round(), cw, ch);
        rows.push(Row { kind: RowKind::Item(n), rect, control });
        y += item_h;
    }
    let height = y + page.scroll - list.y;
    SettingsGeometry { page: content, title, field, close, list, rows, height }
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
    pub fn settings_open(&self) -> bool {
        self.settings.is_some()
    }

    pub(crate) fn open_settings(&mut self) {
        self.overlay = None;
        let mut page = SettingsPage { profiles: self.profile_rows(), ..SettingsPage::default() };
        page.refilter();
        self.settings = Some(page);
        self.dirty = true;
    }

    fn profile_rows(&self) -> Vec<ProfileRow> {
        let default = self.default_profile().map(|p| p.id);
        self.profiles
            .iter()
            .map(|p| ProfileRow {
                id: p.id.clone(),
                name: p.name.clone(),
                detail: match &p.ssh {
                    Some(dest) => format!("ssh {dest}"),
                    None => std::iter::once(p.command.clone())
                        .chain(p.args.iter().enumerate().map(|(i, a)| if i > 0 && p.args[i - 1].eq_ignore_ascii_case("-EncodedCommand") { "\u{2026}".to_owned() } else { a.clone() }))
                        .collect::<Vec<_>>()
                        .join(" "),
                },
                custom: self.config.profiles.iter().any(|c| c.id == p.id),
                default: default.as_deref() == Some(p.id.as_str()),
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
        let editor = if let Some(row) = page.profile_at(item) {
            let Some(p) = self.profiles.iter().find(|p| p.id == row.id) else { return };
            ProfileEditor {
                original: row.custom.then(|| p.id.clone()),
                source: Some(p.id.clone()),
                fields: [
                    if row.custom { p.name.clone() } else { format!("{} (copy)", p.name) },
                    p.command.clone(),
                    join_args(&p.args),
                    p.cwd.clone().unwrap_or_default(),
                    p.ssh.clone().unwrap_or_default(),
                ],
                focus: 0,
                error: None,
            }
        } else {
            ProfileEditor::default()
        };
        if let Some(page) = self.settings.as_mut() {
            page.editor = Some(editor);
        }
        self.dirty = true;
    }

    fn save_editor(&mut self) {
        let Some(editor) = self.settings.as_ref().and_then(|p| p.editor.clone()) else { return };
        let [name, command, args, cwd, ssh] = editor.fields.clone().map(|f| f.trim().to_owned());
        let fail = |app: &mut App, error: &str| {
            if let Some(e) = app.settings.as_mut().and_then(|p| p.editor.as_mut()) {
                e.error = Some(error.to_owned());
            }
            app.dirty = true;
        };
        if name.is_empty() {
            return fail(self, "The profile needs a name");
        }
        if command.is_empty() && ssh.is_empty() {
            return fail(self, "Give a command to run or an SSH destination");
        }
        let id = editor.original.clone().unwrap_or_else(|| {
            let base = format!("custom:{}", slug(&name));
            let taken = |id: &str| self.profiles.iter().any(|p| p.id == id);
            if !taken(&base) {
                base
            } else {
                (2..).map(|n| format!("{base}-{n}")).find(|c| !taken(c)).unwrap_or(base)
            }
        });
        let ssh = (!ssh.is_empty()).then_some(ssh);
        let (command, args) = match (&ssh, command.is_empty()) {
            (Some(dest), true) => ("ssh".to_owned(), vec![dest.clone()]),
            _ => (command, split_args(&args)),
        };
        let mut profile = crate::profile::Profile { id: id.clone(), name, command, args, cwd: (!cwd.is_empty()).then_some(cwd), ssh: ssh.clone(), ..Default::default() };
        if let Some(source) = editor.source.as_ref().and_then(|id| self.profiles.iter().find(|p| &p.id == id)) {
            profile.env = source.env.clone();
            profile.icon = source.icon.clone();
            profile.color = source.color.clone();
        }
        if ssh.is_some() {
            profile.icon = Some("ssh".into());
        }
        let mut config = self.config.clone();
        match config.profiles.iter_mut().find(|p| p.id == id) {
            Some(existing) => {
                profile.env = existing.env.clone();
                profile.color = existing.color.clone();
                *existing = profile;
            }
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
        self.dirty = true;
    }

    pub fn settings_geometry(&self) -> Option<SettingsGeometry> {
        self.settings.as_ref().map(|p| geometry(self.chrome.content, self.scale, p))
    }

    pub fn setting_options(&self, control: Control) -> Vec<(String, String)> {
        match control {
            Control::Choice(list) => list.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect(),
            Control::Schemes => self.scheme_names().into_iter().map(|n| (n.clone(), n)).collect(),
            Control::Profiles => std::iter::once((String::new(), "First available".to_owned())).chain(self.profiles.iter().map(|p| (p.id.clone(), p.name.clone()))).collect(),
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
            "None".into()
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
            page.notice = (!taken.is_empty()).then(|| format!("{text} was taken from {}", taken.join(", ")));
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
            p.editing = Some(text);
        }
        self.dirty = true;
    }

    fn commit_edit(&mut self) {
        let Some(page) = self.settings.as_mut() else { return };
        let Some(text) = page.editing.take() else { return };
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
        if let Some(item) = page.current_item() {
            let row = page.profile_at(item).cloned();
            if row.is_some() || page.is_new_profile(item) {
                if key != Key::Delete {
                    page.deleting = None;
                }
                match (key, row) {
                    (Key::Enter, Some(r)) if mods.ctrl => return self.make_default(&r.id),
                    (Key::Enter, _) => return self.open_editor(item),
                    (Key::Delete, Some(r)) if r.custom => {
                        if page.deleting.as_deref() == Some(r.id.as_str()) {
                            page.deleting = None;
                            return self.delete_profile(&r.id);
                        }
                        page.notice = Some(format!("Press Del again to delete {}", r.name));
                        page.deleting = Some(r.id);
                        return;
                    }
                    (Key::Delete, Some(_)) => {
                        page.notice = Some("Detected profiles cannot be deleted; copy one with Enter instead".into());
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
                Key::Enter | Key::Tab => self.commit_edit(),
                Key::Backspace => {
                    if let Some(t) = page.editing.as_mut() {
                        if mods.ctrl {
                            t.clear();
                        } else {
                            t.pop();
                        }
                    }
                }
                Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
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
            Some(t) => t.push_str(&clean),
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
        if geo.close.contains(x, y) {
            return self.close_settings();
        }
        if geo.field.contains(x, y) {
            return self.commit_edit();
        }
        if !geo.list.contains(x, y) {
            return self.commit_edit();
        }
        let Some(row) = geo.rows.iter().find(|r| r.rect.contains(x, y)).copied() else { return self.commit_edit() };
        let RowKind::Item(n) = row.kind else { return self.commit_edit() };
        let editing_same = self.settings.as_ref().is_some_and(|p| p.selected == n && p.editing.is_some());
        let control = self.settings.as_ref().and_then(|p| SETTINGS.get(p.shown[n])).map(|s| s.control);
        if editing_same && row.control.contains(x, y) && control.is_some_and(|c| part_at(c, row.control, self.scale, x) == Part::Whole) {
            return;
        }
        self.commit_edit();
        let Some(page) = self.settings.as_mut() else { return };
        page.selected = n;
        page.notice = None;
        let item = page.shown[n];
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
            control if row.control.contains(x, y) => match part_at(control, row.control, self.scale, x) {
                Part::Less => self.adjust_setting(setting, -1),
                Part::More => self.adjust_setting(setting, 1),
                Part::Whole => self.adjust_setting(setting, 0),
            },
            _ => {}
        }
    }

    pub(crate) fn settings_wheel(&mut self, dy: f32) {
        let Some(geo) = self.settings_geometry() else { return };
        let Some(page) = self.settings.as_mut() else { return };
        page.scroll = (page.scroll - dy * 3.0 * 20.0 * self.scale).clamp(0.0, (geo.height - geo.list.h).max(0.0));
        self.dirty = true;
    }
}

#[cfg(test)]
#[path = "../test/settings.rs"]
mod tests;
