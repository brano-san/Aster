use crate::config::{Bell, Config, CursorStyle, LinkModifier, RightClick, SchemeMode};
use crate::input::{CursorIcon, Event, HitArea, Key, Mods, MouseButton, Preedit};
use crate::keymap::{Action, Chord, Keymap, Outcome};
use crate::keys::{encode_alt_char, encode_key, encode_mouse, paste_bytes, MouseAction};
use crate::overlay::{Overlay, OverlayKind, Pick};
use crate::panes::{neighbor, Axis, Layout, Node, PaneId, Side};
use crate::profile::{ExitDecision, OnExit, Profile};
use crate::theme::Theme;
use crate::view::{self, Chrome};
use aster_gfx::render::Rect;
use aster_vt::schemes::{self, Scheme};
use aster_vt::{CursorShape, FairMutex, Options, Palette, Scroll, SelectionType, Sink, Size, TermMode, Terminal, VtEvent};
pub use aster_vt::{Progress, ProgressState};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub type Shared = Arc<FairMutex<Terminal>>;
pub type Wake = Arc<dyn Fn() + Send + Sync>;
pub type CwdProbe = Arc<dyn Fn(u32) -> Option<String> + Send + Sync>;
pub type ChildrenProbe = Arc<dyn Fn(u32) -> Vec<String> + Send + Sync>;

pub use crate::cursor_motion::BLINK;
const BELL_FLASH: Duration = Duration::from_millis(150);
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const CLOSED_TABS: usize = 10;
pub(crate) const SETTINGS_PANE: PaneId = PaneId::MAX;

pub const TAB_COLORS: [(&str, &str); 9] = [("No color", ""), ("Red", "#E5534B"), ("Orange", "#E0823D"), ("Yellow", "#D4B13B"), ("Green", "#57AB5A"), ("Teal", "#39B6A8"), ("Blue", "#4C8DF6"), ("Purple", "#9D6CE0"), ("Pink", "#D9619F")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Broadcast {
    Tab,
    AllTabs,
}
pub(crate) const TOAST: Duration = Duration::from_millis(1200);
const BELL_GAP: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub delegation: Option<String>,
    pub elevated: bool,
    pub ssh: Option<String>,
    pub ssh_options: Option<Box<crate::profile::SshOptions>>,
    pub serial: Option<String>,
    pub socket: Option<String>,
    pub stream: Option<crate::profile::Stream>,
    pub clear_after_connect: Option<bool>,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub local_environment: Option<Box<crate::environment::LocalEnvironment>>,
    pub cols: u16,
    pub rows: u16,
}

impl Launch {
    pub(crate) fn baud_placeholder(&self) -> Option<usize> {
        let head = self.serial.as_deref()?.split(',').next()?.trim_end();
        let (_, baud) = head.rsplit_once(':')?;
        (baud.trim_start() == "?").then(|| head.len() - 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerialControl {
    Baud(u32),
    Break,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneTransfer {
    #[default]
    None,
    AwaitingFiles,
    Incoming,
    Running,
}

pub enum Effect {
    Spawn { pane: PaneId, launch: Launch, terminal: Shared, reconnecting: bool },
    SerialControl { pane: PaneId, control: SerialControl },
    Write { pane: PaneId, bytes: Vec<u8> },
    Resize { pane: PaneId, cols: u16, rows: u16 },
    Kill { pane: PaneId, host_id: Option<u64> },
    SetClipboard(String),
    SetClipboardHtml { text: String, html: String },
    RequestPaste,
    RequestVaultPaste(u64),
    RequestConfigPaste(u64),
    SaveConfig(String),
    ConfigSync(crate::sync::Operation),
    SaveRawConfig { text: String, expected: Option<String> },
    Restart,
    Open(String),
    OpenLog,
    StartMenuShortcut(bool),
    InstallForUser,
    DefaultTerminal(bool),
    SaveOutput { pane: PaneId },
    ExportBuffer { pane: PaneId, text: String },
    RecentProfiles(Vec<String>),
    OpenConfigFile,
    RevealConfigFile,
    CheckConfigFile,
    OpenConfigFolder,
    ShowDefaultConfig,
    ApplicationsMenu(bool),
    Title(String),
    Cursor(CursorIcon),
    Minimize,
    ToggleMaximize,
    ToggleFullscreen,
    Close,
    Quit,
    NewWindow,
    Timer(u32),
    Attention,
    Notify(crate::notifications::Notification),
    ProbeCommand { request: u64, pids: Vec<u32> },
    Beep,
    Surface(crate::config::Backdrop),
    LoadBackgroundImage { revision: u64, path: String },
    Progress(Option<Progress>),
    Files { pane: PaneId, request: crate::files::FileRequest },
    SendFiles { pane: PaneId, files: Vec<std::path::PathBuf> },
    CancelTransfer { pane: PaneId },
    AcceptTransfer { pane: PaneId },
    CheckSecret { pane: PaneId, key: String },
    FillSecret { pane: PaneId, key: String },
    SaveSecret { key: String, value: String },
    SshPassword { request: u64, destination: String, options: Box<crate::profile::SshOptions>, value: Option<String> },
    BrowseSshKeys { request: u64, directory: Option<std::path::PathBuf>, import: bool },
    SshInteractive { pane: PaneId, answer: aster_proto::ssh::InteractiveAnswer },
    SshForward { pane: PaneId, command: aster_proto::ssh_forward::Command },
    ImportSshIdentity { request: u64, path: std::path::PathBuf },
    VaultCreate { passphrase: String },
    VaultUnlock { passphrase: String },
    VaultLock,
    VaultChange { old: String, new: String },
    VaultErase { passphrase: Option<String> },
    VaultManage { action: crate::vault_settings::VaultAction, passphrase: Option<String>, request: u64 },
    UnlockConfig { request: u64, passphrase: String },
    CancelConfigUnlock,
    VaultAccessInput(Option<String>),
    ProtectConfig { request: u64, enabled: bool, passphrase: String },
    ImeArea(Rect),
    Update(crate::update::UpdateRequest),
    Extension(crate::extensions::Request),
}

pub(crate) struct Pane {
    pub terminal: Shared,
    pub source_profile: Profile,
    pub profile: Profile,
    pub title: Option<String>,
    pub name: Option<String>,
    pub exited: Option<Option<u32>>,
    pub pending: Option<Launch>,
    pub retry_launch: Option<Box<Launch>>,
    pub ssh_connecting: bool,
    reconnect_at: Option<Instant>,
    generation: u64,
    pub queued: Vec<u8>,
    pub pid: Option<u32>,
    pub host_id: Option<u64>,
    pub restore_buffer: Option<Vec<String>>,
    pub size: Size,
    pub scroll_acc: f32,
    pub seen: u64,
    pub activity_seq: u64,
}

pub(crate) struct Tab {
    pub key: u64,
    pub command_watch: crate::close::CommandWatch,
    pub activity: crate::notifications::Activity,
    pub pinned: bool,
    pub root: Node,
    pub focus: PaneId,
    pub zoomed: Option<PaneId>,
    pub title: Option<String>,
    pub color: Option<String>,
}

impl Tab {
    pub(crate) fn new(root: Node, focus: PaneId) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self { key: NEXT.fetch_add(1, Ordering::Relaxed), root, focus, zoomed: None, title: None, color: None, pinned: false, command_watch: Default::default(), activity: crate::notifications::Activity::new(Instant::now()) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Drag {
    None,
    Select { pane: PaneId, open_link: bool },
    Report { pane: PaneId, button: MouseButton },
    Splitter { path: Vec<bool>, axis: Axis, span: Rect },
    Tab { index: usize, start: (f32, f32), grab: (f32, f32), moved: bool },
    Pane { pane: PaneId, start: (f32, f32), moved: bool },
    Button(HitArea),
    TabClose(usize),
    TabMenu,
    SettingsButton,
    SidebarResize { width: f32 },
    Plus,
    SettingsSlider { item: usize, track: Rect, x: f32 },
    SettingsScrollbar { page: usize, grab: f32 },
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
    pub(crate) restart_baseline: crate::settings::RestartSettings,
    pub(crate) profiles: Vec<Profile>,
    detected: Vec<Profile>,
    schemes: Vec<Scheme>,
    extension_color_schemes: Vec<(Scheme, String)>,
    scheme_before: Option<String>,
    pub(crate) keymap: Keymap,
    pub(crate) palette: Palette,
    pub(crate) background: crate::background::State,
    profile_palettes: std::cell::RefCell<HashMap<String, Palette>>,
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
    system_animations: bool,
    pub(crate) effects: Vec<Effect>,
    pub(crate) extensions: std::sync::Arc<crate::extensions::Snapshot>,
    pub(crate) dirty: bool,
    pub(crate) motion: bool,
    pub(crate) layers: crate::layers::Layers,
    pub(crate) busy: crate::busy::Busy,
    last_frame: Option<Instant>,
    events: Receiver<(PaneId, u64, VtEvent)>,
    sender: Sender<(PaneId, u64, VtEvent)>,
    wake: Wake,
    home: Option<String>,
    ssh: Option<String>,
    cwd_probe: Option<CwdProbe>,
    pub(crate) children_probe: Option<ChildrenProbe>,
    pub(crate) close_confirm: Option<crate::close::CloseConfirm>,
    pub(crate) closing: Option<crate::session::WindowState>,
    pub(crate) mouse: (f32, f32),
    pub(crate) drag: Drag,
    last_click: Option<(Instant, f32, f32, u8)>,
    pub(crate) hover: HitArea,
    pub(crate) hover_state: crate::hover::HoverState,
    pub(crate) tab_strip: crate::tab_motion::TabStrip,
    pub(crate) hover_link: Option<HoverLink>,
    pub(crate) overlay: Option<Overlay>,
    pub(crate) settings: Option<crate::settings::SettingsPage>,
    pub(crate) config_path: Option<String>,
    pub(crate) config_status: Option<Result<(), String>>,
    pub(crate) launcher_installed: Option<bool>,
    pub(crate) default_terminal: crate::default_terminal::State,
    pub(crate) start_menu_shortcut: bool,
    pub(crate) installation: crate::installation::State,
    pub(crate) config_encrypted: bool,
    pub(crate) vault_prompt: Option<crate::vault_settings::VaultDialog>,
    pub(crate) config_editor: crate::config_editor::ConfigEditor,
    pub(crate) vault: crate::settings::VaultView,
    pub(crate) sync_view: crate::sync::View,
    pub(crate) vault_input_id: u64,
    pub(crate) ssh_editor_request: u64,
    pub(crate) ssh_auth: BTreeMap<PaneId, crate::ssh_auth::Panel>,
    pub(crate) ssh_forwards: BTreeMap<PaneId, Vec<aster_proto::ssh_forward::ForwardStatus>>,
    pub(crate) ssh_forward_form: Option<crate::ssh_forward_form::ForwardForm>,
    pub(crate) ssh_auth_paste: Option<(u64, PaneId, u64, usize)>,
    pub(crate) ssh_algorithm_catalog: aster_proto::ssh::AlgorithmCatalog,
    pub(crate) menu: Option<crate::menu::Menu>,
    pub(crate) rename: Option<crate::tabs::Rename>,
    pub(crate) pane_rename: Option<crate::pane_header::Rename>,
    pub(crate) pane_profile_probe: Option<crate::pane_header::ProfileProbe>,
    pub(crate) profile_sidebar: crate::profile_sidebar::ProfileSidebar,
    pub(crate) panel_motion: crate::panel_motion::Panels,
    pub(crate) control_motion: crate::control_motion::Controls,
    pub(crate) list_motion: crate::list_motion::Lists,
    pub(crate) rearrange: Option<crate::rearrange::Rearrange>,
    pub(crate) paste_confirm: Option<crate::paste::PasteConfirm>,
    pub(crate) files: Option<crate::files::FilesPage>,
    pub(crate) transfers: Vec<crate::files::Transfer>,
    pub(crate) transfer_seq: u64,
    pub(crate) last_dirs: HashMap<PaneId, String>,
    pub(crate) offers: HashMap<PaneId, (PaneTransfer, String)>,
    pub(crate) prompts: HashMap<PaneId, crate::sudo::Prompt>,
    can_elevate: bool,
    pub(crate) bell: Option<(PaneId, Instant)>,
    last_bell: Option<Instant>,
    bell_pane: Option<PaneId>,
    translucent: bool,
    backdrop: bool,
    pub(crate) search: Option<crate::search::SearchBar>,
    swallow_char: bool,
    pub(crate) swallow_chord: Option<Key>,
    mouse_mods: Mods,
    modifier_gesture: shortcut_input::ModifierGesture,
    pub(crate) closed_tabs: Vec<crate::session::TabState>,
    last_tab: Option<PaneId>,
    pub(crate) mru: crate::tabs::MruTabs,
    current_tab: Option<PaneId>,
    pub(crate) broadcast: Option<Broadcast>,
    pub(crate) recent: Vec<String>,
    pub(crate) start_selected: usize,
    pub(crate) tab_bar_override: Option<bool>,
    fullscreen: bool,
    pub(crate) tab_toast: Option<crate::notices::Toast>,
    pub(crate) update_notice: Option<crate::update::UpdateNotice>,
    pub(crate) monospace_fonts: Vec<String>,
    pub(crate) all_fonts: Vec<String>,
    pub(crate) background_launches: Vec<PaneId>,
    pub(crate) profile_collapsed: std::collections::BTreeSet<crate::profiles_page::Header>,
    pub(crate) settings_stash: Option<crate::settings::SettingsPage>,
    pub(crate) shown_progress: Option<Progress>,
    swallow_control: bool,
    blink: crate::cursor_motion::Blink,
    timer_due: Option<Instant>,
    pub(crate) metrics: Option<Metrics>,
    pub(crate) layout: Layout,
    pub(crate) pane_motion: crate::pane_motion::PaneMotion,
    pub(crate) rearrange_motion: crate::pane_motion::RearrangeMotion,
    pub(crate) chrome: Chrome,
    pub(crate) preedit: Preedit,
    title: String,
    cursor_icon: CursorIcon,
    ime_area: Rect,
    system_language: String,
}

impl App {
    pub fn new(config: Config, detected: Vec<Profile>, system_dark: bool, home: Option<String>, wake: Wake) -> App {
        let mut profiles = detected.clone();
        profiles.extend(config.profiles.iter().cloned());
        let (sender, events) = channel();
        let keymap = Keymap::with_mru(&config.hotkeys, config.appearance.mru_tabs);
        let mut app = App {
            vault: crate::settings::VaultView { configured: !config.vault.sealed.is_empty(), ..Default::default() },
            sync_view: crate::sync::View::default(),
            vault_input_id: 0,
            ssh_editor_request: 0,
            ssh_auth: BTreeMap::new(),
            ssh_forwards: BTreeMap::new(),
            ssh_forward_form: None,
            ssh_auth_paste: None,
            ssh_algorithm_catalog: Default::default(),
            restart_baseline: crate::settings::RestartSettings::from_config(&config),
            config,
            profiles,
            detected,
            schemes: schemes::builtin(),
            scheme_before: None,
            keymap,
            palette: Palette::default(),
            background: Default::default(),
            profile_palettes: Default::default(),
            extension_color_schemes: Vec::new(),
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
            system_animations: true,
            effects: Vec::new(),
            extensions: Default::default(),
            dirty: true,
            motion: false,
            layers: crate::layers::Layers::default(),
            busy: crate::busy::Busy::default(),
            last_frame: None,
            events,
            sender,
            wake,
            home,
            ssh: None,
            cwd_probe: None,
            children_probe: None,
            close_confirm: None,
            closing: None,
            mouse: (-1.0, -1.0),
            drag: Drag::None,
            last_click: None,
            hover: HitArea::Client,
            hover_state: Default::default(),
            tab_strip: Default::default(),
            hover_link: None,
            overlay: None,
            settings: None,
            config_path: None,
            config_status: None,
            launcher_installed: None,
            default_terminal: Default::default(),
            start_menu_shortcut: false,
            installation: Default::default(),
            config_encrypted: false,
            vault_prompt: None,
            config_editor: crate::config_editor::ConfigEditor::default(),
            menu: None,
            rename: None,
            pane_rename: None,
            pane_profile_probe: None,
            profile_sidebar: Default::default(),
            panel_motion: Default::default(),
            control_motion: Default::default(),
            list_motion: Default::default(),
            rearrange: None,
            paste_confirm: None,
            files: None,
            transfers: Vec::new(),
            transfer_seq: 0,
            last_dirs: HashMap::new(),
            offers: HashMap::new(),
            prompts: HashMap::new(),
            can_elevate: false,
            bell: None,
            last_bell: None,
            bell_pane: None,
            translucent: false,
            backdrop: false,
            search: None,
            swallow_char: false,
            swallow_chord: None,
            mouse_mods: Mods::NONE,
            modifier_gesture: Default::default(),
            closed_tabs: Vec::new(),
            last_tab: None,
            mru: crate::tabs::MruTabs::default(),
            current_tab: None,
            broadcast: None,
            recent: Vec::new(),
            start_selected: 0,
            tab_bar_override: None,
            fullscreen: false,
            tab_toast: None,
            update_notice: None,
            monospace_fonts: Vec::new(),
            all_fonts: Vec::new(),
            settings_stash: None,
            background_launches: Vec::new(),
            profile_collapsed: Default::default(),
            shown_progress: None,
            swallow_control: false,
            blink: crate::cursor_motion::Blink::new(Instant::now()),
            timer_due: None,
            metrics: None,
            layout: Layout::default(),
            pane_motion: Default::default(),
            rearrange_motion: Default::default(),
            chrome: Chrome::default(),
            preedit: Preedit::default(),
            title: String::new(),
            cursor_icon: CursorIcon::Default,
            ime_area: Rect::default(),
            system_language: "en".to_owned(),
        };
        app.restyle();
        app.apply_language();
        app.sync_profile_sidebar();
        app.effects.push(Effect::Surface(app.config.appearance.backdrop));
        app.request_background_image();
        app
    }

    pub fn set_surface(&mut self, translucent: bool, backdrop: bool) {
        if (self.translucent, self.backdrop) != (translucent, backdrop) {
            self.translucent = translucent;
            self.backdrop = backdrop;
            self.restyle();
        }
    }

    pub fn transfer_offered(&mut self, pane: PaneId, state: PaneTransfer) {
        if state == PaneTransfer::None {
            self.offers.remove(&pane);
        } else if self.transfer(pane) != state {
            self.offers.insert(pane, (state, String::new()));
        }
        self.dirty = true;
    }

    pub fn transfer(&self, pane: PaneId) -> PaneTransfer {
        self.offers.get(&pane).map_or(PaneTransfer::None, |o| o.0)
    }

    pub fn offer(&self, pane: PaneId) -> Option<&str> {
        self.offers.get(&pane).filter(|o| o.0 == PaneTransfer::AwaitingFiles).map(|o| o.1.as_str())
    }

    fn offered_pane(&self) -> Option<PaneId> {
        self.focused_pane().filter(|p| self.offers.contains_key(p))
    }

    fn awaiting_pane(&self) -> Option<PaneId> {
        self.offered_pane().filter(|p| self.transfer(*p) == PaneTransfer::AwaitingFiles)
    }

    fn offer_key(&mut self, pane: PaneId, key: Key, mods: Mods) {
        let interrupt = key == Key::Escape || (matches!(key, Key::Char('C' | 'c')) && mods.ctrl && !mods.shift && !mods.alt);
        match self.transfer(pane) {
            PaneTransfer::Incoming => {
                if interrupt {
                    self.offers.remove(&pane);
                    self.effects.push(Effect::CancelTransfer { pane });
                    self.dirty = true;
                } else if key == Key::Enter {
                    self.offers.insert(pane, (PaneTransfer::Running, String::new()));
                    self.effects.push(Effect::AcceptTransfer { pane });
                    self.dirty = true;
                }
                return;
            }
            PaneTransfer::Running => {
                if interrupt {
                    self.offers.remove(&pane);
                    self.effects.push(Effect::CancelTransfer { pane });
                    self.dirty = true;
                }
                return;
            }
            PaneTransfer::AwaitingFiles | PaneTransfer::None => {}
        }
        self.dirty = true;
        match key {
            Key::Escape => {
                self.offers.remove(&pane);
                self.effects.push(Effect::CancelTransfer { pane });
            }
            Key::Enter => {
                let path = self.offers.get(&pane).map(|p| p.1.trim().trim_matches('"').to_owned()).unwrap_or_default();
                if !path.is_empty() {
                    self.effects.push(Effect::SendFiles { pane, files: vec![std::path::PathBuf::from(path)] });
                    if let Some(p) = self.offers.get_mut(&pane) {
                        p.1.clear();
                    }
                }
            }
            Key::Backspace => {
                if let Some((_, p)) = self.offers.get_mut(&pane) {
                    if mods.ctrl {
                        p.clear();
                    } else {
                        p.pop();
                    }
                }
            }
            Key::Char('V') if mods.ctrl => self.effects.push(Effect::RequestPaste),
            _ => {}
        }
    }

    pub fn set_children_probe(&mut self, probe: ChildrenProbe) {
        self.children_probe = Some(probe);
    }

    pub(crate) fn close_tab_asking(&mut self, index: usize) {
        if let Some(rep) = self.tabs.get(index).map(|t| t.focus) {
            self.request_close(crate::close::CloseTarget::Tabs(vec![rep]));
        }
    }

    pub fn set_cwd_probe(&mut self, probe: CwdProbe) {
        self.cwd_probe = Some(probe);
    }

    pub fn set_pane_pid(&mut self, pane: PaneId, pid: u32) {
        if let Some(p) = self.panes.get_mut(&pane) {
            p.pid = Some(pid);
            self.dirty |= std::mem::take(&mut p.ssh_connecting);
        }
    }

    pub(crate) fn pane_connecting(&self, id: PaneId) -> bool {
        self.panes.get(&id).is_some_and(|pane| pane.ssh_connecting || (pane.profile.ssh.is_some() && pane.reconnect_at.is_some()))
    }

    pub(crate) fn pane_cwd(&self, id: PaneId) -> Option<String> {
        let pane = self.panes.get(&id)?;
        if let Some(dir) = pane.terminal.lock().cwd() {
            return Some(dir.to_owned());
        }
        let remote = !pane.profile.is_local() || pane.profile.id.starts_with("ssh:") || pane.profile.is_wsl();
        if remote {
            return None;
        }
        let (probe, pid) = (self.cwd_probe.as_ref()?, pane.pid?);
        probe(pid)
    }

    pub fn set_system_language(&mut self, code: &str) {
        self.system_language = code.to_owned();
        self.apply_language();
    }

    pub fn system_language(&self) -> &str {
        &self.system_language
    }

    fn apply_language(&mut self) {
        let code = crate::i18n::resolve(&self.config.appearance.language, &self.system_language);
        crate::i18n::set_language(code);
        self.dirty = true;
    }

    pub fn set_ssh(&mut self, command: Option<String>) {
        self.ssh = command;
    }

    pub fn set_ssh_algorithm_catalog(&mut self, catalog: aster_proto::ssh::AlgorithmCatalog) {
        self.ssh_algorithm_catalog = catalog;
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
        self.profile_palettes.get_mut().clear();
        let mut palette = self.extended_palette(self.scheme(&name).unwrap_or(fallback));
        palette.bold_is_bright = self.config.terminal.draw_bold_text_in_bright_colors;
        self.palette = palette;
        let mut theme = if self.config.appearance.ui_from_scheme { Theme::from_palette(&palette) } else { Theme::fixed(&palette, dark) };
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
                let palette = c.palette()?;
                Some(Scheme { name: c.name.clone(), palette })
            })
            .collect()
    }

    pub(crate) fn available_schemes(&self) -> impl Iterator<Item = (Scheme, bool, Option<&str>)> + '_ {
        self.custom_schemes().into_iter().map(|s| (s, true, None))
            .chain(self.schemes.iter().cloned().map(|s| (s, false, None)))
            .chain(self.extension_color_schemes.iter().map(|(s, source)| (s.clone(), false, Some(source.as_str()))))
    }

    pub fn scheme_names(&self) -> Vec<String> {
        self.available_schemes().map(|(s, _, _)| s.name).collect()
    }

    pub(crate) fn scheme(&self, name: &str) -> Option<Palette> {
        self.config.color_schemes.iter().filter(|s| s.name.eq_ignore_ascii_case(name)).find_map(|s| s.palette())
            .or_else(|| self.schemes.iter().find(|s| s.name.eq_ignore_ascii_case(name)).map(|s| s.palette))
            .or_else(|| self.extension_color_schemes.iter().find(|(s, _)| s.name.eq_ignore_ascii_case(name)).map(|(s, _)| s.palette))
    }

    fn refresh_extension_schemes(&mut self) {
        self.extension_color_schemes = self.extension_schemes(self.config.color_schemes.iter().map(|s| s.name.clone())
            .chain(self.schemes.iter().map(|s| s.name.clone())));
        self.sync_scheme_rows();
    }

    pub(crate) fn refresh_extension_contributions(&mut self) {
        self.profiles.clone_from(&self.detected);
        self.profiles.extend(self.config.profiles.iter().cloned());
        self.profiles.extend(self.extension_profiles());
        self.keymap = self.extension_keymap();
        self.sync_profile_page();
        self.sync_profile_sidebar();
        self.refresh_extension_schemes();
        self.restyle();
    }

    fn extended_palette(&self, palette: Palette) -> Palette {
        if self.config.colors.generate_256 { palette.with_extended_colors(self.config.colors.harmonious) } else { palette }
    }

    pub(crate) fn pane_palette(&self, id: PaneId) -> Palette {
        let Some(name) = self.panes.get(&id).and_then(|p| p.profile.scheme.as_deref()).filter(|n| !n.is_empty()) else { return self.palette };
        let cached = self.profile_palettes.borrow().get(name).copied();
        let mut palette = match cached {
            Some(palette) => palette,
            None => {
                let Some(palette) = self.scheme(name) else { return self.palette };
                let palette = self.extended_palette(palette);
                self.profile_palettes.borrow_mut().insert(name.to_owned(), palette);
                palette
            }
        };
        palette.bold_is_bright = self.palette.bold_is_bright;
        palette
    }

    fn scheme_is_dark(&self) -> bool {
        match self.config.appearance.color_scheme_mode {
            SchemeMode::Dark => true,
            SchemeMode::Light => false,
            SchemeMode::Auto => self.system_dark,
        }
    }

    pub(crate) fn active_scheme_tab(&self) -> crate::settings_colors::SchemeTab {
        if self.scheme_is_dark() {
            crate::settings_colors::SchemeTab::Dark
        } else {
            crate::settings_colors::SchemeTab::Light
        }
    }

    pub(crate) fn current_scheme(&self) -> &str {
        self.active_scheme_tab().name(&self.config.appearance)
    }

    fn set_scheme_name(&mut self, name: String) {
        if self.scheme_is_dark() {
            self.config.appearance.color_scheme = name;
        } else {
            self.config.appearance.light_color_scheme = name;
        }
        self.restyle();
    }

    pub(crate) fn apply_scheme(&mut self, name: String) {
        if self.config.read_only() {
            return self.warn_read_only_config();
        }
        self.set_scheme_name(name);
        self.push_save_config();
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn animations(&self) -> bool {
        self.config.appearance.animations && self.system_animations
    }

    pub fn clock(&self) -> Instant {
        let now = Instant::now();
        self.last_frame.map_or(now, |last| now.max(last))
    }

    pub fn set_system_animations(&mut self, on: bool) {
        let before = self.animations();
        self.system_animations = on;
        let after = self.animations();
        if after == before {
            return;
        }
        if after {
            self.reset_blink();
        } else {
            self.reset_animations();
        }
        self.apply_terminal_options();
    }

    pub fn set_config(&mut self, config: Config) {
        let background_changed = self.config.appearance.background_image != config.appearance.background_image;
        let schemes_changed = self.config.color_schemes != config.color_schemes;
        let changed_version = config.version != self.config.version;
        let extensions_changed = self.config.extensions.disabled != config.extensions.disabled;
        let extension_index_changed = self.config.extensions.index_url != config.extensions.index_url;
        let profiles_changed = extensions_changed || self.config.groups != config.groups || self.config.profiles != config.profiles || self.config.hidden_profiles != config.hidden_profiles || self.config.profile_defaults != config.profile_defaults || self.config.terminal.profile != config.terminal.profile;
        let sidebar_changed = profiles_changed || self.config.appearance.profile_sidebar != config.appearance.profile_sidebar;
        let blink_changed = self.config.appearance.animations != config.appearance.animations
            || self.config.terminal.cursor_blink != config.terminal.cursor_blink
            || self.config.terminal.cursor_blink_style != config.terminal.cursor_blink_style;
        let activity_changed = self.config.notifications.activity != config.notifications.activity
            || self.config.notifications.activity_silence_seconds != config.notifications.activity_silence_seconds;
        self.mru.cancel();
        if config.appearance.backdrop != self.config.appearance.backdrop {
            self.effects.push(Effect::Surface(config.appearance.backdrop));
        }
        let sync_changed = !self.config.sync.same_store(&config.sync) || self.config.sync.ready() != config.sync.ready();
        self.config = config;
        if extension_index_changed { self.extension_index_changed(); }
        self.profiles.clone_from(&self.detected);
        self.profiles.extend(self.config.profiles.iter().cloned());
        self.profiles.extend(self.extension_profiles());
        self.keymap = self.extension_keymap();
        if sync_changed { self.sync_settings_changed(); }
        if background_changed { self.request_background_image(); }
        self.update_restart_pending();
        if profiles_changed { self.sync_profile_page(); }
        if sidebar_changed { self.sync_profile_sidebar(); }
        if activity_changed { self.reset_activity_tracking(); }
        if schemes_changed || extensions_changed { self.refresh_extension_schemes(); }
        if !self.animations() {
            self.reset_animations();
        } else if blink_changed {
            self.reset_blink();
        }
        self.apply_language();
        if changed_version { self.warn_read_only_config(); }
        self.apply_terminal_options();
        self.restyle();
    }

    fn apply_terminal_options(&self) {
        let options = self.options();
        for p in self.panes.values() {
            p.terminal.lock().set_options(options.clone());
        }
    }

    pub fn set_viewport(&mut self, width: u32, height: u32, scale: f32) {
        self.size = (width as f32, height as f32);
        self.scale = scale.max(0.5);
        self.dirty = true;
    }

    pub(crate) fn ui(&self) -> f32 {
        let density = self.config.appearance.density;
        self.scale * if density.is_nan() { 1.0 } else { density.clamp(0.6, 1.4) }
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
        Options { scrollback: self.config.terminal.scrollback_lines, cursor, blink: self.cursor_blink_enabled(), word_separators: self.config.terminal.word_separator.clone() }
    }

    fn inherited_cwd(&self, profile: &Profile) -> Option<String> {
        let id = self.focused_pane()?;
        let source = &self.panes.get(&id)?.profile;
        if profile.ssh.is_some() && (source.ssh != profile.ssh || source.ssh_options != profile.ssh_options) {
            return None;
        }
        self.pane_cwd(id)
    }

    pub(crate) fn new_pane_in(&mut self, source_profile: Profile, inherited: Option<String>) -> PaneId {
        let profile = crate::profile::effective(&source_profile, &self.config);
        let native_ssh = profile.ssh.is_some() && self.config.ssh.client == crate::config::SshClient::Native;
        let inherited = inherited.filter(|_| !native_ssh || profile.ssh_options.as_ref().and_then(|o| o.remember_cwd) == Some(true));
        let id = self.next_id;
        self.next_id += 1;
        let size = Size::new(80, 24);
        let sink = Self::terminal_sink(self.sender.clone(), self.wake.clone(), id, 0);
        let terminal = Arc::new(FairMutex::new(Terminal::new(size, self.options(), sink)));
        let mut env = self.config.terminal.environment.clone();
        env.extend(profile.env.clone());
        for (k, v) in [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")] {
            env.entry(k.to_owned()).or_insert_with(|| v.to_owned());
        }
        if self.config.terminal.identification == crate::config::Identification::Aster {
            env.entry("TERM_PROGRAM".to_owned()).or_insert_with(|| "Aster".to_owned());
            env.entry("TERM_PROGRAM_VERSION".to_owned()).or_insert_with(|| env!("CARGO_PKG_VERSION").to_owned());
        }
        if inherited.is_some() {
            terminal.lock().set_cwd(inherited.clone());
        }
        let (args, cwd) = if native_ssh {
            (profile.args.clone(), inherited.or_else(|| profile.cwd.clone()))
        } else if let Some(args) = crate::ssh::system_shell_in_profile_folder(&profile) {
            (args, self.home.clone())
        } else {
            inherit_cwd(&profile, inherited, self.home.clone())
        };
        let local_environment = profile.is_local().then(|| Box::new(crate::environment::LocalEnvironment {
            global: self.config.terminal.environment.clone(), profile: profile.env.clone(), refresh: self.config.terminal.refresh_environment, set_comspec: self.config.terminal.set_comspec,
        }));
        let launch = Launch { delegation: None, local_environment, elevated: profile.elevated, ssh: profile.ssh.clone(), ssh_options: profile.ssh_options.clone().map(Box::new), serial: profile.serial.clone(), socket: profile.socket.clone(), stream: profile.stream.clone(), clear_after_connect: profile.clear_after_connect, command: profile.command.clone(), args, cwd, env, cols: 80, rows: 24 };
        self.panes.insert(id, Pane { terminal, source_profile, profile, title: None, name: None, exited: None, pending: Some(launch), retry_launch: None, ssh_connecting: false, reconnect_at: None, generation: 0, queued: Vec::new(), pid: None, host_id: None, restore_buffer: None, size, scroll_acc: 0.0, seen: 0, activity_seq: 0 });
        id
    }

    fn terminal_sink(sender: Sender<(PaneId, u64, VtEvent)>, wake: Wake, id: PaneId, generation: u64) -> Sink {
        Arc::new(move |event| {
            let _ = sender.send((id, generation, event));
            wake();
        })
    }

    pub fn open_tab(&mut self, profile: Profile) {
        let effective = crate::profile::effective(&profile, &self.config);
        let inherited = if effective.cwd.is_some() { None } else { self.inherited_cwd(&effective) };
        self.insert_profile_tab(profile, inherited);
    }

    pub(crate) fn focused_pane(&self) -> Option<PaneId> {
        self.tabs.get(self.active).map(|t| t.focus)
    }

    pub(crate) fn serial_pane(&self) -> Option<PaneId> {
        self.focused_pane().filter(|id| self.panes.get(id).is_some_and(|p| p.profile.serial.is_some() && p.pending.is_none() && p.exited.is_none()))
    }

    pub fn pane_exit_policy(&self, pane: PaneId) -> Option<OnExit> {
        self.panes.get(&pane).map(|p| p.profile.on_exit)
    }

    pub(crate) fn prompt_pending_serial(&mut self) {
        let focus = self.focused_pane();
        let settings = focus == Some(SETTINGS_PANE);
        if let Some(OverlayKind::SerialSpeed(pane)) = self.overlay.as_ref().map(|o| o.kind) {
            if (focus != Some(pane) && !(settings && self.background_launches.contains(&pane))) || self.panes.get(&pane).is_none_or(|p| p.exited.is_some()) {
                self.overlay = None;
            }
        }
        if self.overlay.is_some() || self.close_confirm.is_some() {
            return;
        }
        let pending = focus.into_iter().chain(self.background_launches.iter().copied().filter(|_| settings)).find(|id| {
            self.panes.get(id).and_then(|p| p.pending.as_ref()).and_then(Launch::baud_placeholder).is_some()
        });
        if let Some(id) = pending { self.open_overlay(OverlayKind::SerialSpeed(id)); }
    }

    pub fn focused_terminal(&self) -> Option<Shared> {
        self.focused_pane().and_then(|p| self.terminal(p)).cloned()
    }

    pub fn terminal(&self, pane: PaneId) -> Option<&Shared> {
        self.panes.get(&pane).map(|p| &p.terminal)
    }

    fn split(&mut self, side: Side) {
        let Some(focus) = self.focused_pane().filter(|p| *p != SETTINGS_PANE) else { return };
        let profile = self.panes.get(&focus).map(|p| p.source_profile.clone()).or_else(|| self.default_profile());
        if let Some(profile) = profile {
            self.split_with(side, profile);
        }
    }

    pub(crate) fn split_with(&mut self, side: Side, profile: Profile) {
        let Some(focus) = self.focused_pane().filter(|p| *p != SETTINGS_PANE) else { return self.open_tab(profile) };
        let effective = crate::profile::effective(&profile, &self.config);
        let same_profile = self.panes.get(&focus).is_some_and(|p| p.profile.id == profile.id);
        let cwd = if same_profile || effective.cwd.is_none() { self.inherited_cwd(&effective) } else { None };
        let Some(root) = self.profile_tree(profile, cwd) else { return };
        let id = root.panes()[0];
        let tab = &mut self.tabs[self.active];
        tab.zoomed = None;
        tab.root.split_tree(focus, side, root);
        tab.focus = id;
        self.prompt_pending_serial();
        self.dirty = true;
    }

    pub fn close_pane(&mut self, pane: PaneId) {
        let Some(ti) = self.tabs.iter().position(|t| t.root.contains(pane)) else { return };
        self.release_pane(pane);
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

    pub(crate) fn release_pane(&mut self, pane: PaneId) {
        if self.files.as_ref().is_some_and(|f| f.pane == pane) {
            self.close_files();
        }
        self.offers.remove(&pane);
        self.prompts.remove(&pane);
        self.ssh_auth.remove(&pane);
        self.ssh_forwards.remove(&pane);
        self.ssh_forward_form.take_if(|form| form.pane == pane);
        self.ssh_auth_paste.take_if(|(_, target, _, _)| *target == pane);
        self.overlay.take_if(|overlay| overlay.kind == OverlayKind::SshPorts(pane));
        if self.search.as_ref().is_some_and(|s| s.pane == pane) {
            self.search = None;
        }
        self.pane_rename.take_if(|edit| edit.pane == pane);
        self.pane_profile_probe.take_if(|probe| probe.pane == pane);
        if let Some(p) = self.panes.remove(&pane) {
            if p.exited.is_none() {
                self.effects.push(Effect::Kill { pane, host_id: p.host_id });
            }
        }
    }

    pub(crate) fn update_pane_references(&mut self, old: PaneId, new: PaneId) {
        for reference in [&mut self.current_tab, &mut self.last_tab] {
            if *reference == Some(old) {
                *reference = Some(new);
            }
        }
    }

    pub(crate) fn remember_tab_origin(&mut self, pane: PaneId) {
        self.last_tab = Some(pane);
        self.current_tab = Some(pane);
    }

    pub(crate) fn remove_tab(&mut self, index: usize) {
        if let Some(state) = self.tab_state(index) {
            if self.closed_tabs.len() == CLOSED_TABS {
                self.closed_tabs.remove(0);
            }
            self.closed_tabs.push(state);
        }
        let tab = self.tabs.remove(index);
        self.mru.remove(tab.key);
        for id in tab.root.panes() {
            self.release_pane(id);
        }
        if self.tabs.is_empty() {
            if self.config.terminal.close_window_with_last_tab {
                self.effects.push(Effect::Close);
            } else {
                self.active = 0;
                self.mru.cancel();
                self.start_selected = 0;
                self.current_tab = None;
                self.drag = Drag::None;
                self.layout = Layout::default();
                self.chrome.tabs.clear();
                self.hover_link = None;
                self.close_overlay();
                self.close_confirm = None;
                self.paste_confirm = None;
                self.close_files();
                self.search = None;
                self.rename = None;
                self.menu = None;
                self.settings = None;
                self.settings_stash = None;
                self.tab_toast = None;
                self.broadcast = None;
            }
            self.dirty = true;
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
        self.effects.extend(self.panes.iter().filter(|(_, p)| p.exited.is_none()).map(|(&pane, p)| Effect::Kill { pane, host_id: p.host_id }));
        self.panes.clear();
        self.ssh_auth.clear();
        self.ssh_forwards.clear();
        self.ssh_forward_form = None;
        self.tabs.clear();
    }

    pub fn activate(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.activity.visit();
            if tab.command_watch == crate::close::CommandWatch::Finished {
                tab.command_watch = Default::default();
                self.dirty = true;
            }
        }
        if index < self.tabs.len() && index != self.active {
            self.active = index;
            self.mru.visit(self.tabs[index].key);
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
        if to >= 0 && self.can_move_tab(self.active, to as usize) {
            self.tabs.swap(self.active, to as usize);
            self.active = to as usize;
            self.dirty = true;
        }
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn tab_bar_hidden(&self) -> bool {
        if self.fullscreen && !self.config.appearance.tabs_in_fullscreen {
            return true;
        }
        if let Some(hidden) = self.tab_bar_override {
            return hidden;
        }
        match self.config.appearance.tab_bar {
            crate::config::TabBar::Always => false,
            crate::config::TabBar::Auto => self.tabs.len() < 2,
            crate::config::TabBar::Never => true,
        }
    }

    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        if self.fullscreen != fullscreen {
            self.fullscreen = fullscreen;
            self.dirty = true;
        }
    }

    pub(crate) fn tab_color(&self, index: usize) -> Option<aster_gfx::render::Color> {
        let tab = self.tabs.get(index)?;
        let own = tab.color.clone();
        let hex = own.or_else(|| self.panes.get(&tab.focus).and_then(|p| p.profile.color.clone()))?;
        aster_vt::schemes::parse_hex(&hex).map(crate::theme::color)
    }

    pub(crate) fn tab_activity(&self, index: usize) -> bool {
        self.tabs.get(index).is_some_and(|t| t.command_watch == crate::close::CommandWatch::Finished || (index != self.active
            && t.root.panes().iter().filter_map(|id| self.panes.get(id)).any(|p| p.exited.is_none() && p.terminal.lock().output_seq() != p.seen)))
    }

    pub fn tab_progress(&self, index: usize) -> Option<aster_vt::Progress> {
        let tab = self.tabs.get(index)?;
        let focused = self.panes.get(&tab.focus).and_then(|p| p.terminal.lock().progress());
        focused.or_else(|| tab.root.panes().iter().filter_map(|id| self.panes.get(id)).find_map(|p| p.terminal.lock().progress()))
    }

    pub(crate) fn mark_seen(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        for id in tab.root.panes() {
            if let Some(p) = self.panes.get_mut(&id) {
                p.seen = p.terminal.lock().output_seq();
            }
        }
    }

    pub fn tab_title(&self, index: usize) -> String {
        let Some(tab) = self.tabs.get(index) else { return String::new() };
        if tab.focus == SETTINGS_PANE {
            return crate::i18n::t("Settings").to_owned();
        }
        if let Some(t) = &tab.title {
            return t.clone();
        }
        let elevated = self.panes.get(&tab.focus).is_some_and(|p| p.profile.elevated);
        if elevated {
            let base = self.panes.get(&tab.focus).map(|p| p.title.clone().unwrap_or_else(|| p.profile.name.clone())).unwrap_or_default();
            return crate::i18n::tf("Administrator: {title}", &[("title", &base)]);
        }
        self.panes.get(&tab.focus).map(|p| p.title.clone().unwrap_or_else(|| p.profile.name.clone())).unwrap_or_default()
    }

    pub(crate) fn write(&mut self, pane: PaneId, bytes: Vec<u8>) {
        let Some(p) = self.panes.get_mut(&pane) else { return };
        if p.reconnect_at.is_some() {
            return;
        }
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
        self.note_input(pane, &bytes);
        self.reset_blink();
        if !queued {
            self.effects.push(Effect::Write { pane, bytes });
        }
    }

    fn write_focused(&mut self, bytes: Vec<u8>) {
        if self.broadcast.is_some() {
            for p in self.broadcast_targets() {
                self.write(p, bytes.clone());
            }
            return;
        }
        if let Some(p) = self.focused_pane() {
            self.write(p, bytes);
        }
    }

    pub(crate) fn broadcast_targets(&self) -> Vec<PaneId> {
        let tabs: Vec<&Tab> = match self.broadcast {
            None => return Vec::new(),
            Some(Broadcast::Tab) => self.tabs.get(self.active).into_iter().collect(),
            Some(Broadcast::AllTabs) => self.tabs.iter().collect(),
        };
        tabs.iter().flat_map(|t| t.root.panes()).filter(|p| self.panes.get(p).is_some_and(|p| p.exited.is_none())).collect()
    }

    fn toggle_broadcast(&mut self, scope: Broadcast) {
        self.broadcast = if self.broadcast == Some(scope) { None } else { Some(scope) };
        self.dirty = true;
    }

    fn mode(&self) -> TermMode {
        self.focused_pane().and_then(|p| self.panes.get(&p)).map(|p| p.terminal.lock().mode()).unwrap_or(TermMode::empty())
    }

    pub fn paste_vault(&mut self, input: u64, text: String) {
        if self.vault_prompt.is_some() {
            if input == self.vault_input_id {
                self.vault_prompt_text(&text);
            }
            return;
        }
        if self.ssh_auth_pasted(input, &text) { return; }
        if input == self.vault_input_id && self.settings.as_ref().is_some_and(|p| p.editing_passphrase() || p.editor.as_ref().is_some_and(|e| e.password_focused())) {
            self.settings_text(&text);
        }
    }

    pub fn paste_path(&mut self, path: &str) {
        if path.is_empty() || path.chars().any(char::is_control) || self.offered_pane().is_some() {
            return;
        }
        if self.focused_pane().and_then(|pane| self.panes.get(&pane)).is_none_or(|pane| pane.exited.is_some()) {
            self.open_default_tab();
            self.track_tab();
        }
        if let Some((id, pane)) = self.focused_pane().and_then(|id| self.panes.get(&id).map(|pane| (id, pane))) {
            let text = dropped_text(&pane.profile, &[path.to_owned()]);
            let bytes = paste_bytes(&text, self.paste_mode(), false);
            self.write(id, bytes);
        }
    }

    pub fn paste(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        if self.ssh_forward_form_text(&text) || self.config_editor_text(&text) { return; }
        let text = if self.config.terminal.paste_newlines_as_spaces && text.contains(['\r', '\n']) {
            text.replace("\r\n", " ").replace(['\r', '\n'], " ")
        } else {
            text
        };
        if let Some(o) = self.overlay.as_mut() {
            o.insert(&text.replace(['\r', '\n'], " "));
            self.dirty = true;
            return;
        }
        if self.rename.is_some() {
            return self.rename_text(&text.replace(['\r', '\n'], " "));
        }
        if self.pane_rename.is_some() {
            return self.pane_rename_text(&text.replace(['\r', '\n'], " "));
        }
        if self.files.is_some() {
            return self.files_text(&text.replace(['\r', '\n'], " "));
        }
        if self.settings.is_some() {
            return self.settings_text(&text.replace(['\r', '\n'], " "));
        }
        if self.sidebar_focused() {
            return self.sidebar_text(&text.replace(['\r', '\n'], " "));
        }
        if self.rearranging() {
            return;
        }
        if self.ssh_auth_active() { return; }
        if let Some(pane) = self.offered_pane() {
            if let Some((PaneTransfer::AwaitingFiles, p)) = self.offers.get_mut(&pane) {
                p.push_str(text.lines().next().unwrap_or_default());
            }
            self.dirty = true;
            return;
        }
        if self.search_paste(&text) {
            return;
        }
        if self.tabs.is_empty() {
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
        if !self.focused && self.config.terminal.bell_attention {
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
            if self.animations() {
                self.bell = Some((pane, self.clock()));
                self.dirty = true;
            } else if self.focused {
                self.effects.push(Effect::Attention);
            }
        }
    }

    pub(crate) fn bell_alpha(&self, pane: PaneId, now: Instant) -> f32 {
        self.bell.filter(|(p, _)| *p == pane && self.animations())
            .map_or(0.0, |(_, at)| 1.0 - crate::motion::eased(at, now, BELL_FLASH))
    }

    fn copy(&mut self) -> bool {
        let Some(pane) = self.focused_pane() else { return false };
        let Some(shared) = self.terminal(pane).cloned() else { return false };
        let terminal = shared.lock();
        let Some(text) = terminal.selection_text() else { return false };
        let effect = if self.config.terminal.copy_as_html {
            Effect::SetClipboardHtml { text, html: crate::clipboard::selection_html(&terminal, &self.pane_palette(pane)) }
        } else {
            Effect::SetClipboard(text)
        };
        self.effects.push(effect);
        true
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

    pub(crate) fn focus_pane(&mut self, pane: PaneId) {
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
            OpenProfile(id) => self.open_profile_by_id(&id),
            Extension { extension, command } => self.run_extension(extension, command),
            SelectProfileSection(section) => self.open_filtered_profiles(crate::picker::ProfileFilter::Section(section)),
            SelectProfileGroup(id) => self.open_filtered_profiles(crate::picker::ProfileFilter::Group(&id)),
            Search => self.open_search(),
            SelectColorScheme => self.open_overlay(OverlayKind::Schemes),
            SetTabColor => self.open_overlay(OverlayKind::TabColors),
            SerialSpeed => {
                if let Some(pane) = self.serial_pane() {
                    self.open_overlay(OverlayKind::SerialSpeed(pane));
                }
            }
            SerialBreak => {
                if let Some(pane) = self.serial_pane() {
                    self.effects.push(Effect::SerialControl { pane, control: SerialControl::Break });
                }
            }
            ReportProblem => self.effects.push(Effect::Open(report_url())),
            DebugCopyState => {
                if let Some(terminal) = self.focused_terminal() {
                    self.effects.push(Effect::SetClipboard(terminal.lock().debug_dump()));
                }
            }
            DebugSaveOutput => {
                if let Some(pane) = self.focused_pane().filter(|p| self.panes.contains_key(p)) {
                    self.effects.push(Effect::SaveOutput { pane });
                }
            }
            ExportBuffer => {
                if let Some(pane) = self.focused_pane() {
                    if let Some(terminal) = self.terminal(pane) {
                        let text = terminal.lock().buffer_text();
                        self.effects.push(Effect::ExportBuffer { pane, text });
                    }
                }
            }
            NotifyCommand => self.toggle_command_watch(),
            OpenLog => self.effects.push(Effect::OpenLog),
            EditEnvironment => self.edit_environment(),
            StartMenuShortcut => self.effects.push(Effect::StartMenuShortcut(!self.start_menu_shortcut)),
            InstallForUser => {
                if self.installation == crate::installation::State::Available {
                    self.effects.push(Effect::InstallForUser);
                }
            }
            DefaultTerminal(enable) => {
                if self.default_terminal.offers(enable) {
                    self.effects.push(Effect::DefaultTerminal(enable));
                }
            }
            OpenConfigFile => self.effects.push(Effect::OpenConfigFile),
            OpenConfigFolder => self.effects.push(Effect::OpenConfigFolder),
            ShowDefaultConfig => self.effects.push(Effect::ShowDefaultConfig),
            ApplicationsMenu(install) => self.effects.push(Effect::ApplicationsMenu(install)),
            WhatsNew => self.effects.push(Effect::Open(concat!("https://github.com/brano-san/Aster/releases/tag/v", env!("CARGO_PKG_VERSION")).into())),
            CheckForUpdates => self.check_updates(),
            ToggleTabBar => {
                if !self.fullscreen || self.config.appearance.tabs_in_fullscreen {
                    self.tab_bar_override = Some(!self.tab_bar_hidden());
                }
                self.dirty = true;
            }
            Settings => self.open_settings(),
            ManageProfiles => self.open_profile_settings(),
            ClearRecentProfiles => self.clear_recent(),
            MinimizeWindow => self.effects.push(Effect::Minimize),
            ToggleMaximize => self.effects.push(Effect::ToggleMaximize),
            CloseWindow => self.request_close(crate::close::CloseTarget::Window),
            Quit => self.effects.push(Effect::Quit),
            RenameTab => self.start_rename(self.active),
            ToggleProfileSidebar => self.toggle_profile_sidebar(),
            RearrangePanes => self.toggle_rearrange(),
            PaneToTab => self.move_pane_to_tab(),
            RenamePane => {
                if let Some(pane) = self.focused_pane() {
                    self.start_pane_rename(pane);
                }
            }
            ChangePaneProfile => {
                if let Some(pane) = self.focused_pane().filter(|id| self.panes.contains_key(id)) {
                    self.open_overlay(OverlayKind::PaneProfile(pane));
                }
            }
            PinTab => self.toggle_pin(),
            SaveAsProfile => self.save_as_profile(false),
            SaveLayout => self.save_as_profile(true),
            OpenFiles => self.open_files(),
            FillPassword => {
                self.fill_password();
            }
            CloseOtherTabs => self.close_tabs_except(|i, active| i == active),
            CloseTabsToRight => self.close_tabs_except(|i, active| i <= active),
            CloseTabsToLeft => self.close_tabs_except(|i, active| i >= active),
            ToggleFullscreen => self.effects.push(Effect::ToggleFullscreen),
            NewTab => self.open_default_tab(),
            NewWindow => self.effects.push(Effect::NewWindow),
            CloseTab => self.close_tab_asking(self.active),
            DuplicateTab => {
                if let Some(p) = self.focused_pane().and_then(|p| self.panes.get(&p)).map(|p| p.source_profile.clone()) {
                    self.open_tab(p);
                }
            }
            NewAdminTab => {
                if let Some(p) = self.default_profile() {
                    self.open_admin_tab(p);
                }
            }
            DuplicateAsAdmin => {
                if let Some(p) = self.focused_pane().and_then(|p| self.panes.get(&p)).map(|p| p.source_profile.clone()) {
                    self.open_admin_tab(p);
                }
            }
            PaneResizeLeft => self.nudge_divider(Axis::Horizontal, -1.0),
            PaneResizeRight => self.nudge_divider(Axis::Horizontal, 1.0),
            PaneResizeUp => self.nudge_divider(Axis::Vertical, -1.0),
            PaneResizeDown => self.nudge_divider(Axis::Vertical, 1.0),
            ExplodeTab => self.explode_tab(),
            BroadcastToTab => self.toggle_broadcast(Broadcast::Tab),
            BroadcastToAllTabs => self.toggle_broadcast(Broadcast::AllTabs),
            CombineTabs => self.combine_tabs(),
            ToggleLastTab => self.toggle_last_tab(),
            ReopenTab => self.reopen_tab(),
            NextTab => self.cycle(1),
            PreviousTab => self.cycle(-1),
            NextMruTab | PreviousMruTab => self.switch_mru(action, false),
            MoveTabLeft => self.move_tab(-1),
            MoveTabRight => self.move_tab(1),
            Tab(n) if self.tabs.is_empty() => {
                if let Some(&entry @ crate::start::Entry::Recent(_)) = self.start_page().entries.get(n as usize) {
                    self.launch_start_entry(entry);
                }
            }
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
                    self.request_close(crate::close::CloseTarget::Pane(p));
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

    fn open_filtered_profiles(&mut self, filter: crate::picker::ProfileFilter<'_>) {
        self.overlay = Some(Overlay::new(OverlayKind::Profiles, self.filtered_profile_picks(filter)).with_quick_connect(None, self.ssh.is_some()).opened(self.clock()));
        self.dirty = true;
    }

    pub(crate) fn open_overlay(&mut self, kind: OverlayKind) {
        self.list_motion.overlay = Default::default();
        let items = match kind {
            OverlayKind::Commands => self.command_picks(),
            OverlayKind::Profiles => self.profile_picks(),
            OverlayKind::PaneProfile(_) => self.filtered_profile_picks(crate::picker::ProfileFilter::SinglePane),
            OverlayKind::ProfileCreate => crate::profiles_page::profile_creation(),
            OverlayKind::ProfileTemplates => crate::profiles_page::profile_templates(),
            OverlayKind::ProfileIcons => crate::profiles_page::profile_icons(),
            OverlayKind::Schemes => self.available_schemes().map(|(s, _, source)| Pick {
                label: s.name.clone(), value: s.name, hint: source.unwrap_or_default().into(), ..Pick::default()
            }).collect(),
            OverlayKind::TabColors => TAB_COLORS.iter().map(|(name, hex)| Pick { label: crate::i18n::t(name).to_owned(), hint: hex.to_string(), value: hex.to_string(), ..Pick::default() }).collect(),
            OverlayKind::SerialSpeed(pane) => {
                self.overlay = Some(Overlay::serial_speed(pane).opened(self.clock()));
                self.dirty = true;
                return;
            }
            OverlayKind::SshKeys(_) => Vec::new(),
            OverlayKind::SshPorts(pane) => self.ssh_forward_picks(pane),
        };
        let mut overlay = Overlay::new(kind, items);
        if kind == OverlayKind::Profiles {
            overlay = overlay.with_quick_connect(Some(self.config.picker.quick_connect), self.ssh.is_some());
        }
        if kind == OverlayKind::Schemes {
            let current = self.current_scheme();
            if let Some(at) = overlay.shown.iter().position(|(i, _)| overlay.items[*i].value.eq_ignore_ascii_case(current)) {
                overlay.select(at);
            }
            self.scheme_before = Some(current.to_owned());
        }
        self.overlay = Some(overlay.opened(self.clock()));
        self.dirty = true;
    }

    fn choose(&mut self, kind: OverlayKind, value: String) {
        if let OverlayKind::SshPorts(pane) = kind {
            return self.choose_ssh_forward(pane, &value);
        }
        self.overlay = None;
        self.dirty = true;
        match kind {
            OverlayKind::ProfileCreate => self.create_profile_item(&value),
            OverlayKind::ProfileTemplates => self.edit_profile_template(&value),
            OverlayKind::SshKeys(_) => self.choose_ssh_key(value),
            OverlayKind::SshPorts(_) => {}
            OverlayKind::ProfileIcons => self.set_profile_icon(value),
            OverlayKind::SerialSpeed(pane) => {
                let Ok(baud) = value.parse() else { return };
                if let Some(launch) = self.panes.get_mut(&pane).and_then(|p| p.pending.as_mut()) {
                    if let (Some(at), Some(spec)) = (launch.baud_placeholder(), launch.serial.as_mut()) {
                        spec.replace_range(at..at + 1, &value);
                    }
                } else if self.serial_pane() == Some(pane) {
                    self.effects.push(Effect::SerialControl { pane, control: SerialControl::Baud(baud) });
                }
            }
            OverlayKind::Commands => {
                if let Some(a) = Action::from_id(&value) {
                    self.run(a);
                }
            }
            OverlayKind::Profiles if value.starts_with(crate::overlay::QUICK) => {
                let text = &value[crate::overlay::QUICK.len()..];
                use crate::quick_connect::QuickTarget;
                if let Some((target, _)) = QuickTarget::parse(text, None, self.ssh.is_some()) {
                    if let Some(profile) = target.profile(self.ssh.as_deref()) {
                        self.open_tab(profile);
                    }
                }
            }
            OverlayKind::Profiles if value == crate::overlay::MANAGE => self.run(Action::ManageProfiles),
            OverlayKind::Profiles if value == crate::overlay::CLEAR_RECENT => self.run(Action::ClearRecentProfiles),
            OverlayKind::Profiles => {
                self.open_profile_by_id(value.strip_prefix(crate::picker::RECENT).unwrap_or(&value));
            }
            OverlayKind::PaneProfile(pane) => self.request_pane_profile(pane, value),
            OverlayKind::TabColors => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.color = (!value.is_empty()).then_some(value);
                }
            }
            OverlayKind::Schemes => {
                self.scheme_before = None;
                self.apply_scheme(value);
            }
        }
    }

    fn preview_scheme(&mut self) {
        if self.config.read_only() { return; }
        let Some(o) = self.overlay.as_ref().filter(|o| o.kind == OverlayKind::Schemes) else { return };
        if let Some(v) = o.selected_value() {
            if !v.eq_ignore_ascii_case(self.current_scheme()) {
                self.set_scheme_name(v);
            }
        }
    }

    fn close_overlay(&mut self) {
        if let Some(OverlayKind::SerialSpeed(pane)) = self.overlay.take().map(|o| o.kind) {
            if self.panes.get(&pane).is_some_and(|p| p.pending.is_some()) {
                self.close_pane(pane);
            }
        }
        if let Some(name) = self.scheme_before.take() {
            self.set_scheme_name(name);
        }
        self.dirty = true;
    }

    fn overlay_key(&mut self, key: Key, mods: Mods) {
        if self.scroll_overlay_key(key) {
            return self.preview_scheme();
        }
        let Some(o) = self.overlay.as_mut() else { return };
        self.dirty = true;
        match key {
            Key::Escape => return self.close_overlay(),
            Key::Up => o.step(-1),
            Key::Down => o.step(1),
            Key::PageUp => o.step(-8),
            Key::PageDown => o.step(8),
            Key::Backspace if o.kind == OverlayKind::Profiles && o.query_text().is_empty() => {
                let provider = o.quick_provider();
                let address = o.selected_value().and_then(|value| self.profile_edit_address(&value, provider));
                if let (Some(address), Some(o)) = (address, self.overlay.as_mut()) {
                    o.set_query(&address);
                }
            }
            Key::Backspace => o.backspace(mods.ctrl),
            Key::Enter if mods.ctrl && o.kind == OverlayKind::Profiles => {
                let value = o.selected_value().unwrap_or_default();
                let id = value.strip_prefix(crate::picker::RECENT).unwrap_or(&value).to_owned();
                if let Some(p) = self.profiles.iter().find(|p| p.id == id).cloned() {
                    self.overlay = None;
                    self.note_recent(&id);
                    self.split_with(Side::Right, p);
                }
            }
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

    fn reset_animations(&mut self) {
        self.motion = false;
        self.list_motion = Default::default();
        self.bell = None;
        let now = self.clock();
        self.tab_toast.take_if(|toast| toast.tab_name || now >= toast.fade_at);
        self.reset_blink();
    }

    fn reset_blink(&mut self) {
        self.blink = crate::cursor_motion::Blink::new(self.clock());
        self.dirty = true;
    }

    pub(crate) fn terminal_has_focus(&self) -> bool {
        self.focused && self.overlay.is_none() && self.settings.is_none() && self.menu.is_none() && self.rename.is_none() && self.pane_rename.is_none() && self.paste_confirm.is_none() && self.close_confirm.is_none() && self.files.is_none() && !self.sidebar_focused() && !self.rearranging()
    }

    pub(crate) fn cursor_blink_enabled(&self) -> bool {
        self.animations() && self.config.terminal.cursor_blink
    }

    pub(crate) fn cursor_blink(&self, now: Instant) -> (f32, bool) {
        let smooth = self.config.terminal.cursor_blink_style == crate::config::CursorBlinkStyle::Smooth;
        self.blink.sample(self.cursor_blink_enabled(), smooth, now)
    }

    fn blinking(&self) -> bool {
        self.cursor_blink_enabled() && self.terminal_has_focus() && self.focused_pane().is_some()
    }

    pub fn on_timer(&mut self) {
        self.on_timer_at(self.clock());
    }

    fn on_timer_at(&mut self, now: Instant) {
        self.timer_due = None;
        self.poll_commands(now);
        self.resume_connections(now);
        let animate = self.animations();
        if let Some(toast) = self.tab_toast.as_ref().filter(|toast| now >= toast.fade_at) {
            if now >= toast.expires(animate) {
                self.layers.forget_snapshot(crate::layers::Layer::Toast);
                self.tab_toast = None;
            }
            self.dirty = true;
        }
        if self.bell.is_some_and(|(_, t)| now.saturating_duration_since(t) >= BELL_FLASH) {
            self.bell = None;
            self.dirty = true;
        }
        if self.blinking() && self.blink.tick(now) {
            self.dirty = true;
        }
        self.schedule_timer_at(now);
    }

    fn resume_connections(&mut self, now: Instant) {
        for (&id, pane) in &mut self.panes {
            if pane.reconnect_at.take_if(|due| *due <= now).is_some() {
                let Some(launch) = pane.retry_launch.as_deref_mut() else { continue };
                launch.cols = pane.size.cols as u16;
                launch.rows = pane.size.rows as u16;
                pane.exited = None;
                pane.ssh_connecting = launch.ssh.is_some();
                self.effects.push(Effect::Spawn { pane: id, launch: launch.clone(), terminal: pane.terminal.clone(), reconnecting: true });
                self.dirty = true;
            }
        }
    }

    pub(crate) fn reconnect_ssh(&mut self, pane: PaneId) {
        let options = self.options();
        let Some(p) = self.panes.get_mut(&pane).filter(|p| p.profile.ssh.is_some()) else { return };
        let Some(launch) = p.retry_launch.as_ref() else { return };
        if p.exited.is_none() { self.effects.push(Effect::Kill { pane, host_id: None }); }
        p.generation += 1;
        let sink = Self::terminal_sink(self.sender.clone(), self.wake.clone(), pane, p.generation);
        let terminal = p.terminal.lock().next_session(options, sink, launch.cwd.clone());
        p.terminal = Arc::new(FairMutex::new(terminal));
        p.pid = None;
        p.title = None;
        p.queued.clear();
        self.ssh_auth.remove(&pane);
        self.ssh_forwards.remove(&pane);
        self.ssh_forward_form.take_if(|form| form.pane == pane);
        self.offers.remove(&pane);
        self.prompts.remove(&pane);
        let now = Instant::now();
        p.reconnect_at = Some(now);
        if self.files.as_ref().is_some_and(|page| page.pane == pane) { self.close_files(); }
        self.resume_connections(now);
    }

    pub fn on_event(&mut self, event: Event) {
        self.handle(event);
        self.track_tab();
    }

    fn track_tab(&mut self) {
        if !self.rearranging() {
            self.stop_rearranging();
        }
        let focused = self.focused_pane();
        self.pane_rename.take_if(|edit| focused != Some(edit.pane));
        self.pane_profile_probe.take_if(|probe| focused != Some(probe.pane));
        if let Some(tab) = self.tabs.get_mut(self.active) {
            self.mru.visit(tab.key);
            tab.activity.visit();
        }
        let on = self.tabs.get(self.active).is_some_and(|t| t.focus == SETTINGS_PANE);
        if on && self.settings.is_none() {
            self.restore_settings_page();
            self.dirty = true;
        } else if !on && self.settings.is_some() {
            self.config_editor.cancel_paste();
            if let Some(page) = self.settings.as_mut() {
                page.cancel_vault_input();
            }
            self.settings_stash = self.settings.take();
            self.dirty = true;
        }
        if !self.tabs.iter().any(|t| t.focus == SETTINGS_PANE) {
            self.settings_stash = None;
        }
        let current = self.tabs.get(self.active).map(|t| t.focus);
        let same = match (current, self.current_tab) {
            (Some(now), Some(before)) => self.tabs[self.active].root.contains(before) || now == before,
            (now, before) => now == before,
        };
        if !same && self.animations() && self.tab_bar_hidden() && self.tabs.len() > 1 && self.current_tab.is_some() {
            let title = format!("{} / {}  {}", self.active + 1, self.tabs.len(), self.tab_title(self.active));
            self.tab_toast = Some(crate::notices::Toast::new(title, self.clock(), TOAST, true));
            self.dirty = true;
        }
        if !same {
            if let Some(before) = self.current_tab.filter(|p| self.panes.contains_key(p)) {
                self.last_tab = Some(before);
            }
        }
        if current != self.current_tab {
            self.prompt_pending_serial();
            self.current_tab = current;
        }
    }

    fn nudge_divider(&mut self, axis: Axis, direction: f32) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let delta = direction * self.config.panes.resize_step();
        if tab.zoomed.is_none() && tab.root.nudge(tab.focus, axis, delta) {
            self.dirty = true;
        }
    }

    fn explode_tab(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let panes = tab.root.panes();
        let Some((&first, rest)) = panes.split_first() else { return };
        if rest.is_empty() {
            return;
        }
        let (focus, color, pinned) = (tab.focus, tab.color.clone(), tab.pinned);
        tab.root = Node::Leaf(first);
        tab.focus = first;
        tab.zoomed = None;
        let at = self.active;
        for (i, &id) in rest.iter().enumerate() {
            self.tabs.insert(at + 1 + i, Tab { color: color.clone(), pinned, ..Tab::new(Node::Leaf(id), id) });
        }
        self.active = at + panes.iter().position(|p| *p == focus).unwrap_or(0);
        self.dirty = true;
    }

    fn combine_tabs(&mut self) {
        if self.tabs.iter().filter(|t| t.focus != SETTINGS_PANE).count() < 2 {
            return;
        }
        let active = self.tabs.get(self.active).map(|t| t.focus).filter(|f| *f != SETTINGS_PANE);
        let (settings, terminals): (Vec<Tab>, Vec<Tab>) = std::mem::take(&mut self.tabs).into_iter().partition(|t| t.focus == SETTINGS_PANE);
        let focus = active.unwrap_or(terminals[0].focus);
        let mut tabs = terminals.into_iter();
        let Some(first) = tabs.next() else { return };
        let mut combined = first;
        let mut count = 1.0;
        for tab in tabs {
            let root = std::mem::replace(&mut combined.root, Node::Leaf(0));
            combined.root = Node::Split { axis: Axis::Horizontal, ratio: count / (count + 1.0), a: Box::new(root), b: Box::new(tab.root) };
            count += 1.0;
        }
        combined.focus = focus;
        combined.zoomed = None;
        self.tabs = std::iter::once(combined).chain(settings).collect();
        self.tabs.sort_by_key(|t| !t.pinned);
        self.active = self.tabs.iter().position(|t| t.focus == focus).unwrap_or(0);
        self.dirty = true;
    }

    fn toggle_last_tab(&mut self) {
        let Some(last) = self.last_tab else { return };
        if let Some(index) = self.tabs.iter().position(|t| t.root.contains(last)) {
            self.activate(index);
        }
    }

    fn reopen_tab(&mut self) {
        let Some(state) = self.closed_tabs.pop() else { return };
        let root = self.build(&state.root);
        let order = root.panes();
        let focus = order.get(state.focus).or(order.first()).copied().unwrap_or(0);
        let at = self.tab_insert_index(state.pinned);
        self.tabs.insert(at, Tab { title: state.title, color: state.color, pinned: state.pinned, ..Tab::new(root, focus) });
        self.active = at;
        self.overlay = None;
        self.dirty = true;
    }

    fn handle(&mut self, event: Event) {
        if self.vault_prompt.is_some() && self.vault_prompt_event(&event) {
            if matches!(event, Event::Key { key: Key::Escape | Key::Enter, pressed: true, .. }) {
                self.swallow_control = true;
            }
            return;
        }
        if self.ssh_forward_form_event(&event) {
            if self.ssh_forward_form.is_none() {
                self.swallow_control = matches!(event, Event::Key { key: Key::Enter | Key::Escape, pressed: true, .. });
                self.swallow_char = matches!(event, Event::Key { key: Key::Space, pressed: true, .. });
            }
            return;
        }
        match event {
            Event::Resized { width, height } => {
                self.size = (width as f32, height as f32);
                self.dirty = true;
            }
            Event::ScaleChanged(s) => {
                self.scale = s;
                self.dirty = true;
            }
            Event::Key { key: Key::Control, mods, pressed: false } if !mods.ctrl => self.finish_mru(),
            Event::Key { key, .. } if key.is_modifier() => {}
            Event::Key { key, mods, pressed: true } => {
                self.modifier_gesture.cancel();
                self.on_key(key, mods);
            }
            Event::Key { .. } => {}
            Event::Char(c) => {
                self.modifier_gesture.cancel();
                self.on_char(c, false);
            }
            Event::AltChar(c) => {
                self.modifier_gesture.cancel();
                self.on_char(c, true);
            }
            Event::Pasted(t) => self.paste(t),
            Event::VaultPasted { input, text } => self.paste_vault(input, text),
            Event::ConfigEditorPasted { request, text } => self.paste_config_editor(request, &text),
            Event::Preedit(p) => {
                self.preedit = if self.settings.as_ref().is_some_and(|page| page.editing_passphrase()) { Preedit::default() } else { p };
                self.dirty = true;
            }
            Event::Focus(f) => {
                if !f {
                    self.mru.cancel();
                    self.dirty = true;
                }
                self.focused = f;
                self.mouse_mods = Mods::NONE;
                self.modifier_gesture = Default::default();
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
            Event::Animations(on) => self.set_system_animations(on),
            Event::MouseMove { x, y } => self.on_move(x, y),
            Event::MouseDown { x, y, button, clicks } => {
                if self.mru.selection().is_some() {
                    self.mru.cancel();
                    self.dirty = true;
                }
                self.modifier_gesture.cancel();
                self.on_down(x, y, button, clicks);
            }
            Event::MouseUp { x, y, button } => self.on_up(x, y, button),
            Event::Modifiers(mods) => {
                if !mods.ctrl {
                    self.finish_mru();
                }
                self.mouse_mods = mods;
                if let Some(mods) = self.modifier_gesture.change(mods) {
                    let chord = Chord { key: Key::ModifiersOnly, mods };
                    if !self.capture_shortcut(chord) {
                        self.non_keyboard_shortcut(chord);
                    }
                }
            }
            Event::Wheel { dx, dy } => {
                self.modifier_gesture.cancel();
                self.on_wheel(dx, dy, None);
            }
            Event::PreciseWheel { dx, dy, pixels_per_notch } => {
                self.modifier_gesture.cancel();
                if pixels_per_notch.is_finite() && pixels_per_notch > 0.0 {
                    self.on_wheel(dx / pixels_per_notch, dy / pixels_per_notch, Some(dy));
                }
            }
            Event::MouseLeave => {
                self.mouse = (-1.0, -1.0);
                self.hover = HitArea::Client;
                if self.menu.is_some() {
                    self.menu_hover(self.mouse.0, self.mouse.1);
                }
                self.dirty = true;
            }
            Event::DroppedFiles(paths) => {
                if self.offered_pane().is_some() {
                    if let Some(pane) = self.awaiting_pane() {
                        self.effects.push(Effect::SendFiles { pane, files: paths.into_iter().map(std::path::PathBuf::from).collect() });
                    }
                } else if self.files.is_some() {
                    self.files_dropped(paths);
                } else if let Some(profile) = self.focused_pane().and_then(|p| self.panes.get(&p)).map(|p| p.profile.clone()) {
                    let text = dropped_text(&profile, &paths);
                    self.write_focused(text.into_bytes());
                }
            }
            Event::CloseRequested => self.request_close(crate::close::CloseTarget::Window),
            Event::SessionEnding => {
                self.close_all();
                self.effects.push(Effect::Close);
            }
            Event::Timer => self.on_timer(),
            Event::GlobalHotkey => {}
            Event::NotificationClicked(tab) => self.notification_clicked(tab),
        }
    }

    fn on_key(&mut self, key: Key, mods: Mods) {
        let ssh_auth = self.ssh_auth_active();
        let modal = self.overlay.is_some() || self.paste_confirm.is_some() || self.close_confirm.is_some() || self.menu.is_some() || self.rename.is_some() || self.pane_rename.is_some() || self.settings.is_some() || self.files.is_some() || self.sidebar_focused() || self.rearranging() || ssh_auth;
        self.swallow_char = false;
        self.swallow_chord = None;
        self.swallow_control = (modal || self.tabs.is_empty()) && matches!(key, Key::Enter | Key::Escape | Key::Tab | Key::Backspace);
        if self.mru.selection().is_some() {
            if key == Key::Escape {
                self.mru.cancel();
                self.swallow_char = true;
                self.dirty = true;
            } else if key != Key::Other {
                if let Outcome::Action(action @ (Action::NextMruTab | Action::PreviousMruTab)) = self.keymap.press(Chord { key, mods }) {
                    self.switch_mru(action, mods.ctrl);
                }
                self.swallow_chord = Some(key);
            }
            return;
        }
        if !modal && self.search.is_none() && self.cancel_focused_reconnect() {
            self.swallow_char = true;
            self.keymap.reset();
            return;
        }
        if self.overlay.is_some() {
            self.overlay_key(key, mods);
            return;
        }
        if self.close_confirm.is_some() {
            return self.close_key(key);
        }
        if self.paste_confirm.is_some() {
            return self.paste_key(key);
        }
        if self.menu.is_some() {
            return self.menu_key(key);
        }
        if self.rename.is_some() {
            return self.rename_key(key, mods);
        }
        if self.pane_rename.is_some() {
            return self.pane_rename_key(key, mods);
        }
        if self.files.is_some() {
            if self.keymap.press(Chord { key, mods }) == Outcome::Action(Action::OpenFiles) {
                return self.close_files();
            }
            return self.files_key(key, mods);
        }
        if !modal {
            if let Some(pane) = self.offered_pane() {
                self.swallow_control = true;
                return self.offer_key(pane, key, mods);
            }
        }
        if let Some(page) = self.settings.as_ref() {
            if page.capture.is_some() {
                self.swallow_char = true;
                return self.settings_key(key, mods);
            }
            if page.editing_passphrase() || page.editor.is_some() || page.group_editor.is_some() || page.scheme_editor.is_some() || page.deleting_scheme.is_some() {
                return self.settings_key(key, mods);
            }
            let editing = page.editing() || page.extensions.editing || self.config_editor_focused();
            if !editing && page.current_shortcut().is_some() && crate::settings::shortcut_edit_key(key, mods) {
                self.keymap.reset();
                return self.settings_key(key, mods);
            }
            match self.keymap.press(Chord { key, mods }) {
                Outcome::Action(Action::Settings) => return self.close_settings(),
                Outcome::Action(a) if !editing && a.works_beside_settings() => {
                    self.swallow_chord = Some(key);
                    return self.run_shortcut(a, mods);
                }
                _ => {}
            }
            return self.settings_key(key, mods);
        }
        if self.sidebar_key(key, mods) {
            return;
        }
        if self.rearrange_key(key, mods) {
            self.swallow_chord = Some(key);
            return;
        }
        if ssh_auth {
            if mods.ctrl && !matches!(key, Key::Char('C' | 'V')) {
                if let Outcome::Action(action) = self.keymap.press(Chord { key, mods }) {
                    if action.works_beside_settings() {
                        self.swallow_chord = Some(key);
                        return self.run_shortcut(action, mods);
                    }
                }
            }
            return self.ssh_auth_key(key, mods);
        }
        if key == Key::Other {
            return;
        }
        if self.search_key(key, mods) {
            return;
        }
        let outcome = self.keymap.press(Chord { key, mods });
        if outcome != Outcome::Action(Action::FillPassword) && key != Key::Other {
            self.dismiss_offer();
        }
        match outcome {
            Outcome::Action(a) => {
                self.swallow_chord = Some(key);
                self.run_shortcut(a, mods);
            }
            Outcome::Pending => self.swallow_chord = Some(key),
            Outcome::None if self.search.is_some() => {}
            Outcome::None if self.sidebar_focused() => {}
            Outcome::None if self.rearranging() => {}
            Outcome::None if self.tabs.is_empty() => {
                if mods == Mods::NONE {
                    self.start_key(key);
                }
            }
            Outcome::None => {
                if let Some(e) = encode_key(key, mods, self.mode()) {
                    self.swallow_char = e.swallows_char;
                    self.write_focused(e.bytes);
                }
            }
        }
    }

    fn run_shortcut(&mut self, action: Action, mods: Mods) {
        if mods.ctrl && matches!(action, Action::NextMruTab | Action::PreviousMruTab) {
            self.switch_mru(action, true);
        } else {
            self.run(action);
        }
    }

    fn on_char(&mut self, c: char, alt: bool) {
        if self.mru.selection().is_some() {
            return;
        }
        if std::mem::take(&mut self.swallow_char) {
            return;
        }
        if let Some(key) = self.swallow_chord.take() {
            if c.is_control() || matches!(key, Key::Char(k) if k.eq_ignore_ascii_case(&c)) {
                return;
            }
        }
        if std::mem::take(&mut self.swallow_control) && c.is_control() {
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
        if self.menu.is_some() || self.paste_confirm.is_some() || self.close_confirm.is_some() {
            return;
        }
        if self.rename.is_some() {
            return self.rename_text(&c.to_string());
        }
        if self.pane_rename.is_some() {
            return self.pane_rename_text(&c.to_string());
        }
        if self.files.is_some() {
            return self.files_text(&c.to_string());
        }
        if self.settings.is_some() {
            return self.settings_text(&c.to_string());
        }
        if self.sidebar_focused() {
            return self.sidebar_text(&c.to_string());
        }
        if self.rearranging() {
            return self.rearrange_char(c);
        }
        if self.ssh_auth_active() && self.ssh_auth_text(&c.to_string()) { return; }
        if let Some(pane) = self.offered_pane() {
            if !c.is_control() {
                if let Some((PaneTransfer::AwaitingFiles, p)) = self.offers.get_mut(&pane) {
                    p.push(c);
                }
                self.dirty = true;
            }
            return;
        }
        if self.search.is_some() {
            if !alt {
                self.search_char(c);
            }
            return;
        }
        if self.cancel_focused_reconnect() {
            return;
        }
        let bytes = if alt && self.config.terminal.alt_is_meta {
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
        let inner = view::pane_inner_with(self.pane_terminal_rect(pane, rect), self.ui(), self.config.terminal.padding);
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
        if !mode.intersects(TermMode::MOUSE_MODE) || (matches!(action, MouseAction::Motion(None)) && self.pane_header(pane, rect).contains(x, y)) {
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
        let grab = (3.0 * self.ui()).max(3.0);
        self.layout.splitters.iter().find(|s| s.rect.inset(-grab, -grab).contains(x, y))
    }

    fn on_move(&mut self, x: f32, y: f32) {
        let previous = self.mouse;
        let was_on_bar = self.chrome.tab_bar.contains(self.mouse.0, self.mouse.1);
        let was_on_sidebar = self.chrome.profile_sidebar.contains(self.mouse.0, self.mouse.1);
        self.mouse = (x, y);
        if self.menu.is_some() {
            return self.menu_hover(x, y);
        }
        self.dirty |= self.overlay.is_some() && previous != self.mouse;
        if self.config_editor_mouse_move(x, y) { return; }
        if self.drag == Drag::None && self.update_hovered(x, y) {
            self.hover = HitArea::Client;
            self.hover_link = None;
            self.set_cursor(CursorIcon::Default);
            self.dirty = true;
            return;
        }
        let hover = self.hit_test(x, y);
        if hover != self.hover {
            self.hover = hover;
            self.dirty = true;
        }
        if was_on_bar || was_on_sidebar || self.chrome.tab_bar.contains(x, y) {
            self.dirty = true;
        }
        match self.drag.clone() {
            Drag::SidebarResize { .. } => return self.resize_sidebar(x),
            Drag::Pane { pane, start, moved } => return self.drag_pane(pane, start, moved, x, y),
            Drag::SettingsSlider { item, track, .. } => {
                self.drag = Drag::SettingsSlider { item, track, x };
                self.dirty = true;
                return;
            }
            Drag::SettingsScrollbar { page, grab } => return self.drag_settings_scrollbar(page, grab, y),
            Drag::Select { pane, .. } => {
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
            Drag::Tab { index, start, grab, moved } => return self.drag_tab(index, start, grab, moved, x, y),
            _ => {}
        }
        if self.sidebar_move(x, y) {
            self.hover_link = None;
            return;
        }
        if self.tabs.is_empty() && self.overlay.is_none() && self.chrome.content.contains(x, y) {
            let hit = self.start_page().hit(x, y);
            self.set_cursor(if hit.is_some() { CursorIcon::Hand } else { CursorIcon::Default });
            self.dirty = true;
            return;
        }
        if self.settings.is_some() || self.files.is_some() {
            self.dirty |= self.chrome.content.contains(previous.0, previous.1);
            if self.chrome.content.contains(x, y) {
                self.dirty = true;
                self.hover_link = None;
                self.set_cursor(CursorIcon::Default);
                return;
            }
        }
        let modal = self.overlay.is_some() || self.paste_confirm.is_some() || self.close_confirm.is_some() || self.rename.is_some() || self.pane_rename.is_some() || self.sidebar_focused() || self.rearranging();
        if self.config.panes.focus_follows_mouse && !modal && self.drag == Drag::None {
            if let Some((pane, _)) = self.pane_at(x, y) {
                self.focus_pane(pane);
            }
        }
        if !modal && self.drag == Drag::None {
            if self.search_ruler_rect().is_some_and(|rect| rect.contains(x, y)) {
                self.hover_link = None;
                self.set_cursor(CursorIcon::Hand);
                self.dirty = true;
                return;
            }
            if let Some(cursor) = self.pane_at(x, y).and_then(|(pane, rect)| self.ssh_pointer_at(pane, rect, x, y)) {
                self.hover_link = None;
                self.set_cursor(cursor);
                self.dirty = true;
                return;
            }
            if self.pane_at(previous.0, previous.1).and_then(|(pane, rect)| self.ssh_pointer_at(pane, rect, previous.0, previous.1)).is_some() {
                self.dirty = true;
            }
        }
        let link = self.link_at(x, y);
        if link != self.hover_link {
            self.hover_link = link;
            self.dirty = true;
        }
        let icon = if self.pane_header_at(x, y).is_some() {
            if self.pane_rename.is_some() { CursorIcon::Text } else { CursorIcon::Hand }
        } else if let Some(s) = self.splitter_at(x, y) {
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
        if self.pane_header(pane, rect).contains(x, y) {
            return None;
        }
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
        let chord = Chord { key: Key::Mouse(button), mods: self.mouse_mods };
        if self.capture_shortcut(chord) {
            return;
        }
        if let Some(o) = self.overlay.as_ref() {
            if matches!(o.kind, OverlayKind::SshPorts(_)) && button != MouseButton::Left { return; }
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
        if self.close_confirm.is_some() {
            return self.close_click(x, y);
        }
        if self.paste_confirm.is_some() {
            return self.paste_click(x, y);
        }
        if self.menu.is_some() {
            return self.menu_click(x, y);
        }
        if self.update_click(x, y) {
            return;
        }
        if let Some(r) = self.rename.as_ref() {
            let inside = self.chrome.tabs.get(r.tab).is_some_and(|t| t.rect.contains(x, y));
            if !inside {
                self.commit_rename();
            } else if button == MouseButton::Left {
                return;
            }
        }
        if let Some(edit) = &self.pane_rename {
            if self.pane_header_at(x, y) == Some(edit.pane) && button == MouseButton::Left {
                return;
            }
            self.commit_pane_rename();
        }
        if self.sidebar_down(x, y, button, clicks) {
            return;
        }
        if self.files.is_some() && self.chrome.content.contains(x, y) {
            if button == MouseButton::Left {
                let n = self.click_count(x, y, clicks);
                self.files_click(x, y, n);
            }
            return;
        }
        if self.settings.is_some() && self.chrome.content.contains(x, y) {
            if button == MouseButton::Left {
                self.settings_click(x, y);
            } else {
                self.non_keyboard_shortcut(chord);
            }
            return;
        }
        if self.tabs.is_empty() && self.chrome.content.contains(x, y) {
            if button == MouseButton::Left {
                let page = self.start_page();
                if let Some(i) = page.hit(x, y) {
                    self.start_selected = i;
                    self.launch_start_entry(page.entries[i]);
                }
            } else {
                self.non_keyboard_shortcut(chord);
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
        if !self.tab_bar_hidden() && self.chrome.tab_bar.contains(x, y) {
            if self.chrome.settings.contains(x, y) {
                if button == MouseButton::Left {
                    self.drag = Drag::SettingsButton;
                }
                return;
            }
            if self.chrome.tab_menu.contains(x, y) {
                if button == MouseButton::Left {
                    self.drag = Drag::TabMenu;
                }
                return;
            }
            if self.chrome.plus.contains(x, y) {
                self.drag = Drag::Plus;
                return;
            }
            if let Some(i) = self.chrome.tabs.iter().position(|t| t.rect.contains(x, y)) {
                match button {
                    MouseButton::Middle => self.close_tab_asking(i),
                    MouseButton::Left if self.chrome.tabs[i].close.contains(x, y) => self.drag = Drag::TabClose(i),
                    MouseButton::Left if self.click_count(x, y, clicks) >= 2 => self.start_rename(i),
                    MouseButton::Left => {
                        let slot = &self.chrome.tabs[i];
                        let grab = (x - slot.rect.x, y - slot.rect.y);
                        self.drag = Drag::Tab { index: i, start: (x, y), grab, moved: false };
                    }
                    MouseButton::Right => self.open_tab_menu(i, x, y),
                    _ => { self.non_keyboard_shortcut(chord); }
                }
            }
            return;
        }
        if let Some(pane) = self.pane_header_at(x, y) {
            if !self.rearranging() && self.pane_at(x, y).is_some_and(|(_, rect)| self.ssh_toolbar_click(pane, rect, x, y, button)) {
                return;
            }
            match button {
                MouseButton::Left => {
                    self.focus_pane(pane);
                    if self.click_count(x, y, clicks) >= 2 {
                        self.start_pane_rename(pane);
                    } else {
                        self.start_pane_drag(pane, x, y);
                    }
                }
                MouseButton::Right => {
                    self.focus_pane(pane);
                    self.open_terminal_menu(x, y);
                }
                _ => {}
            }
            return;
        }
        if button == MouseButton::Left && self.mouse_mods.alt && self.rearranging() {
            if let Some((pane, _)) = self.pane_at(x, y) {
                self.focus_pane(pane);
                self.start_pane_drag(pane, x, y);
                return;
            }
        }
        if let Some(s) = self.splitter_at(x, y) {
            self.drag = Drag::Splitter { path: s.path.clone(), axis: s.axis, span: s.span };
            return;
        }
        let Some((pane, rect)) = self.pane_at(x, y) else {
            self.non_keyboard_shortcut(chord);
            return;
        };
        if self.ssh_auth_click(pane, self.pane_terminal_rect(pane, rect), x, y, button) {
            if button == MouseButton::Left { self.focus_pane(pane); }
            return;
        }
        if self.broadcast.is_some() && self.broadcast_targets().contains(&pane) && y >= rect.bottom() - view::hint_height(self.ui()) {
            self.broadcast = None;
            self.dirty = true;
            return;
        }
        self.focus_pane(pane);
        let held = self.mouse_mods;
        if !held.shift && self.report(pane, rect, MouseAction::Press(button), x, y, held) {
            self.drag = Drag::Report { pane, button };
            return;
        }
        let shift_selection = held.shift && self.panes.get(&pane).is_some_and(|p| p.terminal.lock().mode().intersects(TermMode::MOUSE_MODE));
        if !shift_selection && self.non_keyboard_shortcut(chord) {
            return;
        }
        match button {
            MouseButton::Left => {
                let n = self.click_count(x, y, clicks);
                let (line, col, side) = self.cell_at(pane, rect, x, y);
                let ty = match n {
                    1 if held.alt => SelectionType::Block,
                    1 => SelectionType::Simple,
                    2 => SelectionType::Semantic,
                    _ => SelectionType::Lines,
                };
                if let Some(p) = self.panes.get(&pane) {
                    let mut t = p.terminal.lock();
                    let point = t.viewport_point(line, col);
                    if n == 1 && held.shift && t.has_selection() {
                        t.update_selection(point, side);
                    } else {
                        t.start_selection(ty, point, side);
                        if n > 1 {
                            t.update_selection(point, side);
                        }
                    }
                }
                let open_link = match self.config.terminal.link_modifier {
                    LinkModifier::None => true,
                    LinkModifier::Ctrl => held.ctrl,
                    LinkModifier::Shift => held.shift,
                };
                if open_link {
                    self.hover_link = self.link_at(x, y);
                }
                self.drag = Drag::Select { pane, open_link };
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
        if button == MouseButton::Left && self.config_editor_mouse_up() {
            self.dirty = true;
            return;
        }
        if matches!(self.drag, Drag::SettingsSlider { .. } | Drag::SettingsScrollbar { .. }) && button != MouseButton::Left { return; }
        let drag = std::mem::replace(&mut self.drag, Drag::None);
        self.dirty = true;
        match drag {
            Drag::SidebarResize { width } if button == MouseButton::Left => self.save_sidebar_width(width),
            Drag::Pane { pane, moved: true, .. } if button == MouseButton::Left => self.finish_pane_drag(pane, x, y),
            Drag::Tab { index, moved, .. } if button == MouseButton::Left => self.finish_tab_drag(index, moved, x, y),
            Drag::SettingsSlider { item, track, .. } if self.settings.is_some() => self.finish_settings_slider(item, track, x),
            Drag::SettingsScrollbar { page, grab } => self.drag_settings_scrollbar(page, grab, y),
            Drag::Select { pane, open_link } => {
                let has = self.panes.get(&pane).is_some_and(|p| p.terminal.lock().has_selection());
                if !has {
                    self.clear_selection(pane);
                    if open_link && button == MouseButton::Left {
                        if let Some(link) = self.link_at(x, y).filter(|l| self.hover_link.as_ref() == Some(l)) {
                            self.effects.push(Effect::Open(open_target(&link.target)));
                        }
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
            Drag::Button(area) if self.hit_test(x, y) == area => self.run(match area {
                HitArea::Minimize => Action::MinimizeWindow,
                HitArea::Maximize => Action::ToggleMaximize,
                _ => Action::CloseWindow,
            }),
            Drag::TabClose(i) if self.chrome.tabs.get(i).is_some_and(|t| t.close.contains(x, y)) => self.close_tab_asking(i),
            Drag::TabMenu if button == MouseButton::Left && self.chrome.tab_menu.contains(x, y) => {
                let menu = self.chrome.tab_menu;
                self.open_tab_menu(self.active, menu.x, menu.bottom());
            }
            Drag::SettingsButton if button == MouseButton::Left && !self.tab_bar_hidden() && self.chrome.settings.contains(x, y) => {
                self.run(Action::Settings);
            }
            Drag::Plus if self.chrome.plus.contains(x, y) => self.open_default_tab(),
            _ => {}
        }
    }

    fn on_wheel(&mut self, dx: f32, dy: f32, pixels: Option<f32>) {
        use crate::input::WheelDirection;
        if !dx.is_finite() || !dy.is_finite() { return; }
        let Some(direction) = WheelDirection::from_delta(dx, dy) else { return };
        let chord = Chord { key: Key::Wheel(direction), mods: self.mouse_mods };
        if self.capture_shortcut(chord) {
            return;
        }
        let vertical = matches!(direction, WheelDirection::Up | WheelDirection::Down);
        if self.menu.is_some() {
            return self.close_menu();
        }
        if vertical && self.update_wheel(dy) {
            return;
        }
        if vertical && self.scroll_overlay_wheel(dy, pixels) { return; }
        if let Some(o) = self.overlay.as_mut() {
            if vertical {
                o.step(if dy > 0.0 { -3 } else { 3 });
                self.dirty = true;
            }
            return;
        }
        let (x, y) = self.mouse;
        if vertical && self.sidebar_wheel(x, y, dy, pixels) {
            return;
        }
        if self.files.is_some() {
            if vertical {
                self.files_wheel(dy, pixels);
            }
            return;
        }
        if self.settings.is_some() {
            if !self.non_keyboard_shortcut(chord) && vertical {
                self.settings_wheel(dy, pixels);
            }
            return;
        }
        let Some((pane, rect)) = self.pane_at(x, y) else {
            self.non_keyboard_shortcut(chord);
            return;
        };
        if self.pane_header(pane, rect).contains(x, y) {
            self.non_keyboard_shortcut(chord);
            return;
        }
        if self.ssh_auth.contains_key(&pane) {
            if vertical { self.ssh_auth_wheel(pane, self.pane_terminal_rect(pane, rect), dy); }
            return;
        }
        let action = match direction {
            WheelDirection::Up => MouseAction::WheelUp,
            WheelDirection::Down => MouseAction::WheelDown,
            WheelDirection::Left => MouseAction::WheelLeft,
            WheelDirection::Right => MouseAction::WheelRight,
        };
        if chord.mods.any() && self.non_keyboard_shortcut(chord)
            || self.report(pane, rect, action, x, y, chord.mods)
            || !chord.mods.any() && self.non_keyboard_shortcut(chord)
            || !vertical
        {
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
        if self.tab_bar_hidden() || self.menu_open() {
            return HitArea::Client;
        }
        let c = &self.chrome;
        if c.min.contains(x, y) {
            HitArea::Minimize
        } else if c.max.contains(x, y) {
            HitArea::Maximize
        } else if c.close.contains(x, y) {
            HitArea::Close
        } else if c.caption.contains(x, y) && !c.plus.contains(x, y) && !c.tab_menu.contains(x, y)
            && !c.settings.contains(x, y) && !c.tabs.iter().any(|t| t.rect.contains(x, y))
        {
            HitArea::Caption
        } else {
            HitArea::Client
        }
    }

    pub fn woken(&mut self) {
        while let Ok((pane, generation, ev)) = self.events.try_recv() {
            if self.panes.get(&pane).is_none_or(|p| p.generation != generation) {
                continue;
            }
            match ev {
                VtEvent::Write(bytes) => {
                    self.effects.push(Effect::Write { pane, bytes });
                }
                VtEvent::Title(t) => {
                    if let Some(p) = self.panes.get_mut(&pane).filter(|p| p.profile.keep_tab_title != Some(true)) {
                        p.title = Some(t).filter(|t| !t.trim().is_empty() && !is_program_path(t));
                    }
                }
                VtEvent::ResetTitle => {
                    if let Some(p) = self.panes.get_mut(&pane).filter(|p| p.profile.keep_tab_title != Some(true)) {
                        p.title = None;
                    }
                }
                VtEvent::Bell => self.ring(pane),
                VtEvent::Clipboard(text) => self.effects.push(Effect::SetClipboard(text)),
            }
        }
        self.scan_prompts();
        self.poll_background_activity(Instant::now());
        self.dirty = true;
    }

    pub fn pane_clears_after_connect(&self, pane: PaneId) -> bool {
        self.panes.get(&pane).is_some_and(|p| p.profile.clear_after_connect == Some(true))
    }

    pub fn exited(&mut self, pane: PaneId, code: Option<u32>) {
        self.poll_background_activity(Instant::now());
        let tab_key = self.tabs.iter().find(|tab| tab.root.contains(pane)).map(|tab| tab.key);
        let options = self.options();
        let Some(p) = self.panes.get_mut(&pane) else { return };
        if p.exited.is_some() {
            return;
        }
        p.exited = Some(code);
        p.ssh_connecting = false;
        self.ssh_auth.remove(&pane);
        self.ssh_forwards.remove(&pane);
        self.ssh_forward_form.take_if(|form| form.pane == pane);
        if self.overlay.as_ref().is_some_and(|overlay| overlay.kind == OverlayKind::SshPorts(pane)) { self.overlay = None; }
        p.pid = None;
        self.offers.remove(&pane);
        self.prompts.remove(&pane);
        match p.profile.exit_decision(code) {
            ExitDecision::Close => self.close_pane(pane),
            ExitDecision::Reconnect if p.retry_launch.is_some() => {
                let cwd = p.retry_launch.as_ref().and_then(|launch| launch.cwd.clone());
                p.generation += 1;
                let sink = Self::terminal_sink(self.sender.clone(), self.wake.clone(), pane, p.generation);
                let mut terminal = p.terminal.lock().next_session(options, sink, cwd);
                let notice = crate::i18n::t("[Reconnecting in 1 second. Press any key to cancel.]");
                terminal.feed(format!("\r\n\x1b[0;2m{notice}\x1b[0m\r\n").as_bytes());
                p.terminal = Arc::new(FairMutex::new(terminal));
                p.title = None;
                p.reconnect_at = Some(Instant::now() + RECONNECT_DELAY);
                self.schedule_timer();
            }
            _ => {
                let notice = match code {
                    Some(c) => crate::i18n::tf("[Process exited with code {code}. Press any key to close.]", &[("code", &c.to_string())]),
                    None => crate::i18n::t("[Process exited. Press any key to close.]").to_owned(),
                };
                p.terminal.lock().feed(format!("\r\n\x1b[0;2m{notice}\x1b[0m\r\n").as_bytes());
            }
        }
        if let Some(p) = self.panes.get_mut(&pane) {
            p.activity_seq = p.terminal.lock().output_seq();
        }
        if let Some(key) = tab_key {
            self.notify_session_exit(key);
        }
        self.dirty = true;
    }

    fn cancel_focused_reconnect(&mut self) -> bool {
        let Some(p) = self.focused_pane().and_then(|id| self.panes.get_mut(&id)) else { return false };
        if p.reconnect_at.take().is_none() {
            return false;
        }
        let notice = crate::i18n::t("[Reconnect cancelled. Press any key to close.]");
        p.terminal.lock().feed(format!("\r\n\x1b[0;2m{notice}\x1b[0m\r\n").as_bytes());
        self.dirty = true;
        true
    }

    pub fn set_can_elevate(&mut self, can: bool) {
        self.can_elevate = can;
    }

    pub fn can_elevate(&self) -> bool {
        self.can_elevate
    }

    fn open_admin_tab(&mut self, mut profile: Profile) {
        let local = profile.is_local();
        if !self.can_elevate || !local {
            return;
        }
        profile.elevated = true;
        self.open_tab(profile);
    }

    pub fn spawn_failed(&mut self, pane: PaneId, message: &str) {
        if let Some(p) = self.panes.get_mut(&pane) {
            p.ssh_connecting = false;
            p.terminal.lock().feed(format!("\x1b[31m{message}\x1b[0m\r\n").as_bytes());
            if matches!(p.profile.on_exit, OnExit::Auto | OnExit::Inherit) {
                p.exited = Some(None);
                self.dirty = true;
            } else {
                self.exited(pane, None);
            }
        }
    }

    pub fn needs_redraw(&self) -> bool {
        self.dirty
    }

    pub(crate) fn mark_drawn(&mut self, now: Instant) {
        self.last_frame = Some(now);
        self.dirty = self.motion && self.animations();
        let title = self.tab_title(self.active);
        let title = if title.is_empty() { "Aster".to_owned() } else { title };
        if title != self.title {
            self.title = title.clone();
            self.effects.push(Effect::Title(title));
        }
        self.schedule_timer_at(now);
    }

    pub(crate) fn schedule_timer(&mut self) {
        self.schedule_timer_at(self.clock());
    }

    fn schedule_timer_at(&mut self, now: Instant) {
        let next = [
            self.blinking().then(|| self.blink.deadline()),
            self.bell.map(|(_, start)| start + BELL_FLASH),
            self.tab_toast.as_ref().map(|toast| toast.deadline(now, self.animations())),
            self.tabs.iter().filter_map(|t| t.command_watch.deadline()).min(),
            self.panes.values().filter_map(|p| p.reconnect_at).min(),
        ].into_iter().flatten().min();
        if let Some(due) = next.filter(|&due| self.timer_due.is_none_or(|armed| due < armed)) {
            let left = due.saturating_duration_since(now).as_millis().max(1) as u32;
            self.effects.push(Effect::Timer(left));
            self.timer_due = Some(due);
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
        if self.config.read_only() {
            return self.warn_read_only_config();
        }
        let text = self.config.to_text();
        self.effects.push(Effect::SaveConfig(text));
    }

    pub fn warn_read_only_config(&mut self) {
        if self.config.read_only() {
            self.toast(self.config.read_only_message(), Duration::from_secs(8));
        }
    }

    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }
}

pub fn dropped_text(profile: &Profile, paths: &[String]) -> String {
    use aster_proto::quote::{quote_paths, shell_kind, ShellKind};
    let wsl = profile.is_wsl();
    let kind = if profile.ssh.is_some() || wsl { ShellKind::Unix } else { shell_kind(&profile.command) };
    let paths: Vec<String> = paths.iter().map(|p| if wsl { wsl_path(p) } else { p.clone() }).collect();
    quote_paths(&paths, kind)
}

pub fn wsl_path(path: &str) -> String {
    let b = path.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        format!("/mnt/{}{}", (b[0] as char).to_ascii_lowercase(), path[2..].replace('\\', "/"))
    } else {
        path.to_owned()
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
    if let Some(dir) = inherited.as_ref().or(profile.cwd.as_ref()).filter(|dir| dir.starts_with('/')) {
        if let Some(next) = args.iter().position(|a| a == "--cd").and_then(|i| args.get_mut(i + 1)) {
            next.clone_from(dir);
            return (args, profile.cwd.clone().filter(|cwd| !cwd.starts_with('/')).or(home));
        }
    }
    let Some(dir) = inherited else { return (args, profile.cwd.clone().or(home)) };
    if dir.starts_with('/') {
        return (args, profile.cwd.clone().or(home));
    }
    (args, Some(dir))
}

#[path = "shortcut_input.rs"]
mod shortcut_input;

#[cfg(test)]
#[path = "../test/app.rs"]
mod tests;

pub fn report_url() -> String {
    let body = format!("**What happened**\n\n\n**What you expected**\n\n\n**Steps**\n\n1. \n\n---\nAster {} on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    let encoded: String = body
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => char::from(b).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("https://github.com/brano-san/Aster/issues/new?body={encoded}")
}
