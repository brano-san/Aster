use crate::config::{Bell, Config, CursorStyle, RightClick, SchemeMode};
use crate::input::{CursorIcon, Event, HitArea, Key, Mods, MouseButton, Preedit};
use crate::keymap::{Action, Chord, Keymap, Outcome};
use crate::keys::{encode_alt_char, encode_key, encode_mouse, paste_bytes, MouseAction};
use crate::overlay::{Overlay, OverlayKind, Pick};
use crate::panes::{neighbor, Axis, Layout, Node, PaneId, Side};
use crate::profile::Profile;
use crate::theme::Theme;
use crate::view::{self, Chrome};
use aster_gfx::render::Rect;
use aster_vt::schemes::{self, Scheme};
use aster_vt::{CursorShape, FairMutex, Options, Palette, Scroll, SelectionType, Sink, Size, TermMode, Terminal, VtEvent};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub type Shared = Arc<FairMutex<Terminal>>;
pub type Wake = Arc<dyn Fn() + Send + Sync>;
pub type CwdProbe = Arc<dyn Fn(u32) -> Option<String> + Send + Sync>;

pub const BLINK: Duration = Duration::from_millis(530);
const BELL_FLASH: Duration = Duration::from_millis(150);
const BELL_GAP: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
}

pub enum Effect {
    Spawn { pane: PaneId, launch: Launch, terminal: Shared },
    Write { pane: PaneId, bytes: Vec<u8> },
    Resize { pane: PaneId, cols: u16, rows: u16 },
    Kill { pane: PaneId },
    SetClipboard(String),
    RequestPaste,
    SaveConfig(String),
    Open(String),
    Title(String),
    Cursor(CursorIcon),
    Minimize,
    ToggleMaximize,
    ToggleFullscreen,
    Close,
    NewWindow,
    Timer(u32),
    Attention,
    Beep,
    Surface(crate::config::Backdrop),
    ImeArea(Rect),
}

pub(crate) struct Pane {
    pub terminal: Shared,
    pub profile: Profile,
    pub title: Option<String>,
    pub exited: Option<Option<u32>>,
    pub pending: Option<Launch>,
    pub queued: Vec<u8>,
    pub pid: Option<u32>,
    pub size: Size,
    pub scroll_acc: f32,
}

pub(crate) struct Tab {
    pub root: Node,
    pub focus: PaneId,
    pub zoomed: Option<PaneId>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Drag {
    None,
    Select { pane: PaneId },
    Report { pane: PaneId, button: MouseButton },
    Splitter { path: Vec<bool>, axis: Axis, span: Rect },
    Tab { index: usize, start_x: f32, moved: bool },
    Button(HitArea),
    TabClose(usize),
    Plus,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Metrics {
    pub w: f32,
    pub h: f32,
    pub baseline: f32,
    pub px: f32,
}

pub struct App {
    pub(crate) config: Config,
    pub(crate) profiles: Vec<Profile>,
    detected: Vec<Profile>,
    schemes: Vec<Scheme>,
    scheme_before: Option<String>,
    pub(crate) keymap: Keymap,
    pub(crate) palette: Palette,
    pub(crate) theme: Theme,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: usize,
    pub(crate) panes: HashMap<PaneId, Pane>,
    next_id: PaneId,
    pub(crate) size: (f32, f32),
    pub(crate) scale: f32,
    pub(crate) zoom: f32,
    pub(crate) focused: bool,
    system_dark: bool,
    effects: Vec<Effect>,
    pub(crate) dirty: bool,
    events: Receiver<(PaneId, VtEvent)>,
    sender: Sender<(PaneId, VtEvent)>,
    wake: Wake,
    home: Option<String>,
    ssh: Option<String>,
    cwd_probe: Option<CwdProbe>,
    pub(crate) mouse: (f32, f32),
    pub(crate) drag: Drag,
    last_click: Option<(Instant, f32, f32, u8)>,
    pub(crate) hover: HitArea,
    pub(crate) hover_link: Option<HoverLink>,
    pub(crate) overlay: Option<Overlay>,
    pub(crate) settings: Option<crate::settings::SettingsPage>,
    pub(crate) menu: Option<crate::menu::Menu>,
    pub(crate) rename: Option<crate::tabs::Rename>,
    pub(crate) paste_confirm: Option<crate::paste::PasteConfirm>,
    pub(crate) bell: Option<(PaneId, Instant)>,
    last_bell: Option<Instant>,
    bell_pane: Option<PaneId>,
    translucent: bool,
    backdrop: bool,
    pub(crate) search: Option<crate::search::SearchBar>,
    swallow_char: bool,
    pub(crate) blink_on: bool,
    blink_since: Instant,
    blink_armed: bool,
    pub(crate) metrics: Option<Metrics>,
    pub(crate) layout: Layout,
    pub(crate) chrome: Chrome,
    pub(crate) preedit: Preedit,
    title: String,
    cursor_icon: CursorIcon,
    ime_area: Rect,
}

impl App {
    pub fn new(config: Config, detected: Vec<Profile>, system_dark: bool, home: Option<String>, wake: Wake) -> App {
        let mut profiles = detected.clone();
        profiles.extend(config.profiles.iter().cloned());
        let (sender, events) = channel();
        let keymap = Keymap::new(&config.hotkeys);
        let mut app = App {
            config,
            profiles,
            detected,
            schemes: schemes::builtin(),
            scheme_before: None,
            keymap,
            palette: Palette::default(),
            theme: Theme::from_palette(&Palette::default()),
            tabs: Vec::new(),
            active: 0,
            panes: HashMap::new(),
            next_id: 1,
            size: (0.0, 0.0),
            scale: 1.0,
            zoom: 0.0,
            focused: true,
            system_dark,
            effects: Vec::new(),
            dirty: true,
            events,
            sender,
            wake,
            home,
            ssh: None,
            cwd_probe: None,
            mouse: (-1.0, -1.0),
            drag: Drag::None,
            last_click: None,
            hover: HitArea::Client,
            hover_link: None,
            overlay: None,
            settings: None,
            menu: None,
            rename: None,
            paste_confirm: None,
            bell: None,
            last_bell: None,
            bell_pane: None,
            translucent: false,
            backdrop: false,
            search: None,
            swallow_char: false,
            blink_on: true,
            blink_since: Instant::now(),
            blink_armed: false,
            metrics: None,
            layout: Layout::default(),
            chrome: Chrome::default(),
            preedit: Preedit::default(),
            title: String::new(),
            cursor_icon: CursorIcon::Default,
            ime_area: Rect::default(),
        };
        app.restyle();
        app.effects.push(Effect::Surface(app.config.appearance.backdrop));
        app
    }

    pub fn set_surface(&mut self, translucent: bool, backdrop: bool) {
        if (self.translucent, self.backdrop) != (translucent, backdrop) {
            self.translucent = translucent;
            self.backdrop = backdrop;
            self.restyle();
        }
    }

    pub fn set_cwd_probe(&mut self, probe: CwdProbe) {
        self.cwd_probe = Some(probe);
    }

    pub fn set_pane_pid(&mut self, pane: PaneId, pid: u32) {
        if let Some(p) = self.panes.get_mut(&pane) {
            p.pid = Some(pid);
        }
    }

    pub(crate) fn pane_cwd(&self, id: PaneId) -> Option<String> {
        let pane = self.panes.get(&id)?;
        if let Some(dir) = pane.terminal.lock().cwd() {
            return Some(dir.to_owned());
        }
        let remote = pane.profile.id.starts_with("ssh:") || pane.profile.id.starts_with("local:wsl");
        if remote {
            return None;
        }
        let (probe, pid) = (self.cwd_probe.as_ref()?, pane.pid?);
        probe(pid)
    }

    pub fn set_ssh(&mut self, command: Option<String>) {
        self.ssh = command;
    }

    pub fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn is_dark(&self) -> bool {
        self.theme.dark
    }

    fn restyle(&mut self) {
        let dark = match self.config.appearance.color_scheme_mode {
            SchemeMode::Dark => true,
            SchemeMode::Light => false,
            SchemeMode::Auto => self.system_dark,
        };
        let name = if dark { self.config.appearance.color_scheme.clone() } else { self.config.appearance.light_color_scheme.clone() };
        let fallback = if dark { Palette::DARK } else { Palette::LIGHT };
        let mut palette = self.scheme(&name).unwrap_or(fallback);
        palette.bold_is_bright = self.config.terminal.draw_bold_text_in_bright_colors;
        self.palette = palette;
        let mut theme = Theme::from_palette(&palette);
        let opacity = if self.translucent { self.config.appearance.opacity.clamp(0.2, 1.0) } else { 1.0 };
        let alpha = (opacity * 255.0).round() as u8;
        theme.background = theme.background.alpha(alpha);
        theme.chrome = theme.chrome.alpha(if self.backdrop { 0 } else { alpha });
        self.theme = theme;
        self.dirty = true;
    }

    fn custom_schemes(&self) -> Vec<Scheme> {
        self.config
            .color_schemes
            .iter()
            .filter_map(|c| {
                let colors: Vec<_> = c.colors.iter().filter_map(|s| schemes::parse_hex(s)).collect();
                let cursor = c.cursor.as_deref().and_then(schemes::parse_hex);
                let palette = schemes::from_colors(schemes::parse_hex(&c.foreground)?, schemes::parse_hex(&c.background)?, cursor, &colors)?;
                Some(Scheme { name: c.name.clone(), palette })
            })
            .collect()
    }

    pub fn scheme_names(&self) -> Vec<String> {
        self.custom_schemes().into_iter().chain(self.schemes.iter().cloned()).map(|s| s.name).collect()
    }

    fn scheme(&self, name: &str) -> Option<Palette> {
        self.custom_schemes().into_iter().chain(self.schemes.iter().cloned()).find(|s| s.name.eq_ignore_ascii_case(name)).map(|s| s.palette)
    }

    fn scheme_is_dark(&self) -> bool {
        match self.config.appearance.color_scheme_mode {
            SchemeMode::Dark => true,
            SchemeMode::Light => false,
            SchemeMode::Auto => self.system_dark,
        }
    }

    fn current_scheme(&self) -> String {
        if self.scheme_is_dark() {
            self.config.appearance.color_scheme.clone()
        } else {
            self.config.appearance.light_color_scheme.clone()
        }
    }

    fn set_scheme_name(&mut self, name: String) {
        if self.scheme_is_dark() {
            self.config.appearance.color_scheme = name;
        } else {
            self.config.appearance.light_color_scheme = name;
        }
        self.restyle();
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn set_config(&mut self, config: Config) {
        let mut profiles = self.detected.clone();
        profiles.extend(config.profiles.iter().cloned());
        self.profiles = profiles;
        self.keymap = Keymap::new(&config.hotkeys);
        if config.appearance.backdrop != self.config.appearance.backdrop {
            self.effects.push(Effect::Surface(config.appearance.backdrop));
        }
        self.config = config;
        let options = self.options();
        for p in self.panes.values() {
            p.terminal.lock().set_options(options);
        }
        self.restyle();
    }

    pub fn set_viewport(&mut self, width: u32, height: u32, scale: f32) {
        self.size = (width as f32, height as f32);
        self.scale = scale.max(0.5);
        self.dirty = true;
    }

    pub fn default_profile(&self) -> Option<Profile> {
        let id = &self.config.terminal.profile;
        self.profiles.iter().find(|p| !id.is_empty() && (p.id == *id || p.name == *id)).or_else(|| self.profiles.first()).cloned()
    }

    pub fn open_default_tab(&mut self) {
        if let Some(p) = self.default_profile() {
            self.open_tab(p);
        }
    }

    fn options(&self) -> Options {
        let cursor = match self.config.terminal.cursor {
            CursorStyle::Block => CursorShape::Block,
            CursorStyle::Beam => CursorShape::Beam,
            CursorStyle::Underline => CursorShape::Underline,
        };
        Options { scrollback: self.config.terminal.scrollback_lines, cursor, blink: self.config.terminal.cursor_blink }
    }

    fn new_pane(&mut self, profile: Profile) -> PaneId {
        let inherited = self.focused_pane().and_then(|p| self.pane_cwd(p));
        self.new_pane_in(profile, inherited)
    }

    pub(crate) fn new_pane_in(&mut self, profile: Profile, inherited: Option<String>) -> PaneId {
        let id = self.next_id;
        self.next_id += 1;
        let sender = self.sender.clone();
        let wake = self.wake.clone();
        let sink: Sink = Arc::new(move |e| {
            let _ = sender.send((id, e));
            wake();
        });
        let size = Size::new(80, 24);
        let terminal = Arc::new(FairMutex::new(Terminal::new(size, self.options(), sink)));
        let mut env = self.config.terminal.environment.clone();
        env.extend(profile.env.clone());
        for (k, v) in [("TERM", "xterm-256color"), ("COLORTERM", "truecolor"), ("TERM_PROGRAM", "Aster")] {
            env.entry(k.to_owned()).or_insert_with(|| v.to_owned());
        }
        if inherited.is_some() {
            terminal.lock().set_cwd(inherited.clone());
        }
        let (args, cwd) = inherit_cwd(&profile, inherited, self.home.clone());
        let launch = Launch { command: profile.command.clone(), args, cwd, env, cols: 80, rows: 24 };
        self.panes.insert(id, Pane { terminal, profile, title: None, exited: None, pending: Some(launch), queued: Vec::new(), pid: None, size, scroll_acc: 0.0 });
        id
    }

    pub fn open_tab(&mut self, profile: Profile) {
        let id = self.new_pane(profile);
        let at = if self.tabs.is_empty() { 0 } else { self.active + 1 };
        self.tabs.insert(at, Tab { root: Node::Leaf(id), focus: id, zoomed: None, title: None });
        self.active = at;
        self.overlay = None;
        self.dirty = true;
    }

    pub(crate) fn focused_pane(&self) -> Option<PaneId> {
        self.tabs.get(self.active).map(|t| t.focus)
    }

    pub fn focused_terminal(&self) -> Option<Shared> {
        self.focused_pane().and_then(|p| self.panes.get(&p)).map(|p| p.terminal.clone())
    }

    fn split(&mut self, side: Side) {
        let Some(focus) = self.focused_pane() else { return };
        let profile = self.panes.get(&focus).map(|p| p.profile.clone()).or_else(|| self.default_profile());
        let Some(profile) = profile else { return };
        let id = self.new_pane(profile);
        let tab = &mut self.tabs[self.active];
        tab.zoomed = None;
        tab.root.split(focus, side, id);
        tab.focus = id;
        self.dirty = true;
    }

    pub fn close_pane(&mut self, pane: PaneId) {
        let Some(ti) = self.tabs.iter().position(|t| t.root.contains(pane)) else { return };
        if self.search.as_ref().is_some_and(|s| s.pane == pane) {
            self.search = None;
        }
        if let Some(p) = self.panes.remove(&pane) {
            if p.exited.is_none() {
                self.effects.push(Effect::Kill { pane });
            }
        }
        let tab = &mut self.tabs[ti];
        if tab.root == Node::Leaf(pane) {
            self.remove_tab(ti);
        } else {
            let order = tab.root.panes();
            let at = order.iter().position(|p| *p == pane).unwrap_or(0);
            tab.root.remove(pane);
            let rest = tab.root.panes();
            if tab.focus == pane {
                tab.focus = rest[at.saturating_sub(1).min(rest.len() - 1)];
            }
            if tab.zoomed == Some(pane) {
                tab.zoomed = None;
            }
        }
        self.dirty = true;
    }

    fn remove_tab(&mut self, index: usize) {
        let tab = self.tabs.remove(index);
        for id in tab.root.panes() {
            if let Some(p) = self.panes.remove(&id) {
                if p.exited.is_none() {
                    self.effects.push(Effect::Kill { pane: id });
                }
            }
        }
        if self.tabs.is_empty() {
            self.effects.push(Effect::Close);
            return;
        }
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.dirty = true;
    }

    pub fn close_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.remove_tab(index);
        }
    }

    pub fn close_all(&mut self) {
        let ids: Vec<PaneId> = self.panes.iter().filter(|(_, p)| p.exited.is_none()).map(|(id, _)| *id).collect();
        self.effects.extend(ids.into_iter().map(|pane| Effect::Kill { pane }));
        self.panes.clear();
        self.tabs.clear();
    }

    pub fn activate(&mut self, index: usize) {
        if index < self.tabs.len() && index != self.active {
            self.active = index;
            self.overlay = None;
            self.reset_blink();
            self.dirty = true;
        }
    }

    fn cycle(&mut self, delta: isize) {
        let n = self.tabs.len() as isize;
        if n == 0 {
            return;
        }
        let next = self.active as isize + delta;
        let next = if self.config.appearance.cycle_tabs { next.rem_euclid(n) } else { next.clamp(0, n - 1) };
        self.activate(next as usize);
    }

    fn move_tab(&mut self, delta: isize) {
        let to = self.active as isize + delta;
        if to >= 0 && (to as usize) < self.tabs.len() {
            self.tabs.swap(self.active, to as usize);
            self.active = to as usize;
            self.dirty = true;
        }
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn tab_title(&self, index: usize) -> String {
        let Some(tab) = self.tabs.get(index) else { return String::new() };
        if let Some(t) = &tab.title {
            return t.clone();
        }
        self.panes.get(&tab.focus).map(|p| p.title.clone().unwrap_or_else(|| p.profile.name.clone())).unwrap_or_default()
    }

    fn write(&mut self, pane: PaneId, bytes: Vec<u8>) {
        let Some(p) = self.panes.get_mut(&pane) else { return };
        if p.exited.is_some() {
            self.close_pane(pane);
            return;
        }
        if self.config.terminal.scroll_on_input {
            let mut t = p.terminal.lock();
            if t.display_offset() > 0 {
                t.scroll(Scroll::Bottom);
                self.dirty = true;
            }
        }
        let queued = p.pending.is_some();
        if queued {
            p.queued.extend_from_slice(&bytes);
        }
        self.reset_blink();
        if !queued {
            self.effects.push(Effect::Write { pane, bytes });
        }
    }

    fn write_focused(&mut self, bytes: Vec<u8>) {
        if let Some(p) = self.focused_pane() {
            self.write(p, bytes);
        }
    }

    fn mode(&self) -> TermMode {
        self.focused_pane().and_then(|p| self.panes.get(&p)).map(|p| p.terminal.lock().mode()).unwrap_or(TermMode::empty())
    }

    pub fn paste(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        if let Some(o) = self.overlay.as_mut() {
            o.insert(&text.replace(['\r', '\n'], " "));
            self.dirty = true;
            return;
        }
        if self.rename.is_some() {
            return self.rename_text(&text.replace(['\r', '\n'], " "));
        }
        if self.settings.is_some() {
            return self.settings_text(&text.replace(['\r', '\n'], " "));
        }
        if self.search_paste(&text) {
            return;
        }
        if self.needs_paste_warning(&text, self.paste_mode()) {
            return self.ask_paste(text);
        }
        self.paste_now(&text);
    }

    fn paste_mode(&self) -> TermMode {
        let mut mode = self.mode();
        if !self.config.terminal.bracketed_paste {
            mode.remove(TermMode::BRACKETED_PASTE);
        }
        mode
    }

    pub(crate) fn paste_now(&mut self, text: &str) {
        let bytes = paste_bytes(text, self.paste_mode(), self.config.terminal.trim_whitespace_on_paste);
        self.write_focused(bytes);
    }

    fn ring(&mut self, pane: PaneId) {
        let kind = self.config.terminal.bell;
        if kind == Bell::Off {
            return;
        }
        if !self.focused {
            self.effects.push(Effect::Attention);
        }
        let now = Instant::now();
        if self.last_bell.is_some_and(|t| now.duration_since(t) < BELL_GAP) {
            return;
        }
        self.last_bell = Some(now);
        self.bell_pane = Some(pane);
        match kind {
            Bell::Audible => self.effects.push(Effect::Beep),
            _ => self.flash_bell(),
        }
    }

    #[cfg(test)]
    pub(crate) fn last_bell_reset(&mut self) {
        self.last_bell = None;
    }

    pub fn flash_bell(&mut self) {
        if let Some(pane) = self.bell_pane {
            self.bell = Some((pane, Instant::now()));
            self.dirty = true;
        }
    }

    pub(crate) fn bell_flash(&self, pane: PaneId) -> bool {
        self.bell.is_some_and(|(p, t)| p == pane && t.elapsed() < BELL_FLASH)
    }

    fn copy(&mut self) -> bool {
        let Some(p) = self.focused_pane().and_then(|p| self.panes.get(&p)) else { return false };
        let text = p.terminal.lock().selection_text();
        match text {
            Some(t) => {
                self.effects.push(Effect::SetClipboard(t));
                true
            }
            None => false,
        }
    }

    fn clear_selection(&mut self, pane: PaneId) {
        if let Some(p) = self.panes.get(&pane) {
            p.terminal.lock().clear_selection();
            self.dirty = true;
        }
    }

    fn scroll_focused(&mut self, s: Scroll) {
        if let Some(p) = self.focused_pane().and_then(|p| self.panes.get(&p)) {
            p.terminal.lock().scroll(s);
            self.dirty = true;
        }
    }

    fn navigate(&mut self, side: Side) {
        let Some(focus) = self.focused_pane() else { return };
        if let Some(next) = neighbor(&self.layout, focus, side) {
            self.focus_pane(next);
        }
    }

    fn focus_pane(&mut self, pane: PaneId) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if tab.root.contains(pane) && tab.focus != pane {
                tab.focus = pane;
                self.reset_blink();
                self.dirty = true;
            }
        }
    }

    fn step_pane(&mut self, delta: isize) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let order = tab.root.panes();
        let at = order.iter().position(|p| *p == tab.focus).unwrap_or(0) as isize;
        let next = order[(at + delta).rem_euclid(order.len() as isize) as usize];
        self.focus_pane(next);
    }

    pub fn run(&mut self, action: Action) {
        use Action::*;
        match action {
            CommandSelector => self.open_overlay(OverlayKind::Commands),
            ProfileSelector => self.open_overlay(OverlayKind::Profiles),
            Search => self.open_search(),
            SelectColorScheme => self.open_overlay(OverlayKind::Schemes),
            Settings => self.open_settings(),
            RenameTab => self.start_rename(self.active),
            CloseOtherTabs => self.close_tabs_except(|i, active| i == active),
            CloseTabsToRight => self.close_tabs_except(|i, active| i <= active),
            CloseTabsToLeft => self.close_tabs_except(|i, active| i >= active),
            ToggleFullscreen => self.effects.push(Effect::ToggleFullscreen),
            NewTab => self.open_default_tab(),
            NewWindow => self.effects.push(Effect::NewWindow),
            CloseTab => self.close_tab(self.active),
            DuplicateTab => {
                if let Some(p) = self.focused_pane().and_then(|p| self.panes.get(&p)).map(|p| p.profile.clone()) {
                    self.open_tab(p);
                }
            }
            NextTab => self.cycle(1),
            PreviousTab => self.cycle(-1),
            MoveTabLeft => self.move_tab(-1),
            MoveTabRight => self.move_tab(1),
            Tab(n) => self.activate(n as usize - 1),
            SplitRight => self.split(Side::Right),
            SplitBottom => self.split(Side::Bottom),
            SplitLeft => self.split(Side::Left),
            SplitTop => self.split(Side::Top),
            PaneNavUp => self.navigate(Side::Top),
            PaneNavDown => self.navigate(Side::Bottom),
            PaneNavLeft => self.navigate(Side::Left),
            PaneNavRight => self.navigate(Side::Right),
            PaneNavPrevious => self.step_pane(-1),
            PaneNavNext => self.step_pane(1),
            PaneMaximize => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.zoomed = if tab.zoomed.is_some() || tab.root.panes().len() < 2 { None } else { Some(tab.focus) };
                    self.dirty = true;
                }
            }
            ClosePane => {
                if let Some(p) = self.focused_pane() {
                    self.close_pane(p);
                }
            }
            Copy => {
                self.copy();
            }
            Paste => self.effects.push(Effect::RequestPaste),
            SelectAll => {
                if let Some(p) = self.focused_pane().and_then(|p| self.panes.get(&p)) {
                    p.terminal.lock().select_all();
                    self.dirty = true;
                }
            }
            Clear => {
                if let Some(p) = self.focused_pane().and_then(|p| self.panes.get(&p)) {
                    let mut t = p.terminal.lock();
                    t.feed(b"\x1b[H\x1b[2J");
                    t.clear_history();
                }
                self.write_focused(b"\x0c".to_vec());
            }
            ZoomIn => self.set_zoom(self.zoom + 1.0),
            ZoomOut => self.set_zoom(self.zoom - 1.0),
            ResetZoom => self.set_zoom(0.0),
            CtrlC => {
                if self.copy() {
                    if let Some(p) = self.focused_pane() {
                        self.clear_selection(p);
                    }
                } else {
                    self.write_focused(vec![3]);
                }
            }
            DeletePreviousWord => self.write_focused(vec![0x17]),
            DeleteNextWord => self.write_focused(b"\x1bd".to_vec()),
            DeleteLine => self.write_focused(vec![0x15]),
            InsertNewLine => self.write_focused(b"\x1b\r".to_vec()),
            ScrollToTop => self.scroll_focused(Scroll::Top),
            ScrollToBottom => self.scroll_focused(Scroll::Bottom),
            ScrollPageUp => self.scroll_focused(Scroll::PageUp),
            ScrollPageDown => self.scroll_focused(Scroll::PageDown),
            ScrollUp => self.scroll_focused(Scroll::Delta(1)),
            ScrollDown => self.scroll_focused(Scroll::Delta(-1)),
        }
    }

    fn set_zoom(&mut self, zoom: f32) {
        let base = self.config.terminal.font_size;
        self.zoom = zoom.clamp(6.0 - base, 48.0 - base);
        self.dirty = true;
    }

    fn open_overlay(&mut self, kind: OverlayKind) {
        let items = match kind {
            OverlayKind::Commands => Action::all()
                .map(|a| Pick { label: a.label(), hint: self.keymap.bindings_of(a).next().map(crate::keymap::format_binding).unwrap_or_default(), value: a.id() })
                .collect(),
            OverlayKind::Profiles => self.profiles.iter().map(|p| Pick { label: p.name.clone(), hint: String::new(), value: p.id.clone() }).collect(),
            OverlayKind::Schemes => self.scheme_names().into_iter().map(|n| Pick { label: n.clone(), hint: String::new(), value: n }).collect(),
        };
        let mut overlay = Overlay::new(kind, items);
        if kind == OverlayKind::Profiles && self.ssh.is_some() {
            overlay = overlay.with_quick_connect();
        }
        if kind == OverlayKind::Schemes {
            let current = self.current_scheme();
            if let Some(at) = overlay.shown.iter().position(|(i, _)| overlay.items[*i].value.eq_ignore_ascii_case(&current)) {
                overlay.select(at);
            }
            self.scheme_before = Some(current);
        }
        self.overlay = Some(overlay);
        self.dirty = true;
    }

    fn choose(&mut self, kind: OverlayKind, value: String) {
        self.overlay = None;
        self.dirty = true;
        match kind {
            OverlayKind::Commands => {
                if let Some(a) = Action::from_id(&value) {
                    self.run(a);
                }
            }
            OverlayKind::Profiles if value.starts_with(crate::overlay::QUICK) => {
                let target = crate::ssh::parse_target(&value[crate::overlay::QUICK.len()..]);
                if let (Some(t), Some(ssh)) = (target, self.ssh.clone()) {
                    self.open_tab(t.profile(&ssh));
                }
            }
            OverlayKind::Profiles => {
                if let Some(p) = self.profiles.iter().find(|p| p.id == value).cloned() {
                    self.open_tab(p);
                }
            }
            OverlayKind::Schemes => {
                self.scheme_before = None;
                self.set_scheme_name(value);
                self.effects.push(Effect::SaveConfig(self.config.to_text()));
            }
        }
    }

    fn preview_scheme(&mut self) {
        let Some(o) = self.overlay.as_ref().filter(|o| o.kind == OverlayKind::Schemes) else { return };
        if let Some(v) = o.selected_value() {
            if !v.eq_ignore_ascii_case(&self.current_scheme()) {
                self.set_scheme_name(v);
            }
        }
    }

    fn close_overlay(&mut self) {
        self.overlay = None;
        if let Some(name) = self.scheme_before.take() {
            self.set_scheme_name(name);
        }
        self.dirty = true;
    }

    fn overlay_key(&mut self, key: Key, mods: Mods) {
        let Some(o) = self.overlay.as_mut() else { return };
        self.dirty = true;
        match key {
            Key::Escape => return self.close_overlay(),
            Key::Up => o.step(-1),
            Key::Down => o.step(1),
            Key::PageUp => o.step(-8),
            Key::PageDown => o.step(8),
            Key::Backspace => o.backspace(mods.ctrl),
            Key::Enter => {
                if let Some(v) = o.selected_value() {
                    let kind = o.kind;
                    self.choose(kind, v);
                }
            }
            Key::Char('V') if mods.ctrl => self.effects.push(Effect::RequestPaste),
            _ => {}
        }
        self.preview_scheme();
    }

    fn reset_blink(&mut self) {
        self.blink_on = true;
        self.blink_since = Instant::now();
        self.dirty = true;
    }

    pub(crate) fn terminal_has_focus(&self) -> bool {
        self.focused && self.overlay.is_none() && self.settings.is_none() && self.menu.is_none() && self.rename.is_none() && self.paste_confirm.is_none()
    }

    fn blinking(&self) -> bool {
        self.config.terminal.cursor_blink && self.terminal_has_focus()
    }

    pub fn on_timer(&mut self) {
        self.blink_armed = false;
        if self.bell.is_some_and(|(_, t)| t.elapsed() >= BELL_FLASH) {
            self.bell = None;
            self.dirty = true;
        }
        if self.blinking() && self.blink_since.elapsed() >= BLINK {
            self.blink_on = !self.blink_on;
            self.blink_since = Instant::now();
            self.dirty = true;
        }
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Resized { width, height } => {
                self.size = (width as f32, height as f32);
                self.dirty = true;
            }
            Event::ScaleChanged(s) => {
                self.scale = s;
                self.dirty = true;
            }
            Event::Key { key, mods, pressed: true } => self.on_key(key, mods),
            Event::Key { .. } => {}
            Event::Char(c) => self.on_char(c, false),
            Event::AltChar(c) => self.on_char(c, true),
            Event::Pasted(t) => self.paste(t),
            Event::Preedit(p) => {
                self.preedit = p;
                self.dirty = true;
            }
            Event::Focus(f) => {
                self.focused = f;
                self.keymap.reset();
                self.reset_blink();
                for p in self.panes.values() {
                    p.terminal.lock().set_focus(f);
                }
                if self.mode().contains(TermMode::FOCUS_IN_OUT) {
                    self.effects.push(Effect::Write { pane: self.focused_pane().unwrap_or(0), bytes: if f { b"\x1b[I".to_vec() } else { b"\x1b[O".to_vec() } });
                }
            }
            Event::ThemeChanged { dark } => {
                self.system_dark = dark;
                self.restyle();
            }
            Event::MouseMove { x, y } => self.on_move(x, y),
            Event::MouseDown { x, y, button, clicks } => self.on_down(x, y, button, clicks),
            Event::MouseUp { x, y, button } => self.on_up(x, y, button),
            Event::Wheel { dy, .. } => self.on_wheel(dy),
            Event::MouseLeave => {
                self.mouse = (-1.0, -1.0);
                self.hover = HitArea::Client;
                self.dirty = true;
            }
            Event::DroppedFiles(paths) => {
                let text = paths.iter().map(|p| if p.contains(' ') { format!("\"{p}\"") } else { p.clone() }).collect::<Vec<_>>().join(" ");
                self.write_focused(text.into_bytes());
            }
            Event::CloseRequested | Event::SessionEnding => {
                self.close_all();
                self.effects.push(Effect::Close);
            }
            Event::Timer => self.on_timer(),
            Event::GlobalHotkey => {}
        }
    }

    fn on_key(&mut self, key: Key, mods: Mods) {
        if self.overlay.is_some() {
            self.swallow_char = false;
            self.overlay_key(key, mods);
            return;
        }
        self.swallow_char = false;
        if self.paste_confirm.is_some() {
            return self.paste_key(key);
        }
        if self.menu.is_some() {
            return self.menu_key(key);
        }
        if self.rename.is_some() {
            return self.rename_key(key, mods);
        }
        if self.settings.is_some() {
            if self.keymap.press(Chord { key, mods }) == Outcome::Action(Action::Settings) {
                return self.close_settings();
            }
            return self.settings_key(key, mods);
        }
        if key == Key::Other {
            return;
        }
        if self.search_key(key, mods) {
            return;
        }
        match self.keymap.press(Chord { key, mods }) {
            Outcome::Action(a) => {
                self.swallow_char = true;
                self.run(a);
            }
            Outcome::Pending => self.swallow_char = true,
            Outcome::None if self.search.is_some() => {}
            Outcome::None => {
                if let Some(e) = encode_key(key, mods, self.mode()) {
                    self.swallow_char = e.swallows_char;
                    self.write_focused(e.bytes);
                }
            }
        }
    }

    fn on_char(&mut self, c: char, alt: bool) {
        if std::mem::take(&mut self.swallow_char) {
            return;
        }
        if let Some(o) = self.overlay.as_mut() {
            if !c.is_control() {
                o.insert(&c.to_string());
                self.dirty = true;
            }
            self.preview_scheme();
            return;
        }
        if self.menu.is_some() || self.paste_confirm.is_some() {
            return;
        }
        if self.rename.is_some() {
            return self.rename_text(&c.to_string());
        }
        if self.settings.is_some() {
            return self.settings_text(&c.to_string());
        }
        if self.search.is_some() {
            if !alt {
                self.search_char(c);
            }
            return;
        }
        let bytes = if alt {
            encode_alt_char(c)
        } else {
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).as_bytes().to_vec()
        };
        self.write_focused(bytes);
    }

    pub(crate) fn pane_at(&self, x: f32, y: f32) -> Option<(PaneId, Rect)> {
        self.layout.panes.iter().find(|(_, r)| r.contains(x, y)).copied()
    }

    fn cell_at(&self, pane: PaneId, rect: Rect, x: f32, y: f32) -> (usize, usize, aster_vt::Side) {
        let Some(m) = self.metrics else { return (0, 0, aster_vt::Side::Left) };
        let inner = view::pane_inner(rect, self.scale);
        let size = self.panes.get(&pane).map(|p| p.size).unwrap_or(Size::new(1, 1));
        let fx = ((x - inner.x) / m.w).max(0.0);
        let col = (fx as usize).min(size.cols - 1);
        let line = (((y - inner.y) / m.h).max(0.0) as usize).min(size.rows - 1);
        let side = if fx.fract() < 0.5 { aster_vt::Side::Left } else { aster_vt::Side::Right };
        (line, col, side)
    }

    fn report(&mut self, pane: PaneId, rect: Rect, action: MouseAction, x: f32, y: f32, mods: Mods) -> bool {
        let Some(p) = self.panes.get(&pane) else { return false };
        let mode = p.terminal.lock().mode();
        if mods.shift || !mode.intersects(TermMode::MOUSE_MODE) {
            return false;
        }
        let (line, col, _) = self.cell_at(pane, rect, x, y);
        if let Some(bytes) = encode_mouse(action, col, line, mods, mode) {
            self.effects.push(Effect::Write { pane, bytes });
        }
        true
    }

    pub(crate) fn set_cursor(&mut self, c: CursorIcon) {
        if self.cursor_icon != c {
            self.cursor_icon = c;
            self.effects.push(Effect::Cursor(c));
        }
    }

    pub(crate) fn splitter_at(&self, x: f32, y: f32) -> Option<&crate::panes::Splitter> {
        let grab = (3.0 * self.scale).max(3.0);
        self.layout.splitters.iter().find(|s| s.rect.inset(-grab, -grab).contains(x, y))
    }

    fn on_move(&mut self, x: f32, y: f32) {
        self.mouse = (x, y);
        if self.menu.is_some() {
            return self.menu_hover(x, y);
        }
        let hover = self.hit_test(x, y);
        if hover != self.hover {
            self.hover = hover;
            self.dirty = true;
        }
        if self.chrome.tab_bar.contains(x, y) {
            self.dirty = true;
        }
        match self.drag.clone() {
            Drag::Select { pane } => {
                if let Some(rect) = self.layout.panes.iter().find(|(id, _)| *id == pane).map(|(_, r)| *r) {
                    let (line, col, side) = self.cell_at(pane, rect, x, y);
                    if let Some(p) = self.panes.get(&pane) {
                        let mut t = p.terminal.lock();
                        let point = t.viewport_point(line, col);
                        t.update_selection(point, side);
                    }
                    self.dirty = true;
                }
                return;
            }
            Drag::Report { pane, button } => {
                if let Some(rect) = self.layout.panes.iter().find(|(id, _)| *id == pane).map(|(_, r)| *r) {
                    self.report(pane, rect, MouseAction::Motion(Some(button)), x, y, Mods::NONE);
                }
                return;
            }
            Drag::Splitter { path, axis, span } => {
                let ratio = match axis {
                    Axis::Horizontal => (x - span.x) / span.w.max(1.0),
                    Axis::Vertical => (y - span.y) / span.h.max(1.0),
                };
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.root.set_ratio(&path, ratio);
                }
                self.dirty = true;
                return;
            }
            Drag::Tab { index, start_x, moved } => {
                let threshold = 6.0 * self.scale;
                if moved || (x - start_x).abs() > threshold {
                    self.drag = Drag::Tab { index, start_x, moved: true };
                    if let Some(target) = self.chrome.tabs.iter().position(|t| t.rect.contains(x, t.rect.y + 1.0)) {
                        if target != index {
                            let tab = self.tabs.remove(index);
                            self.tabs.insert(target, tab);
                            self.active = target;
                            self.drag = Drag::Tab { index: target, start_x, moved: true };
                        }
                    }
                    self.dirty = true;
                }
                return;
            }
            _ => {}
        }
        if self.settings.is_some() && self.chrome.content.contains(x, y) {
            self.dirty = true;
            self.hover_link = None;
            self.set_cursor(CursorIcon::Default);
            return;
        }
        let link = self.link_at(x, y);
        if link != self.hover_link {
            self.hover_link = link;
            self.dirty = true;
        }
        let icon = if let Some(s) = self.splitter_at(x, y) {
            if s.axis == Axis::Horizontal {
                CursorIcon::ResizeHorizontal
            } else {
                CursorIcon::ResizeVertical
            }
        } else if self.hover_link.is_some() {
            CursorIcon::Hand
        } else if self.overlay.is_none() && self.pane_at(x, y).is_some() {
            CursorIcon::Text
        } else {
            CursorIcon::Default
        };
        self.set_cursor(icon);
        if let Some((pane, rect)) = self.pane_at(x, y) {
            self.report(pane, rect, MouseAction::Motion(None), x, y, Mods::NONE);
        }
    }

    fn link_at(&self, x: f32, y: f32) -> Option<HoverLink> {
        if self.overlay.is_some() || !matches!(self.drag, Drag::None) {
            return None;
        }
        let (pane, rect) = self.pane_at(x, y)?;
        let (line, col, _) = self.cell_at(pane, rect, x, y);
        let p = self.panes.get(&pane)?;
        let (text, cols) = {
            let t = p.terminal.lock();
            if t.mode().intersects(TermMode::MOUSE_MODE) {
                return None;
            }
            t.line_text(line)
        };
        let at = cols.iter().position(|c| *c >= col)?;
        let link = crate::links::find_links(&text).into_iter().find(|l| (l.start..l.end).contains(&at))?;
        let start = cols[link.start];
        let end = cols.get(link.end).copied().unwrap_or_else(|| cols.last().map_or(0, |c| c + 1));
        Some(HoverLink { pane, line, start, end, target: link.target })
    }

    fn click_count(&mut self, x: f32, y: f32, clicks: u8) -> u8 {
        let now = Instant::now();
        let near = |px: f32, py: f32| (px - x).abs() < 4.0 * self.scale && (py - y).abs() < 4.0 * self.scale;
        let n = match self.last_click {
            Some((t, px, py, n)) if now.duration_since(t) < Duration::from_millis(500) && near(px, py) && clicks >= 2 => (n + 1).clamp(2, 3),
            Some((t, px, py, n)) if now.duration_since(t) < Duration::from_millis(500) && near(px, py) && n >= 2 => 3,
            _ => clicks.max(1),
        };
        self.last_click = Some((now, x, y, n));
        n
    }

    fn on_down(&mut self, x: f32, y: f32, button: MouseButton, clicks: u8) {
        self.mouse = (x, y);
        self.dirty = true;
        if let Some(o) = self.overlay.as_ref() {
            match view::overlay_hit(self, o, x, y) {
                Some(Some(i)) => {
                    let o = self.overlay.as_mut().expect("overlay");
                    o.selected = i;
                    if let Some(v) = o.selected_value() {
                        let kind = o.kind;
                        self.choose(kind, v);
                    }
                }
                Some(None) => {}
                None => self.close_overlay(),
            }
            return;
        }
        if self.paste_confirm.is_some() {
            return self.paste_click(x, y);
        }
        if self.menu.is_some() {
            return self.menu_click(x, y);
        }
        if let Some(r) = self.rename.as_ref() {
            let inside = self.chrome.tabs.get(r.tab).is_some_and(|t| t.rect.contains(x, y));
            if !inside {
                self.commit_rename();
            } else if button == MouseButton::Left {
                return;
            }
        }
        if self.settings.is_some() && self.chrome.content.contains(x, y) {
            if button == MouseButton::Left {
                self.settings_click(x, y);
            }
            return;
        }
        if button == MouseButton::Left && self.search_click(x, y) {
            return;
        }
        let hit = self.hit_test(x, y);
        if matches!(hit, HitArea::Minimize | HitArea::Maximize | HitArea::Close) {
            self.drag = Drag::Button(hit);
            return;
        }
        if self.chrome.tab_bar.contains(x, y) {
            if self.chrome.plus.contains(x, y) {
                self.drag = Drag::Plus;
                return;
            }
            if let Some(i) = self.chrome.tabs.iter().position(|t| t.rect.contains(x, y)) {
                match button {
                    MouseButton::Middle => self.close_tab(i),
                    MouseButton::Left if self.chrome.tabs[i].close.contains(x, y) => self.drag = Drag::TabClose(i),
                    MouseButton::Left if self.click_count(x, y, clicks) >= 2 => self.start_rename(i),
                    MouseButton::Left => {
                        self.activate(i);
                        self.drag = Drag::Tab { index: i, start_x: x, moved: false };
                    }
                    MouseButton::Right => self.open_tab_menu(i, x, y),
                    _ => {}
                }
            }
            return;
        }
        if let Some(s) = self.splitter_at(x, y) {
            self.drag = Drag::Splitter { path: s.path.clone(), axis: s.axis, span: s.span };
            return;
        }
        let Some((pane, rect)) = self.pane_at(x, y) else { return };
        self.focus_pane(pane);
        let mods = Mods::NONE;
        if self.report(pane, rect, MouseAction::Press(button), x, y, mods) {
            self.drag = Drag::Report { pane, button };
            return;
        }
        match button {
            MouseButton::Left => {
                let n = self.click_count(x, y, clicks);
                let (line, col, side) = self.cell_at(pane, rect, x, y);
                let ty = match n {
                    1 => SelectionType::Simple,
                    2 => SelectionType::Semantic,
                    _ => SelectionType::Lines,
                };
                if let Some(p) = self.panes.get(&pane) {
                    let mut t = p.terminal.lock();
                    let point = t.viewport_point(line, col);
                    t.start_selection(ty, point, side);
                    if n > 1 {
                        t.update_selection(point, side);
                    }
                }
                self.drag = Drag::Select { pane };
            }
            MouseButton::Right => match self.config.terminal.right_click {
                RightClick::Menu => self.open_terminal_menu(x, y),
                RightClick::Clipboard => {
                    if self.copy() {
                        self.clear_selection(pane);
                    } else {
                        self.effects.push(Effect::RequestPaste);
                    }
                }
                RightClick::Paste => self.effects.push(Effect::RequestPaste),
                RightClick::Off => {}
            },
            MouseButton::Middle if self.config.terminal.paste_on_middle_click => self.effects.push(Effect::RequestPaste),
            _ => {}
        }
    }

    fn on_up(&mut self, x: f32, y: f32, button: MouseButton) {
        let drag = std::mem::replace(&mut self.drag, Drag::None);
        self.dirty = true;
        match drag {
            Drag::Select { pane } => {
                let has = self.panes.get(&pane).is_some_and(|p| p.terminal.lock().has_selection());
                if !has {
                    self.clear_selection(pane);
                    if let Some(link) = self.link_at(x, y).filter(|l| self.hover_link.as_ref() == Some(l)) {
                        self.effects.push(Effect::Open(open_target(&link.target)));
                    }
                } else if self.config.terminal.copy_on_select {
                    self.copy();
                }
            }
            Drag::Report { pane, .. } => {
                if let Some(rect) = self.layout.panes.iter().find(|(id, _)| *id == pane).map(|(_, r)| *r) {
                    self.report(pane, rect, MouseAction::Release(button), x, y, Mods::NONE);
                }
            }
            Drag::Button(area) if self.hit_test(x, y) == area => self.effects.push(match area {
                HitArea::Minimize => Effect::Minimize,
                HitArea::Maximize => Effect::ToggleMaximize,
                _ => Effect::Close,
            }),
            Drag::TabClose(i) if self.chrome.tabs.get(i).is_some_and(|t| t.close.contains(x, y)) => self.close_tab(i),
            Drag::Plus if self.chrome.plus.contains(x, y) => self.open_default_tab(),
            _ => {}
        }
    }

    fn on_wheel(&mut self, dy: f32) {
        let (x, y) = self.mouse;
        if self.menu.is_some() {
            return self.close_menu();
        }
        if let Some(o) = self.overlay.as_mut() {
            o.step(if dy > 0.0 { -3 } else { 3 });
            self.dirty = true;
            return;
        }
        if self.settings.is_some() {
            return self.settings_wheel(dy);
        }
        let Some((pane, rect)) = self.pane_at(x, y) else { return };
        let action = if dy > 0.0 { MouseAction::WheelUp } else { MouseAction::WheelDown };
        if self.report(pane, rect, action, x, y, Mods::NONE) {
            return;
        }
        let Some(p) = self.panes.get_mut(&pane) else { return };
        p.scroll_acc += dy * 3.0;
        let lines = p.scroll_acc.trunc() as i32;
        if lines == 0 {
            return;
        }
        p.scroll_acc -= lines as f32;
        let mode = p.terminal.lock().mode();
        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            let seq: &[u8] = if lines > 0 { if mode.contains(TermMode::APP_CURSOR) { b"\x1bOA" } else { b"\x1b[A" } } else if mode.contains(TermMode::APP_CURSOR) { b"\x1bOB" } else { b"\x1b[B" };
            let bytes = seq.repeat(lines.unsigned_abs() as usize);
            self.effects.push(Effect::Write { pane, bytes });
        } else {
            p.terminal.lock().scroll(Scroll::Delta(lines));
            self.dirty = true;
        }
    }

    pub fn hit_test(&self, x: f32, y: f32) -> HitArea {
        let c = &self.chrome;
        if c.min.contains(x, y) {
            HitArea::Minimize
        } else if c.max.contains(x, y) {
            HitArea::Maximize
        } else if c.close.contains(x, y) {
            HitArea::Close
        } else if c.tab_bar.contains(x, y) && !c.plus.contains(x, y) && !c.tabs.iter().any(|t| t.rect.contains(x, y)) {
            HitArea::Caption
        } else {
            HitArea::Client
        }
    }

    pub fn woken(&mut self) {
        while let Ok((pane, ev)) = self.events.try_recv() {
            match ev {
                VtEvent::Write(bytes) => {
                    if self.panes.contains_key(&pane) {
                        self.effects.push(Effect::Write { pane, bytes });
                    }
                }
                VtEvent::Title(t) => {
                    if let Some(p) = self.panes.get_mut(&pane) {
                        p.title = Some(t).filter(|t| !t.trim().is_empty() && !is_program_path(t));
                    }
                }
                VtEvent::ResetTitle => {
                    if let Some(p) = self.panes.get_mut(&pane) {
                        p.title = None;
                    }
                }
                VtEvent::Bell => self.ring(pane),
                VtEvent::Clipboard(text) => self.effects.push(Effect::SetClipboard(text)),
            }
        }
        self.dirty = true;
    }

    pub fn exited(&mut self, pane: PaneId, code: Option<u32>) {
        let Some(p) = self.panes.get_mut(&pane) else { return };
        if p.exited.is_some() {
            return;
        }
        p.exited = Some(code);
        if code == Some(0) {
            self.close_pane(pane);
        } else {
            let text = match code {
                Some(c) => format!("\r\n\x1b[0m\x1b[2m[Process exited with code {c}. Press any key to close.]\x1b[0m\r\n"),
                None => "\r\n\x1b[0m\x1b[2m[Process exited. Press any key to close.]\x1b[0m\r\n".to_owned(),
            };
            p.terminal.lock().feed(text.as_bytes());
            self.dirty = true;
        }
    }

    pub fn spawn_failed(&mut self, pane: PaneId, message: &str) {
        if let Some(p) = self.panes.get_mut(&pane) {
            p.terminal.lock().feed(format!("\x1b[31m{message}\x1b[0m\r\n").as_bytes());
            p.exited = Some(None);
            self.dirty = true;
        }
    }

    pub fn needs_redraw(&self) -> bool {
        self.dirty
    }

    pub(crate) fn mark_drawn(&mut self) {
        self.dirty = false;
        let title = self.tab_title(self.active);
        let title = if title.is_empty() { "Aster".to_owned() } else { title };
        if title != self.title {
            self.title = title.clone();
            self.effects.push(Effect::Title(title));
        }
        if self.blinking() && !self.blink_armed {
            self.blink_armed = true;
            let left = BLINK.saturating_sub(self.blink_since.elapsed()).as_millis().max(16) as u32;
            self.effects.push(Effect::Timer(left));
        }
        if let Some((_, t)) = self.bell {
            let left = BELL_FLASH.saturating_sub(t.elapsed()).as_millis().max(16) as u32;
            self.effects.push(Effect::Timer(left));
            self.blink_armed = false;
        }
    }

    pub(crate) fn set_ime_area(&mut self, r: Rect) {
        if r != self.ime_area {
            self.ime_area = r;
            self.effects.push(Effect::ImeArea(r));
        }
    }

    pub(crate) fn push(&mut self, e: Effect) {
        self.effects.push(e);
    }

    pub(crate) fn push_save_config(&mut self) {
        let text = self.config.to_text();
        self.effects.push(Effect::SaveConfig(text));
    }

    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HoverLink {
    pub pane: PaneId,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub target: String,
}

pub fn is_program_path(title: &str) -> bool {
    let t = title.trim().to_ascii_lowercase();
    let bytes = t.as_bytes();
    bytes.len() > 3 && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/') && t.ends_with(".exe")
}

pub fn open_target(target: &str) -> String {
    if target.contains("://") {
        return target.to_owned();
    }
    let mut t = target;
    for _ in 0..2 {
        if let Some((head, tail)) = t.rsplit_once(':') {
            if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) && head.len() > 1 {
                t = head;
            }
        }
    }
    t.to_owned()
}

pub fn inherit_cwd(profile: &Profile, inherited: Option<String>, home: Option<String>) -> (Vec<String>, Option<String>) {
    let mut args = profile.args.clone();
    let Some(dir) = inherited else { return (args, profile.cwd.clone().or(home)) };
    if dir.starts_with('/') {
        if let Some(i) = args.iter().position(|a| a == "--cd") {
            if let Some(next) = args.get_mut(i + 1) {
                *next = dir;
                return (args, profile.cwd.clone().or(home));
            }
        }
        return (args, profile.cwd.clone().or(home));
    }
    (args, Some(dir))
}

#[cfg(test)]
#[path = "../test/app.rs"]
mod tests;
