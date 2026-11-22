use crate::config_file::ConfigAccess;
use crate::session::{Exit, Session};
use crate::secrets::{Secrets, Reply, SecretUse, System, VaultCommand, VaultUpdate, VaultWorker, WindowSecrets};
use aster_core::app::{App, Effect};
use aster_core::config::Config;
use aster_core::input::{Event, HitArea};
use aster_core::panes::PaneId;
use aster_core::profile::Profile;
use aster_core::settings::{VaultIssue, VaultView};
use aster_core::vault_settings::VaultReply;
use aster_core::session::{SessionState, WindowState};
use aster_gfx::text::{Gfx, TextBackend};
use aster_core::config::{Dock, SshClient};
use aster_core::keymap::Chord;
use aster_platform::{DockSpec, Edge, Platform, RenderError, Renderer, SpawnRequest, Waker, Window, WindowHandler, WindowId, WindowOptions, WindowSystem};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const ATLAS: u32 = 2048;

type Win<P> = <<P as Platform>::Windows as WindowSystem>::Window;
type Device<P> = <<P as Platform>::Renderer as Renderer>::Device;

struct State<P: Platform> {
    app: App,
    gfx: Gfx<P::Text>,
    renderer: Option<P::Renderer>,
    flow: crate::session::OutputFlow,
    output: HashMap<PaneId, crate::session::RawOutput>,
}

enum StoreEvent {
    Launcher(Option<WindowId>, std::io::Result<bool>),
    BackgroundImage { window: WindowId, revision: u64, bitmap: Option<aster_core::background::Bitmap> },
    FontFamilies { monospace: Vec<String>, all: Vec<String> },
    CommandProbe { window: WindowId, request: u64, busy: bool },
    Vault(VaultUpdate),
    VaultReply(WindowId, u64, Result<VaultReply, VaultIssue>),
    VaultAccess(WindowId, AccessPrompt),
    ConfigEncryption(bool),
    ConfigOpened(WindowId, u64, Result<Box<Config>, VaultIssue>),
    Config(u64, Option<SystemTime>, Result<Box<Config>, String>),
    ConfigStatus(u64, Option<String>),
    StartMenuShortcut(Option<WindowId>, Result<bool, aster_platform::PlatformError>),
    DefaultConfig(WindowId, std::io::Result<PathBuf>),
    Known { window: WindowId, pane: PaneId, key: String, line: usize, known: bool },
    Fill { window: WindowId, pane: PaneId, key: String, request: u64, secret: Option<String> },
}

#[derive(Clone)]
struct StoreEvents<W> {
    tx: Sender<StoreEvent>,
    waker: W,
}

impl<W: Waker> StoreEvents<W> {
    fn post(&self, event: StoreEvent) {
        if self.tx.send(event).is_ok() {
            self.waker.wake();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfigStartup {
    Ready,
    Prompt,
    Skipped,
}

struct AccessPrompt {
    issue: Option<VaultIssue>,
    reply: Reply<Option<String>>,
}

pub struct Handler<P: Platform> {
    desktop: crate::desktop::Worker,
    launcher_installed: Option<bool>,
    font_families: Option<(Vec<String>, Vec<String>)>,
    config: Config,
    profiles: Vec<Profile>,
    waker: <P::Windows as WindowSystem>::Waker,
    device: Option<Device<P>>,
    windows: HashMap<WindowId, State<P>>,
    sessions: HashMap<(WindowId, PaneId), Session>,
    serials: HashMap<(WindowId, PaneId), crate::serial::Serial>,
    exits: Receiver<Exit>,
    remotes: HashMap<(WindowId, PaneId), tokio::sync::mpsc::UnboundedSender<aster_ssh::console::Input>>,
    file_tx: Sender<(WindowId, PaneId, aster_core::files::FileReply)>,
    file_rx: Receiver<(WindowId, PaneId, aster_core::files::FileReply)>,
    note_tx: Sender<crate::session::TransferNote>,
    note_rx: Receiver<crate::session::TransferNote>,
    local_spawns: crate::local_spawn::LocalSpawns,
    exit_tx: Sender<Exit>,
    config_path: Option<PathBuf>,
    config_access: Arc<Mutex<ConfigAccess>>,
    config_startup: ConfigStartup,
    startup_config: Option<(WindowId, Box<Config>)>,
    config_stamp: Option<SystemTime>,
    settings_revision: u64,
    secrets: Secrets,
    vault_view: VaultView,
    vault_access: HashMap<WindowId, VecDeque<AccessPrompt>>,
    store_events: StoreEvents<<P::Windows as WindowSystem>::Waker>,
    store_rx: Receiver<StoreEvent>,
    ssh: Option<String>,
    pending: Vec<WindowState>,
    session_saved: bool,
    applied: Option<(String, Option<DockSpec>)>,
    window_prefs_dirty: bool,
    requests: Option<std::sync::mpsc::Receiver<String>>,
    startup: Option<crate::cli::Request>,
    last_focused: Option<WindowId>,
    recent: Vec<String>,
    recent_folders: Vec<String>,
    jump_list: Option<aster_core::jump_list::JumpList>,
    updates: crate::update::UpdateWorker,
    update_window: Option<WindowId>,
    shell_integration: crate::shell_integration::ShellIntegration,
    start_menu_shortcut: bool,
    restarting: bool,
}

fn ssh_hosts<P: Platform>(ssh: &str) -> Vec<Profile> {
    let Some(home) = P::home_dir() else { return Vec::new() };
    let dir = home.join(".ssh");
    let Ok(text) = std::fs::read_to_string(dir.join("config")) else { return Vec::new() };
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok();
    aster_ssh::config::parse(&text, &dir, &home, &read).iter().map(|h| aster_core::ssh::host_profile(&h.alias, ssh)).collect()
}

fn capturing() -> bool {
    std::env::var_os("ASTER_CAPTURE").is_some() && std::env::var_os("ASTER_CAPTURE_SESSION").is_none()
}

fn queue_background_request(startup: &mut Option<crate::cli::Request>, requests: Option<&std::sync::mpsc::Receiver<String>>) -> bool {
    if startup.is_some() {
        return false;
    }
    *startup = requests.and_then(|rx| rx.try_iter().find_map(|text| crate::cli::Request::decode(&text).filter(|r| r.command != crate::cli::Command::Hidden)));
    startup.is_some()
}

fn initialize_tabs(app: &mut App, saved: Option<WindowState>, first: bool, command: Option<&crate::cli::Command>) {
    use crate::cli::Command;
    if saved.is_some_and(|state| app.restore(&state)) {
        return;
    }
    let open_default = match command {
        Some(Command::Paste { .. }) => true,
        Some(Command::Window | Command::Hidden) | None => !first || app.config().terminal.open_on_start,
        _ => false,
    };
    if open_default {
        app.open_default_tab();
    }
}

fn serve_request(app: &mut App, request: crate::cli::Request) -> bool {
    use crate::cli::Command;
    match request.command {
        Command::Window => return false,
        Command::Hidden => return true,
        Command::Open { dir } => {
            if let Some(profile) = app.default_profile() {
                app.open_tab_at(profile, Some(dir));
            }
        }
        Command::Run { command, dir } => app.run_command(&command, dir.or(request.cwd)),
        Command::Profile { name } => {
            if !app.open_named_profile(&name) {
                tracing::warn!("no profile named {name}");
            }
        }
        Command::Paste { text } => app.paste(text),
        Command::PastePath { path } => app.paste_path(&path),
    }
    true
}

fn populate_window(app: &mut App, saved: Option<WindowState>, first: bool, request: Option<crate::cli::Request>) {
    initialize_tabs(app, saved, first, request.as_ref().map(|r| &r.command));
    if first && !crate::update::restarted() {
        app.check_updates_on_start();
    }
    if let Some(request) = request {
        serve_request(app, request);
    }
}

fn load_session<P: Platform>() -> SessionState {
    if capturing() {
        return SessionState::default();
    }
    crate::session_path::<P>().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| SessionState::parse(&t)).unwrap_or_default()
}

impl<P: Platform> Handler<P> {
    pub fn new(config: Config, waker: <P::Windows as WindowSystem>::Waker, encrypted: bool) -> std::io::Result<(Self, VaultWorker)> {
        let mut profiles = P::shells();
        let ssh = P::ssh_command().or_else(|| (config.ssh.client == SshClient::Native).then(|| "ssh".to_owned()));
        if let Some(cmd) = &ssh {
            profiles.extend(ssh_hosts::<P>(cmd));
        }
        profiles.extend(crate::serial::profiles());
        let (exit_tx, exits) = unbounded();
        let (file_tx, file_rx) = unbounded();
        let (note_tx, note_rx) = unbounded();
        let local_spawns = crate::local_spawn::LocalSpawns::new()?;
        let config_path = crate::config_path::<P>();
        let path = config_path.clone();
        let config_access = Arc::new(Mutex::new(ConfigAccess::default()));
        let access = config_access.clone();
        let config_startup = if encrypted { ConfigStartup::Prompt } else { ConfigStartup::Ready };
        let (tx, store_rx) = unbounded();
        let store_events = StoreEvents { tx, waker: waker.clone() };
        let fonts = store_events.clone();
        if let Err(e) = std::thread::Builder::new().name("aster-font-families".into()).spawn(move || {
            fonts.post(StoreEvent::FontFamilies { monospace: P::font_families(true), all: P::font_families(false) });
        }) {
            tracing::warn!("font family worker: {e}");
        }
        let events = store_events.clone();
        let config_events = store_events.clone();
        let vault_view = VaultView { configured: !config.vault.sealed.is_empty(), ..VaultView::default() };
        let (secrets, worker) = Secrets::start(Arc::new(System::<P>::default()), config.vault.sealed.clone(), config.vault.remember_minutes, move |expected, sealed, rekey| {
            let Some(path) = &path else { return false };
            match access.lock().expect("config access").save_vault(path, expected, sealed, rekey) {
                Ok(encrypted) => {
                    config_events.post(StoreEvent::ConfigEncryption(encrypted));
                    true
                }
                Err(e) => {
                    tracing::error!("{}: {e}", path.display());
                    false
                }
            }
        }, move |update| events.post(StoreEvent::Vault(update)))?;
        let saved = load_session::<P>();
        let recent = saved.recent;
        let recent_folders = saved.recent_folders;
        let pending = if encrypted || config.terminal.recover_tabs || crate::update::restarted() { saved.windows } else { Vec::new() };
        let update_waker = waker.clone();
        let updates = crate::update::UpdateWorker::start::<P>(Arc::new(move || update_waker.wake()))?;
        let desktop = crate::desktop::Worker::start()?;
        let launcher_installed = cfg!(target_os = "linux").then_some(false);
        let shell_integration = crate::shell_integration::ShellIntegration::start::<P>()?;
        if cfg!(windows) && !capturing() {
            let events = store_events.clone();
            shell_integration.shortcut(None, move |result| events.post(StoreEvent::StartMenuShortcut(None, result)));
        }
        let handler = Self { desktop, launcher_installed, shell_integration, start_menu_shortcut: false, font_families: None, config, profiles, waker, device: None, windows: HashMap::new(), sessions: HashMap::new(), serials: HashMap::new(), exits, remotes: HashMap::new(), file_tx, file_rx, note_tx, note_rx, local_spawns, exit_tx, config_path, config_access, config_startup, startup_config: None, config_stamp: None, settings_revision: 0, secrets, vault_view, vault_access: HashMap::new(), store_events, store_rx, ssh, pending, session_saved: false, applied: None, window_prefs_dirty: false, requests: None, startup: None, last_focused: None, recent, recent_folders, jump_list: None, updates, update_window: None, restarting: false };
        if !encrypted {
            handler.update_launcher(&handler.config, None);
            handler.apply_autostart(&handler.config);
        }
        Ok((handler, worker))
    }

    pub fn set_instance(&mut self, requests: Option<std::sync::mpsc::Receiver<String>>, startup: Option<crate::cli::Request>) {
        self.requests = requests;
        self.startup = startup;
    }

    fn request_target(&self) -> Option<WindowId> {
        self.last_focused.filter(|id| self.windows.contains_key(id)).or_else(|| self.windows.keys().min().copied())
    }

    fn drain_requests(&mut self, window: &Win<P>) {
        if self.config_startup == ConfigStartup::Prompt || self.request_target() != Some(window.id()) {
            return;
        }
        let incoming: Vec<String> = self.requests.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for text in incoming {
            let Some(request) = crate::cli::Request::decode(&text).filter(|r| r.command != crate::cli::Command::Hidden) else { continue };
            let handled = self.windows.get_mut(&window.id()).is_some_and(|s| serve_request(&mut s.app, request));
            if !handled {
                let dark = self.windows.get(&window.id()).is_none_or(|s| s.app.is_dark());
                window.open_window(WindowOptions { title: "Aster".into(), placement: None, dark, visible: true });
            }
            window.set_visible(true);
            window.activate();
            window.request_redraw();
        }
    }

    fn save_config(&mut self, text: String) {
        let Ok(mut config) = Config::parse(&text) else { return };
        config.vault.sealed.clone_from(&self.config.vault.sealed);
        config.vault.encrypt_config = self.config.vault.encrypt_config;
        self.settings_revision += 1;
        self.apply_config(config.clone());
        if self.config_startup != ConfigStartup::Ready { return; }
        let events = self.store_events.clone();
        let revision = self.settings_revision;
        let path = self.config_path.clone();
        let access = self.config_access.clone();
        self.secrets.sync_config(move |sealed| {
            if let Some(path) = path {
                match access.lock().expect("config access").save_settings(&path, &mut config, sealed) {
                    Ok(()) => {
                        events.post(StoreEvent::ConfigStatus(revision, None));
                        return Some((config.vault.sealed, config.vault.remember_minutes));
                    }
                    Err(e) => {
                        tracing::error!("{}: {e}", path.display());
                        events.post(StoreEvent::ConfigStatus(revision, Some(e.to_string())));
                    }
                }
            }
            Some((sealed.to_owned(), config.vault.remember_minutes))
        });
    }

    fn unlock_config(&mut self, window: WindowId, request: u64, passphrase: String) {
        if self.config_startup != ConfigStartup::Prompt { return; }
        let path = self.config_path.clone();
        let access = self.config_access.clone();
        let events = self.store_events.clone();
        if !self.secrets.sync_config(move |_| {
            let result = path.ok_or(VaultIssue::ConfigReadFailed).and_then(|path| {
                access.lock().expect("config access").unlock(&path, &passphrase).map(Box::new)
            });
            events.post(StoreEvent::ConfigOpened(window, request, result));
            None
        }) {
            if let Some(state) = self.windows.get_mut(&window) {
                state.app.config_unlock_error(request, VaultIssue::Unavailable);
            }
        }
    }

    fn protect_config(&mut self, window: WindowId, request: u64, enabled: bool, passphrase: String) {
        let path = self.config_path.clone();
        let access = self.config_access.clone();
        let events = self.store_events.clone();
        if !self.secrets.sync_config(move |sealed| {
            let result = path.ok_or(VaultIssue::SaveFailed).and_then(|path| {
                access.lock().expect("config access").set_encryption(&path, sealed, &passphrase, enabled)
            });
            if result.is_ok() {
                events.post(StoreEvent::ConfigEncryption(enabled));
            }
            events.post(StoreEvent::VaultReply(window, request, result.map(|()| VaultReply::Applied)));
            None
        }) {
            if let Some(state) = self.windows.get_mut(&window) {
                state.app.vault_reply(request, Err(VaultIssue::Unavailable));
            }
        }
    }

    fn take_window_startup(&mut self) -> (Option<WindowState>, Option<crate::cli::Request>) {
        let saved = (!self.pending.is_empty()).then(|| self.pending.remove(0));
        let request = self.startup.take().filter(|r| r.command != crate::cli::Command::Window);
        (saved, request)
    }

    fn open_pending_windows(&self, window: &Win<P>, dark: bool, visible: bool) {
        for _ in &self.pending {
            window.open_window(WindowOptions { title: "Aster".into(), placement: None, dark, visible });
        }
    }

    fn finish_config_start(&mut self, window: &Win<P>, config: Option<Config>) {
        if self.config_startup != ConfigStartup::Prompt { return; }
        self.startup_config = None;
        if let Some(config) = config {
            if !config.terminal.recover_tabs && !crate::update::restarted() {
                self.pending.clear();
            }
            let sealed = config.vault.sealed.clone();
            let minutes = config.vault.remember_minutes;
            self.show_vault(VaultView { configured: !sealed.is_empty(), ..VaultView::default() });
            self.secrets.sync_config(move |_| Some((sealed, minutes)));
            self.apply_config(config);
            self.config_startup = ConfigStartup::Ready;
        } else {
            self.config_startup = ConfigStartup::Skipped;
            self.pending.clear();
            self.recent.clear();
            self.recent_folders.clear();
        }
        let (saved, request) = self.take_window_startup();
        if let Some(state) = self.windows.get_mut(&window.id()) {
            state.app.dismiss_config_unlock();
            state.app.set_recent(self.recent.clone());
            populate_window(&mut state.app, saved, true, request);
            let dark = state.app.is_dark();
            window.set_dark(dark);
            self.open_pending_windows(window, dark, true);
            self.apply_window_prefs(window);
            window.request_redraw();
        }
    }

    fn show_config_status(&mut self, error: Option<String>) {
        for state in self.windows.values_mut() {
            state.app.set_config_status(error.clone());
        }
    }

    fn apply_config(&mut self, config: Config) {
        for s in self.windows.values_mut() {
            if s.app.config() != &config {
                s.app.set_config(config.clone());
            }
        }
        let starting = self.config_startup == ConfigStartup::Prompt;
        let explorer = config.system.explorer_menu;
        if (starting && (explorer || cfg!(target_os = "linux"))) || explorer != self.config.system.explorer_menu || (explorer && config.appearance.language != self.config.appearance.language) {
            self.apply_explorer_menu(explorer);
        }
        if starting || config.profiles != self.config.profiles {
            self.update_launcher(&config, None);
        }
        if starting || config.system.autostart != self.config.system.autostart || (config.system.autostart && crate::background_mode(&config) != crate::background_mode(&self.config)) {
            self.apply_autostart(&config);
        }
        if (starting && (config.system.url_scheme || !capturing())) || config.system.url_scheme != self.config.system.url_scheme {
            self.shell_integration.url_scheme(config.system.url_scheme);
        }
        if config.appearance != self.config.appearance {
            self.window_prefs_dirty = true;
            self.waker.wake();
        }
        self.config = config;
        self.refresh_jump_list();
    }

    fn refresh_jump_list(&mut self) {
        if cfg!(windows) && !capturing() {
            let next = aster_core::jump_list::build(&self.config, &self.profiles, &self.recent, &self.recent_folders);
            if self.jump_list.as_ref() != Some(&next) {
                self.shell_integration.jump_list(next.clone());
                self.jump_list = Some(next);
            }
        }
    }

    fn update_launcher(&self, config: &Config, change: Option<(WindowId, bool)>) {
        if self.launcher_installed.is_none() || std::env::var_os("ASTER_CAPTURE").is_some() {
            return;
        }
        let profiles: Vec<_> = config.profiles.iter().map(|p| p.name.clone()).collect();
        let events = self.store_events.clone();
        self.desktop.send(move || {
            let result = P::update_launcher(&profiles, change.map(|(_, install)| install));
            events.post(StoreEvent::Launcher(change.map(|(id, _)| id), result));
        });
    }

    fn apply_explorer_menu(&self, enabled: bool) {
        if std::env::var_os("ASTER_CAPTURE").is_some() {
            return;
        }
        let label = aster_core::settings::explorer_label();
        if cfg!(windows) {
            self.shell_integration.explorer(label, aster_core::settings::explorer_paste_label(), enabled);
            return;
        }
        self.desktop.send(move || {
            if !P::set_explorer_menu(label, enabled) {
                tracing::warn!("could not update the file manager menu");
            }
        });
    }

    fn apply_autostart(&self, config: &Config) {
        if std::env::var_os("ASTER_CAPTURE").is_some() {
            return;
        }
        let enabled = config.system.autostart;
        let args: &[&str] = if crate::background_mode(config) { &["--hidden"] } else { &[] };
        self.desktop.send(move || {
            if let Err(e) = P::set_autostart(enabled, args) {
                tracing::warn!("autostart: {e}");
            }
        });
    }

    fn reload_config(&mut self, force: bool) {
        if self.config_startup != ConfigStartup::Ready { return; }
        let Some(path) = self.config_path.clone() else { return };
        let events = self.store_events.clone();
        let revision = self.settings_revision;
        let previous = self.config_stamp;
        let access = self.config_access.clone();
        self.secrets.sync_config(move |_| {
            let stamp = std::fs::metadata(&path).and_then(|metadata| metadata.modified()).ok();
            if !force && stamp.is_some() && stamp == previous {
                return None;
            }
            match access.lock().expect("config access").load(&path) {
                Ok(config) => {
                    let vault = (config.vault.sealed.clone(), config.vault.remember_minutes);
                    events.post(StoreEvent::Config(revision, stamp, Ok(Box::new(config))));
                    Some(vault)
                }
                Err(e) => {
                    tracing::error!("could not load {}: {}", path.display(), e.kind());
                    events.post(StoreEvent::Config(revision, stamp, Err(e.to_string())));
                    None
                }
            }
        });
    }

    fn device(&mut self) -> Option<&Device<P>> {
        if self.device.is_none() {
            self.device = P::Renderer::device(false)
                .or_else(|e| {
                    tracing::warn!("hardware device: {e}");
                    P::Renderer::device(true)
                })
                .map_err(|e| tracing::error!("graphics device: {e}"))
                .ok();
        }
        self.device.as_ref()
    }

    fn apply(&mut self, window: &Win<P>) {
        let id = window.id();
        loop {
            let Some(state) = self.windows.get_mut(&id) else { return };
            let effects = state.app.take_effects();
            if effects.is_empty() {
                break;
            }
            for e in effects {
                self.effect(window, e);
            }
        }
        if let Some(state) = self.windows.get_mut(&id) {
            state.output.retain(|pane, _| state.app.terminal(*pane).is_some());
            if state.app.needs_redraw() {
                window.request_redraw();
            }
        }
    }

    fn effect(&mut self, window: &Win<P>, effect: Effect) {
        let id = window.id();
        match effect {
            Effect::Spawn { pane, launch, terminal, .. } if launch.serial.is_some() => {
                match crate::serial::Serial::new(launch.serial.as_deref().unwrap_or_default(), launch.stream) {
                    Ok(serial) => {
                        self.serials.insert((id, pane), serial);
                        self.start_serial(id, pane, terminal);
                    }
                    Err(error) => {
                        if let Some(state) = self.windows.get_mut(&id) {
                            state.app.spawn_failed(pane, &error);
                        }
                    }
                }
            }
            Effect::Spawn { pane, launch, terminal, .. } if launch.socket.is_some() => {
                let spawned = crate::net::connect(launch.socket.as_deref().unwrap_or_default(), launch.cols, launch.rows, launch.env.get("TERM").map(String::as_str), launch.stream.as_ref().is_none_or(|s| s.input_mode() == aster_core::profile::InputMode::Normal));
                let session = Session::start(self.session_start(id, pane), crate::stream::wrap(spawned, launch.stream.as_ref()), terminal);
                self.sessions.insert((id, pane), session);
            }
            Effect::Spawn { pane, launch, terminal, reconnecting } if launch.ssh.is_some() && self.config.ssh.client == SshClient::Native => {
                let destination = launch.ssh.unwrap_or_default();
                let options = crate::remote::Options {
                    profile: launch.ssh_options.map(|o| *o).unwrap_or_default(),
                    cwd: launch.cwd,
                    clear_after_connect: self.config.ssh.clear_after_connect && !reconnecting,
                    x11_display: self.config.ssh.x11_display.clone(),
                    share: self.config.ssh.share_connections,
                    forward_agent: self.config.ssh.forward_agent,
                    remember: self.config.ssh.remember_passwords,
                    secrets: Arc::new(self.window_secrets(id)),
                };
                let remote = crate::remote::connect(&destination, launch.cols, launch.rows, launch.env.get("TERM").map(String::as_str), P::home_dir(), options);
                let (files, tx, waker) = (remote.files, self.file_tx.clone(), self.waker.clone());
                let spawned = std::thread::Builder::new().name(format!("aster-files-{pane}")).spawn(move || {
                    for reply in files {
                        if tx.send((id, pane, crate::remote::file_reply(reply))).is_err() {
                            break;
                        }
                        waker.wake();
                    }
                });
                if let Err(e) = spawned {
                    tracing::error!("file reply thread: {e}");
                }
                self.remotes.insert((id, pane), remote.input);
                let session = Session::start(self.session_start(id, pane), crate::stream::wrap(remote.spawned, launch.stream.as_ref()), terminal);
                self.sessions.insert((id, pane), session);
            }
            Effect::Spawn { pane, launch, terminal, .. } => {
                let req = SpawnRequest {
                    command: launch.command,
                    args: launch.args,
                    cwd: launch.cwd.map(PathBuf::from),
                    env: launch.env.into_iter().collect(),
                    local_environment: launch.local_environment,
                    cols: launch.cols,
                    rows: launch.rows,
                };
                let request = crate::local_spawn::Request { key: (id, pane), request: req, terminal, stream: launch.stream, elevated: launch.elevated };
                let waker = self.waker.clone();
                if let Err(error) = self.local_spawns.launch::<P>(request, move || waker.wake()) {
                    tracing::error!("local spawn worker: {error}");
                    if let Some(s) = self.windows.get_mut(&id) {
                        s.app.spawn_failed(pane, &error.to_string());
                    }
                }
            }
            Effect::Write { pane, bytes } => {
                use crate::serial::KeysAction;
                match self.serials.get(&(id, pane)).map(|s| s.keys(&bytes)) {
                    Some(KeysAction::Reconnect) => {
                        if let Some(terminal) = self.windows.get(&id).and_then(|s| s.app.terminal(pane)).cloned() {
                            self.start_serial(id, pane, terminal);
                        }
                    }
                    Some(KeysAction::Close) => {
                        if let Some(s) = self.windows.get_mut(&id) {
                            s.app.close_pane(pane);
                        }
                    }
                    Some(KeysAction::Ignore) => {}
                    Some(KeysAction::Write) | None => {
                        if let Some(s) = self.sessions.get(&(id, pane)) {
                            s.write(bytes);
                        } else if let Some(pending) = self.local_spawns.pending((id, pane)) {
                            pending.input.push(bytes);
                        }
                    }
                }
            }
            Effect::SerialControl { pane, control } => {
                if let Some(serial) = self.serials.get_mut(&(id, pane)) {
                    serial.control(control);
                }
            }
            Effect::Resize { pane, cols, rows } => {
                if let Some(s) = self.sessions.get(&(id, pane)) {
                    s.pty.resize(cols, rows);
                } else if let Some(pending) = self.local_spawns.pending((id, pane)) {
                    pending.size = Some((cols, rows));
                }
            }
            Effect::Kill { pane } => {
                self.local_spawns.take_pending((id, pane));
                self.serials.remove(&(id, pane));
                if let Some(s) = self.sessions.remove(&(id, pane)) {
                    s.pty.kill();
                }
            }
            Effect::SetClipboard(text) => window.set_clipboard_text(&text),
            Effect::SetClipboardHtml { text, html } => window.set_clipboard_html(&text, &html),
            Effect::ExportBuffer { pane, text } => {
                if let Some(state) = self.windows.get(&id) {
                    if let Some(terminal) = state.app.terminal(pane) {
                        crate::session::OutputFile::Buffer(text).save(pane, P::downloads_dir, terminal.clone(), state.flow.wake.clone());
                    }
                }
            }
            Effect::SaveOutput { pane } => {
                if let Some(state) = self.windows.get(&id) {
                    if let Some(terminal) = state.app.terminal(pane) {
                        state.output.get(&pane).cloned().unwrap_or_default().save(pane, P::downloads_dir, terminal.clone(), state.flow.wake.clone());
                    }
                }
            }
            Effect::RequestPaste => self.request_paste(window, None),
            Effect::RequestVaultPaste(input) => self.request_paste(window, Some(input)),
            Effect::SaveConfig(text) => {
                self.save_config(text);
                if self.windows.keys().min() == Some(&id) {
                    self.apply_window_prefs(window);
                }
            }
            Effect::Open(target) => P::open(&target),
            Effect::RecentProfiles(recent) => {
                for s in self.windows.values_mut() {
                    s.app.set_recent(recent.clone());
                }
                self.recent = recent;
                self.refresh_jump_list();
            }
            Effect::OpenConfigFile => {
                if let Some(path) = &self.config_path {
                    P::open(&path.to_string_lossy());
                }
            }
            Effect::StartMenuShortcut(create) => {
                let events = self.store_events.clone();
                self.shell_integration.shortcut(Some(create), move |result| events.post(StoreEvent::StartMenuShortcut(Some(id), result)));
            }
            Effect::OpenLog | Effect::OpenConfigFolder => {
                if let Some(dir) = self.config_path.as_deref().and_then(|p| p.parent()) {
                    P::open(&dir.to_string_lossy());
                }
            }
            Effect::CheckConfigFile => self.reload_config(true),
            Effect::ApplicationsMenu(install) => self.update_launcher(&self.config, Some((id, install))),
            Effect::ShowDefaultConfig => {
                if let Some(dir) = self.config_path.as_deref().and_then(|p| p.parent()).map(ToOwned::to_owned) {
                    let events = self.store_events.clone();
                    self.secrets.sync_config(move |_| {
                        events.post(StoreEvent::DefaultConfig(id, crate::config_file::write_defaults(&dir)));
                        None
                    });
                }
            }
            Effect::Title(t) => window.set_title(&t),
            Effect::Cursor(c) => window.set_cursor(c),
            Effect::Minimize => window.minimize(),
            Effect::Quit => window.request_quit(),
            Effect::ToggleMaximize => window.toggle_maximize(),
            Effect::ToggleFullscreen => {
                window.toggle_fullscreen();
                if let Some(s) = self.windows.get_mut(&id) {
                    s.app.set_fullscreen(window.is_fullscreen());
                }
            }
            Effect::Progress(progress) => window.set_progress(progress),
            Effect::Close => {
                if self.windows.len() == 1 && !std::mem::take(&mut self.session_saved) {
                    self.save_session();
                }
                window.close();
            }
            Effect::NewWindow => window.open_window(WindowOptions { title: "Aster".into(), placement: None, dark: self.windows.get(&id).is_none_or(|s| s.app.is_dark()), visible: true }),
            Effect::Timer(ms) => window.set_timer(ms),
            Effect::Attention => window.request_attention(),
            Effect::Notify(notification) => window.notify(notification),
            Effect::ProbeCommand { request, pids } => {
                let events = self.store_events.clone();
                let started = std::thread::Builder::new().name("aster-command-probe".into()).spawn(move || {
                    let busy = pids.into_iter().any(|pid| !P::child_processes(pid).is_empty());
                    events.post(StoreEvent::CommandProbe { window: id, request, busy });
                });
                if let Err(e) = started {
                    tracing::error!("command probe: {e}");
                    if let Some(state) = self.windows.get_mut(&id) {
                        state.app.command_probe_finished(request, None);
                    }
                }
            }
            Effect::CheckSecret { pane, key } => {
                if self.config.vault.remember_minutes == 0 && self.vault_view.configured {
                    if let Some(state) = self.windows.get_mut(&id) {
                        state.app.secret_known(pane, self.vault_view.entries.iter().any(|entry| entry.key == key));
                    }
                } else if let Some(prompt) = self.windows.get(&id).and_then(|s| s.app.password_prompt(pane)) {
                    let line = prompt.line;
                    let events = self.store_events.clone();
                    self.secrets.read(key.clone(), move |secret| events.post(StoreEvent::Known { window: id, pane, key, line, known: secret.is_some() }));
                }
            }
            Effect::FillSecret { pane, key } => {
                if let Some(request) = self.windows.get(&id).and_then(|s| s.app.pending_secret_fill(pane)) {
                    let events = self.store_events.clone();
                    self.window_secrets(id).use_secret(SecretUse::Read(key.clone()), move |result| {
                        events.post(StoreEvent::Fill { window: id, pane, key, request, secret: result.ok().flatten() });
                    });
                }
            }
            Effect::SaveSecret { key, value } => {
                self.window_secrets(id).use_secret(SecretUse::Write(key, Some(value)), |result| {
                    if result.is_err() {
                        tracing::warn!("could not save a prompt password");
                    }
                });
            }
            Effect::UnlockConfig { request, passphrase } => self.unlock_config(id, request, passphrase),
            Effect::CancelConfigUnlock if self.config_startup == ConfigStartup::Prompt => {
                let access = self.config_access.clone();
                self.secrets.sync_config(move |_| {
                    access.lock().expect("config access").skip();
                    None
                });
                self.finish_config_start(window, None);
            }
            Effect::CancelConfigUnlock => {}
            Effect::VaultAccessInput(password) => self.answer_vault_access(id, password),
            Effect::ProtectConfig { request, enabled, passphrase } => self.protect_config(id, request, enabled, passphrase),
            Effect::VaultCreate { passphrase } => self.vault_command(id, VaultCommand::Create(passphrase)),
            Effect::VaultUnlock { passphrase } => self.vault_command(id, VaultCommand::Unlock(passphrase)),
            Effect::VaultLock => self.vault_command(id, VaultCommand::Lock),
            Effect::VaultChange { old, new } => self.vault_command(id, VaultCommand::Change { old, new }),
            Effect::VaultErase { passphrase } => self.vault_command(id, VaultCommand::Erase(passphrase)),
            Effect::VaultManage { action, passphrase, request } => {
                let events = self.store_events.clone();
                self.secrets.manage(action, passphrase, move |result| events.post(StoreEvent::VaultReply(id, request, result)));
            }
            Effect::SendFiles { pane, files } => {
                if let Some(s) = self.sessions.get(&(id, pane)) {
                    s.send_files(&files);
                }
            }
            Effect::CancelTransfer { pane } => {
                if let Some(s) = self.sessions.get(&(id, pane)) {
                    s.cancel_transfer();
                }
            }
            Effect::AcceptTransfer { pane } => {
                if let Some(s) = self.sessions.get(&(id, pane)) {
                    s.accept_transfer();
                }
            }
            Effect::Files { pane, request } => {
                let downloads = P::downloads_dir().unwrap_or_else(std::env::temp_dir);
                let sent = self.remotes.get(&(id, pane)).is_some_and(|r| r.send(aster_ssh::console::Input::Sftp(crate::remote::file_request(request, downloads))).is_ok());
                if !sent {
                    if let Some(s) = self.windows.get_mut(&id) {
                        s.app.files_reply(pane, aster_core::files::FileReply::Failed { id: None, error: "the connection is closed".into() });
                    }
                }
            }
            Effect::Surface(backdrop) => {
                let got = window.set_surface(backdrop);
                if let Some(s) = self.windows.get_mut(&id) {
                    s.app.set_surface(got.translucent, got.backdrop);
                }
            }
            Effect::LoadBackgroundImage { revision, path } => {
                let mut path = PathBuf::from(path);
                if path.is_relative() {
                    if let Some(base) = self.config_path.as_deref().and_then(std::path::Path::parent) {
                        path = base.join(path);
                    }
                }
                let events = self.store_events.clone();
                if let Err(error) = crate::background::request(path, move |bitmap| {
                    events.post(StoreEvent::BackgroundImage { window: id, revision, bitmap });
                }) {
                    tracing::warn!("background image worker: {error}");
                    if let Some(state) = self.windows.get_mut(&id) {
                        state.app.background_image_loaded(revision, None);
                    }
                }
            }
            Effect::Beep => {
                if !P::beep() {
                    if let Some(s) = self.windows.get_mut(&id) {
                        s.app.flash_bell();
                        window.request_redraw();
                    }
                }
            }
            Effect::ImeArea(r) => window.set_ime_area(r),
            Effect::Update(request) => {
                self.update_window = Some(id);
                self.updates.submit(request);
            }
        }
    }

    fn request_paste(&mut self, window: &Win<P>, input: Option<u64>) {
        if let (Some(text), Some(s)) = (window.clipboard_text(input), self.windows.get_mut(&window.id())) {
            match input {
                Some(input) => s.app.paste_vault(input, text),
                None => s.app.paste(text),
            }
        }
    }

    fn dock_spec(&self) -> Option<DockSpec> {
        let a = &self.config.appearance;
        let edge = match a.dock {
            Dock::Off => return None,
            Dock::Top => Edge::Top,
            Dock::Bottom => Edge::Bottom,
            Dock::Left => Edge::Left,
            Dock::Right => Edge::Right,
        };
        Some(DockSpec { edge, fill: a.dock_fill, space: a.dock_space, on_top: a.dock_always_on_top, screen: self.config.dock.selection() })
    }

    fn apply_window_prefs(&mut self, window: &Win<P>) {
        if std::env::var_os("ASTER_CAPTURE").is_some() && std::env::var_os("ASTER_CAPTURE_PREFS").is_none() {
            return;
        }
        let wanted = (self.config.appearance.global_hotkey.clone(), self.dock_spec());
        let before = self.applied.replace(wanted.clone());
        if before.as_ref().map(|b| &b.0) != Some(&wanted.0) {
            let chord = Chord::parse(&wanted.0);
            if !wanted.0.trim().is_empty() && chord.is_none() {
                tracing::warn!("global hotkey {:?} is not a valid key", wanted.0);
            }
            if !window.set_global_hotkey(chord.map(|c| (c.key, c.mods))) && chord.is_some() {
                tracing::warn!("global hotkey {} is taken by another program or not supported here", wanted.0);
            }
        }
        if before.map(|b| b.1) != Some(wanted.1) {
            window.dock(wanted.1);
        }
    }

    fn save_session(&self) {
        if capturing() || self.config_startup != ConfigStartup::Ready {
            return;
        }
        let Some(path) = crate::session_path::<P>() else { return };
        let state = SessionState { windows: self.windows.values().map(|s| s.app.session_snapshot()).filter(|w| !w.tabs.is_empty()).collect(), recent: self.recent.clone(), recent_folders: self.recent_folders.clone() };
        let tmp = path.with_extension("tmp");
        if let Err(e) = std::fs::write(&tmp, state.to_text()).and_then(|_| std::fs::rename(&tmp, &path)) {
            tracing::warn!("{}: {e}", path.display());
        }
    }

    fn start_serial(&mut self, window: WindowId, pane: PaneId, terminal: aster_core::app::Shared) {
        let start = self.session_start(window, pane);
        let wake = start.flow.wake.clone();
        let screen = terminal.clone();
        let serial = self.serials.get_mut(&(window, pane)).expect("serial session");
        let spawned = serial.connect(move |message| {
            let clean: String = message.chars().filter(|c| !c.is_control()).collect();
            screen.lock().feed(format!("\r\n\x1b[2m[{clean}]\x1b[0m\r\n").as_bytes());
            wake();
        });
        self.sessions.insert((window, pane), Session::start(start, spawned, terminal));
    }

    fn session_start(&mut self, window: WindowId, pane: PaneId) -> crate::session::Start {
        let state = self.windows.get_mut(&window).expect("session window");
        crate::session::Start { flow: state.flow.clone(), raw_output: state.output.entry(pane).or_default().clone(),
            window, pane, downloads: P::downloads_dir().unwrap_or_else(std::env::temp_dir), exits: self.exit_tx.clone(), notes: self.note_tx.clone() }
    }

    fn window_secrets(&self, window: WindowId) -> WindowSecrets {
        let events = self.store_events.clone();
        WindowSecrets::new(self.secrets.clone(), move |issue, reply| {
            events.post(StoreEvent::VaultAccess(window, AccessPrompt { issue, reply }));
        })
    }

    fn answer_vault_access(&mut self, window: WindowId, password: Option<String>) {
        let Some(queue) = self.vault_access.get_mut(&window) else { return };
        if let Some(prompt) = queue.pop_front() {
            (prompt.reply)(password);
        }
        if let Some(prompt) = queue.front() {
            if let Some(state) = self.windows.get_mut(&window) {
                state.app.prompt_vault_access(prompt.issue);
            }
        } else {
            self.vault_access.remove(&window);
        }
    }

    fn vault_command(&self, window: WindowId, command: VaultCommand) {
        let Some(state) = self.windows.get(&window) else { return };
        let request = state.app.vault_request_id();
        let events = self.store_events.clone();
        self.secrets.command(command, move |result| events.post(StoreEvent::VaultReply(window, request, result)));
    }

    fn show_vault(&mut self, view: VaultView) {
        for s in self.windows.values_mut() {
            s.app.vault_state(view.clone());
        }
        self.vault_view = view;
    }

    fn drain_store(&mut self) {
        while let Ok(event) = self.store_rx.try_recv() {
            match event {
                StoreEvent::Launcher(window, result) => match result {
                    Ok(installed) => {
                        self.launcher_installed = Some(installed);
                        for state in self.windows.values_mut() {
                            state.app.set_launcher_installed(installed);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("applications menu: {e}");
                        if let Some(state) = window.and_then(|id| self.windows.get_mut(&id)) {
                            state.app.launcher_error(&e.to_string());
                        }
                    }
                },
                StoreEvent::ConfigEncryption(encrypted) => {
                    if encrypted != self.config.vault.encrypt_config {
                        let mut config = self.config.clone();
                        config.vault.encrypt_config = encrypted;
                        self.apply_config(config);
                    }
                    for state in self.windows.values_mut() {
                        state.app.set_config_encrypted(encrypted);
                    }
                }
                StoreEvent::ConfigOpened(window, request, result) => {
                    if let Some(state) = self.windows.get_mut(&window).filter(|state| state.app.config_unlock_waiting(request)) {
                        match result {
                            Ok(config) => self.startup_config = Some((window, config)),
                            Err(issue) => state.app.config_unlock_error(request, issue),
                        }
                    }
                }
                StoreEvent::BackgroundImage { window, revision, bitmap } => {
                    if let Some(state) = self.windows.get_mut(&window) {
                        state.app.background_image_loaded(revision, bitmap);
                    }
                }
                StoreEvent::CommandProbe { window, request, busy } => {
                    if let Some(state) = self.windows.get_mut(&window) {
                        state.app.command_probe_finished(request, Some(busy));
                    }
                }
                StoreEvent::VaultAccess(window, prompt) => {
                    if let Some(state) = self.windows.get_mut(&window) {
                        let queue = self.vault_access.entry(window).or_default();
                        if queue.is_empty() {
                            state.app.prompt_vault_access(prompt.issue);
                        }
                        queue.push_back(prompt);
                    }
                }
                StoreEvent::VaultReply(window, request, result) => {
                    if let Some(state) = self.windows.get_mut(&window) {
                        state.app.vault_reply(request, result);
                    }
                }
                StoreEvent::Vault(update) => {
                    if let Some(sealed) = update.sealed {
                        let mut config = self.config.clone();
                        config.vault.sealed = sealed;
                        self.apply_config(config);
                    }
                    self.show_vault(update.view);
                }
                StoreEvent::FontFamilies { monospace, all } => {
                    for state in self.windows.values_mut() {
                        state.app.set_font_families(monospace.clone(), all.clone());
                    }
                    self.font_families = Some((monospace, all));
                }
                StoreEvent::Config(revision, stamp, config) => {
                    if self.settings_revision == revision {
                        self.config_stamp = stamp;
                        self.show_config_status(config.as_ref().err().cloned());
                        if let Ok(config) = config {
                            if *config != self.config {
                                self.apply_config(*config);
                            }
                        }
                    }
                }
                StoreEvent::StartMenuShortcut(window, result) => match result {
                    Ok(exists) => {
                        self.start_menu_shortcut = exists;
                        for state in self.windows.values_mut() {
                            state.app.set_start_menu_shortcut(exists);
                        }
                    }
                    Err(error) => {
                        tracing::warn!("Start menu shortcut: {error}");
                        if let Some(state) = window.and_then(|id| self.windows.get_mut(&id)) {
                            state.app.start_menu_shortcut_error(&error.to_string());
                        }
                    }
                },
                StoreEvent::ConfigStatus(revision, error) => {
                    if self.settings_revision == revision {
                        self.show_config_status(error);
                    }
                }
                StoreEvent::DefaultConfig(window, result) => match result {
                    Ok(path) => P::open(&path.to_string_lossy()),
                    Err(e) => {
                        tracing::error!("defaults.toml: {e}");
                        if let Some(s) = self.windows.get_mut(&window) {
                            s.app.settings_file_error(&e.to_string());
                        }
                    }
                },
                StoreEvent::Known { window, pane, key, line, known } => {
                    if let Some(s) = self.windows.get_mut(&window) {
                        if s.app.password_prompt(pane).is_some_and(|p| p.key == key && p.line == line) {
                            s.app.secret_known(pane, known);
                        }
                    }
                }
                StoreEvent::Fill { window, pane, key, request, secret } => {
                    if self.windows.get_mut(&window).is_some_and(|s| s.app.finish_secret_fill(pane, &key, request, secret.is_some())) {
                        if let (Some(secret), Some(session)) = (secret, self.sessions.get(&(window, pane))) {
                            let mut bytes = secret.into_bytes();
                            bytes.push(b'\r');
                            session.write(bytes);
                        }
                    }
                }
            }
        }
    }

    fn drain_updates(&mut self) {
        while let Ok(reply) = self.updates.events.try_recv() {
            match reply {
                crate::update::Reply::Ui(event) => {
                    let target = self.update_window.filter(|id| self.windows.contains_key(id)).or_else(|| self.request_target());
                    if let Some(state) = target.and_then(|id| self.windows.get_mut(&id)) {
                        state.app.on_update(event);
                    }
                }
                crate::update::Reply::Restart => {
                    if !self.session_saved {
                        self.save_session();
                    }
                    self.session_saved = true;
                    self.restarting = true;
                    self.waker.wake();
                }
            }
        }
    }

    fn close_for_restart(&mut self, window: &Win<P>) {
        let first = self.windows.keys().min() == Some(&window.id());
        if first && !window.is_fullscreen() && self.dock_spec().is_none() {
            if let Some(path) = crate::state_path::<P>() {
                let _ = std::fs::write(path, crate::format_placement(&window.placement()));
            }
        }
        window.close();
    }

    fn drain_local_spawns(&mut self) {
        while let Some(mut ready) = self.local_spawns.next() {
            let (id, pane) = ready.key;
            let Some(mut pending) = self.local_spawns.take_pending((id, pane)) else { continue };
            let Some(state) = self.windows.get_mut(&id) else { continue };
            if state.app.terminal(pane).is_none() {
                continue;
            }
            match ready.take() {
                Ok(spawned) => {
                    state.app.set_pane_pid(pane, spawned.pty.pid());
                    if let Some((cols, rows)) = pending.size {
                        spawned.pty.resize(cols, rows);
                    }
                    let session = Session::start(self.session_start(id, pane), spawned, ready.terminal.clone());
                    for input in std::mem::take(&mut pending.input) {
                        session.write(input);
                    }
                    self.sessions.insert((id, pane), session);
                }
                Err(error) => {
                    tracing::error!("local shell: {error}");
                    state.app.spawn_failed(pane, &error.message);
                }
            }
        }
    }

    fn drain_exits(&mut self) {
        self.drain_local_spawns();
        while let Ok((wid, pane, note)) = self.note_rx.try_recv() {
            if let Some(s) = self.windows.get_mut(&wid) {
                let state = match note {
                    crate::zmodem::Note::AwaitingFiles => aster_core::app::PaneTransfer::AwaitingFiles,
                    crate::zmodem::Note::Incoming => aster_core::app::PaneTransfer::Incoming,
                    crate::zmodem::Note::Running => aster_core::app::PaneTransfer::Running,
                    crate::zmodem::Note::Ended => aster_core::app::PaneTransfer::None,
                };
                s.app.transfer_offered(pane, state);
            }
        }
        while let Ok((wid, pane, reply)) = self.file_rx.try_recv() {
            if let Some(s) = self.windows.get_mut(&wid) {
                s.app.files_reply(pane, reply);
            }
        }
        while let Ok((wid, pane, code)) = self.exits.try_recv() {
            use aster_core::profile::OnExit;
            let policy = self.windows.get(&wid).and_then(|s| s.app.pane_exit_policy(pane));
            let manual_retry = matches!(policy, Some(OnExit::Auto | OnExit::Inherit)) && self.serials.get_mut(&(wid, pane)).is_some_and(|s| s.disconnected(code));
            if let Some(session) = self.sessions.remove(&(wid, pane)).filter(|_| manual_retry || policy == Some(OnExit::Reconnect)) {
                session.abandon_transfer();
            }
            self.remotes.remove(&(wid, pane));
            if manual_retry {
                if let Some(state) = self.windows.get_mut(&wid) {
                    state.app.transfer_offered(pane, aster_core::app::PaneTransfer::None);
                    if let Some(terminal) = state.app.terminal(pane) {
                        let notice = aster_core::i18n::serial_disconnected();
                        terminal.lock().feed(format!("\r\n\x1b[0;2m{notice}\x1b[0m\r\n").as_bytes());
                    }
                }
                continue;
            }
            self.serials.remove(&(wid, pane));
            if let Some(s) = self.windows.get_mut(&wid) {
                s.app.exited(pane, code);
            }
        }
    }
}

impl<P: Platform> WindowHandler<P::Windows> for Handler<P> {
    fn tray_enabled(&self) -> bool {
        self.config.appearance.tray && !self.restarting
    }

    fn confirming_close(&self, window: &Win<P>) -> bool {
        self.windows.get(&window.id()).is_some_and(|s| s.app.close_confirm().is_some())
    }

    fn quitting(&mut self, quitting: bool) {
        if quitting && !self.session_saved {
            self.save_session();
        }
        self.session_saved = quitting;
    }

    fn background_woken(&mut self) -> bool {
        self.drain_exits();
        self.drain_store();
        self.drain_updates();
        queue_background_request(&mut self.startup, self.requests.as_ref())
    }

    fn created(&mut self, window: &Win<P>) {
        let mut text = match P::text() {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("text backend: {e}");
                window.close();
                return;
            }
        };
        let waker = self.waker.clone();
        let home = P::home_dir().map(|p| p.to_string_lossy().into_owned());
        let flow = crate::session::OutputFlow::new(move || waker.wake());
        text.set_font_waker(flow.wake.clone());
        let mut app = App::new(self.config.clone(), self.profiles.clone(), window.is_dark_mode(), home, flow.wake.clone());
        if let Some((monospace, all)) = &self.font_families {
            app.set_font_families(monospace.clone(), all.clone());
        }
        let (w, h) = window.size();
        app.set_viewport(w, h, window.scale());
        app.set_ssh(self.ssh.clone());
        app.set_config_path(self.config_path.as_ref().map(|p| p.to_string_lossy().into_owned()));
        if let Some(installed) = self.launcher_installed {
            app.set_launcher_installed(installed);
        }
        app.set_start_menu_shortcut(self.start_menu_shortcut);
        app.set_system_language(&P::ui_language());
        app.vault_state(self.vault_view.clone());
        app.set_config_encrypted(self.config.vault.encrypt_config || self.config_startup != ConfigStartup::Ready);
        app.set_cwd_probe(Arc::new(P::process_cwd));
        app.set_children_probe(Arc::new(P::child_processes));
        app.set_can_elevate(P::can_elevate());
        app.set_recent(self.recent.clone());
        let first = self.windows.is_empty();
        let mut hidden = false;
        if self.config_startup == ConfigStartup::Prompt {
            app.prompt_config_unlock();
        } else {
            let (saved, request) = self.take_window_startup();
            hidden = request.as_ref().is_some_and(|r| r.command == crate::cli::Command::Hidden);
            populate_window(&mut app, saved, first, request);
        }
        let dark = app.is_dark();
        window.set_dark(dark);
        self.windows.insert(window.id(), State { app, flow, output: HashMap::new(), gfx: Gfx::new(text, ATLAS), renderer: None });
        if first && self.config_startup != ConfigStartup::Prompt {
            self.open_pending_windows(window, dark, !hidden);
            self.applied = None;
            self.waker.wake();
            self.apply_window_prefs(window);
            self.refresh_jump_list();
            if self.config.system.url_scheme || !capturing() {
                self.shell_integration.url_scheme(self.config.system.url_scheme);
            }
            if self.config.system.explorer_menu || cfg!(target_os = "linux") {
                self.apply_explorer_menu(self.config.system.explorer_menu);
            }
        }
        window.request_redraw();
    }

    fn event(&mut self, window: &Win<P>, event: Event) {
        if matches!(event, Event::NotificationClicked(_)) {
            if !window.is_visible() {
                window.set_visible(true);
            }
            window.activate();
        }
        if matches!(event, Event::Focus(true)) {
            self.last_focused = Some(window.id());
            self.reload_config(false);
            if self.windows.keys().min() == Some(&window.id()) {
                self.apply_window_prefs(window);
            }
        }
        if matches!(event, Event::GlobalHotkey) {
            if window.is_visible() && window.is_foreground() {
                window.set_visible(false);
            } else {
                window.set_visible(true);
                if let Some(spec) = self.dock_spec() {
                    window.dock(Some(spec));
                }
                window.activate();
            }
            return;
        }
        if matches!(event, Event::Focus(false)) && self.config.appearance.dock_hide_on_blur && self.dock_spec().is_some() && self.windows.keys().min() == Some(&window.id()) {
            window.set_visible(false);
        }
        if matches!(event, Event::SessionEnding) {
            self.save_session();
            self.session_saved = true;
        }
        if matches!(event, Event::CloseRequested) && !window.is_fullscreen() && self.dock_spec().is_none() {
            if let Some(path) = crate::state_path::<P>() {
                let _ = std::fs::write(path, crate::format_placement(&window.placement()));
            }
        }
        if let Some(s) = self.windows.get_mut(&window.id()) {
            if matches!(event, Event::Resized { .. }) {
                s.app.set_fullscreen(window.is_fullscreen());
            }
            s.app.on_event(event);
        }
        self.apply(window);
    }

    fn hit_test(&mut self, window: &Win<P>, x: f32, y: f32) -> HitArea {
        self.windows.get(&window.id()).map_or(HitArea::Client, |s| s.app.hit_test(x, y))
    }

    fn redraw(&mut self, window: &Win<P>) {
        let id = window.id();
        if !self.windows.contains_key(&id) {
            return;
        }
        let needs_renderer = self.windows.get(&id).is_some_and(|s| s.renderer.is_none());
        if needs_renderer {
            let Some(device) = self.device() else { return };
            match P::Renderer::new(device, window, ATLAS) {
                Ok(r) => {
                    let s = self.windows.get_mut(&id).expect("window state");
                    s.renderer = Some(r);
                    s.gfx.reset_atlas();
                }
                Err(e) => {
                    tracing::error!("renderer: {e}");
                    return;
                }
            }
        }
        let s = self.windows.get_mut(&id).expect("window state");
        let (w, h) = window.size();
        s.app.set_viewport(w, h, window.scale());
        let maximized = window.is_maximized();
        s.app.frame(&mut s.gfx, maximized);
        let renderer = s.renderer.as_mut().expect("renderer");
        renderer.resize(w, h);
        for (slot, offset) in &s.gfx.uploads.rects {
            let len = (slot.w * slot.h * 4) as usize;
            renderer.upload(*slot, &s.gfx.uploads.data[*offset..*offset + len]);
        }
        s.gfx.uploads.clear();
        match renderer.render(&s.gfx.dl) {
            Ok(()) => s.flow.frame_drawn(),
            Err(RenderError::DeviceLost) => {
                tracing::warn!("graphics device lost");
                s.renderer = None;
                self.device = None;
                window.request_redraw();
            }
            Err(RenderError::Other(e)) => tracing::error!("render: {e}"),
        }
        self.apply(window);
    }

    fn woken(&mut self, window: &Win<P>) {
        self.drain_updates();
        if self.restarting {
            return self.close_for_restart(window);
        }
        self.drain_store();
        if self.startup_config.as_ref().is_some_and(|(id, _)| *id == window.id()) {
            if let Some((_, config)) = self.startup_config.take() {
                self.finish_config_start(window, Some(*config));
            }
        }
        if self.window_prefs_dirty && self.windows.keys().min() == Some(&window.id()) {
            self.window_prefs_dirty = false;
            self.apply_window_prefs(window);
        }
        self.drain_requests(window);
        self.drain_exits();
        if let Some(s) = self.windows.get_mut(&window.id()) {
            s.app.woken();
        }
        if cfg!(windows) && self.request_target() == Some(window.id())
            && self.windows.get(&window.id()).and_then(|s| s.app.reported_local_cwd())
                .is_some_and(|folder| aster_core::jump_list::remember_folder(&mut self.recent_folders, &folder))
        {
            self.refresh_jump_list();
        }
        self.apply(window);
    }

    fn destroyed(&mut self, window: &Win<P>) {
        let id = window.id();
        self.local_spawns.cancel_window(id);
        if let Some(mut s) = self.windows.remove(&id) {
            s.app.close_all();
            for e in s.app.take_effects() {
                if let Effect::Kill { pane } = e {
                    if let Some(session) = self.sessions.remove(&(id, pane)) {
                        session.pty.kill();
                    }
                }
            }
        }
        self.vault_access.remove(&id);
        self.remotes.retain(|(w, _), _| *w != id);
        self.serials.retain(|(w, _), _| *w != id);
        self.sessions.retain(|(w, _), s| {
            if *w == id {
                s.pty.kill();
            }
            *w != id
        });
    }
}

#[cfg(test)]
#[path = "../test/handler.rs"]
mod tests;
