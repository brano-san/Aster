use crate::session::{Exit, Session};
use aster_core::app::{App, Effect};
use aster_core::config::Config;
use aster_core::input::{Event, HitArea};
use aster_core::panes::PaneId;
use aster_core::profile::Profile;
use aster_core::session::{SessionState, WindowState};
use aster_gfx::text::Gfx;
use aster_core::config::{Dock, SshClient};
use aster_core::keymap::Chord;
use aster_platform::{DockSpec, Edge, Platform, RenderError, Renderer, SpawnRequest, Waker, Window, WindowHandler, WindowId, WindowOptions, WindowSystem};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;
use std::sync::Arc;

const ATLAS: u32 = 2048;

type Elevated = (WindowId, PaneId, Result<aster_platform::Spawned, aster_platform::PlatformError>, aster_core::app::Shared);

type Win<P> = <<P as Platform>::Windows as WindowSystem>::Window;
type Device<P> = <<P as Platform>::Renderer as Renderer>::Device;

struct State<P: Platform> {
    app: App,
    gfx: Gfx<P::Text>,
    renderer: Option<P::Renderer>,
}

pub struct Handler<P: Platform> {
    config: Config,
    profiles: Vec<Profile>,
    waker: <P::Windows as WindowSystem>::Waker,
    device: Option<Device<P>>,
    windows: HashMap<WindowId, State<P>>,
    sessions: HashMap<(WindowId, PaneId), Session>,
    exits: Receiver<Exit>,
    remotes: HashMap<(WindowId, PaneId), tokio::sync::mpsc::UnboundedSender<aster_ssh::console::Input>>,
    file_tx: Sender<(WindowId, PaneId, aster_core::files::FileReply)>,
    file_rx: Receiver<(WindowId, PaneId, aster_core::files::FileReply)>,
    note_tx: Sender<crate::session::TransferNote>,
    note_rx: Receiver<crate::session::TransferNote>,
    elevated_tx: Sender<Elevated>,
    elevated_rx: Receiver<Elevated>,
    exit_tx: Sender<Exit>,
    config_stamp: Option<SystemTime>,
    ssh: Option<String>,
    pending: Vec<WindowState>,
    session_saved: bool,
    applied: Option<(String, Option<DockSpec>)>,
    requests: Option<std::sync::mpsc::Receiver<String>>,
    startup: Option<crate::cli::Request>,
    last_focused: Option<WindowId>,
}

fn ssh_hosts<P: Platform>(ssh: &str) -> Vec<Profile> {
    let Some(home) = P::home_dir() else { return Vec::new() };
    let dir = home.join(".ssh");
    let Ok(text) = std::fs::read_to_string(dir.join("config")) else { return Vec::new() };
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok();
    aster_ssh::config::parse(&text, &dir, &home, &read).iter().map(|h| aster_core::ssh::host_profile(&h.alias, ssh)).collect()
}

fn stamp(path: &Option<PathBuf>) -> Option<SystemTime> {
    path.as_ref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok())
}

fn capturing() -> bool {
    std::env::var_os("ASTER_CAPTURE").is_some() && std::env::var_os("ASTER_CAPTURE_SESSION").is_none()
}

fn load_session<P: Platform>() -> Vec<WindowState> {
    if capturing() {
        return Vec::new();
    }
    crate::session_path::<P>().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| SessionState::parse(&t)).map(|s| s.windows).unwrap_or_default()
}

impl<P: Platform> Handler<P> {
    pub fn new(config: Config, waker: <P::Windows as WindowSystem>::Waker) -> Self {
        let mut profiles = P::shells();
        let ssh = P::ssh_command().or_else(|| (config.ssh.client == SshClient::Native).then(|| "ssh".to_owned()));
        if let Some(cmd) = &ssh {
            profiles.extend(ssh_hosts::<P>(cmd));
        }
        profiles.extend(crate::serial::profiles());
        let (exit_tx, exits) = unbounded();
        let (file_tx, file_rx) = unbounded();
        let (note_tx, note_rx) = unbounded();
        let (elevated_tx, elevated_rx) = unbounded();
        let config_stamp = stamp(&crate::config_path::<P>());
        let pending = if config.terminal.recover_tabs { load_session::<P>() } else { Vec::new() };
        Self { config, profiles, waker, device: None, windows: HashMap::new(), sessions: HashMap::new(), exits, remotes: HashMap::new(), file_tx, file_rx, note_tx, note_rx, elevated_tx, elevated_rx, exit_tx, config_stamp, ssh, pending, session_saved: false, applied: None, requests: None, startup: None, last_focused: None }
    }

    pub fn set_instance(&mut self, requests: Option<std::sync::mpsc::Receiver<String>>, startup: Option<crate::cli::Request>) {
        self.requests = requests;
        self.startup = startup;
    }

    fn request_target(&self) -> Option<WindowId> {
        self.last_focused.filter(|id| self.windows.contains_key(id)).or_else(|| self.windows.keys().min().copied())
    }

    fn serve_request(app: &mut App, request: crate::cli::Request) -> bool {
        use crate::cli::Command;
        match request.command {
            Command::Window => return false,
            Command::Open { dir } => {
                if let Some(profile) = app.default_profile() {
                    app.open_tab_at(profile, Some(dir));
                }
            }
            Command::Run { command } => app.run_command(&command, request.cwd),
            Command::Profile { name } => match app.find_profile(&name) {
                Some(profile) => app.open_tab(profile),
                None => tracing::warn!("no profile named {name}"),
            },
            Command::Paste { text } => app.paste(text),
        }
        true
    }

    fn drain_requests(&mut self, window: &Win<P>) {
        if self.request_target() != Some(window.id()) {
            return;
        }
        let incoming: Vec<String> = self.requests.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for text in incoming {
            let Some(request) = crate::cli::Request::decode(&text) else { continue };
            let handled = self.windows.get_mut(&window.id()).is_some_and(|s| Self::serve_request(&mut s.app, request));
            if !handled {
                let dark = self.windows.get(&window.id()).is_none_or(|s| s.app.is_dark());
                window.open_window(WindowOptions { title: "Aster".into(), placement: None, dark });
            }
            window.set_visible(true);
            window.activate();
            window.request_redraw();
        }
    }

    fn save_config(&mut self, text: String) {
        let path = crate::config_path::<P>();
        if let Some(p) = &path {
            if let Err(e) = std::fs::write(p, &text) {
                tracing::error!("{}: {e}", p.display());
            }
        }
        self.config_stamp = stamp(&path);
        if let Ok(config) = Config::parse(&text) {
            self.apply_config(config);
        }
    }

    fn apply_config(&mut self, config: Config) {
        for s in self.windows.values_mut() {
            if s.app.config() != &config {
                s.app.set_config(config.clone());
            }
        }
        let explorer = config.system.explorer_menu;
        if explorer != self.config.system.explorer_menu {
            self.apply_explorer_menu(explorer);
        }
        self.config = config;
    }

    fn apply_explorer_menu(&self, enabled: bool) {
        if !P::set_explorer_menu(aster_core::settings::explorer_label(), enabled) && enabled {
            tracing::warn!("could not add the Explorer menu");
        }
    }

    fn reload_config(&mut self) {
        let path = crate::config_path::<P>();
        let now = stamp(&path);
        if now == self.config_stamp {
            return;
        }
        self.config_stamp = now;
        let Some(text) = path.and_then(|p| std::fs::read_to_string(p).ok()) else { return };
        match Config::parse(&text) {
            Ok(config) => self.apply_config(config),
            Err(e) => tracing::error!("config: {e}"),
        }
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
        if self.windows.get(&id).is_some_and(|s| s.app.needs_redraw()) {
            window.request_redraw();
        }
    }

    fn effect(&mut self, window: &Win<P>, effect: Effect) {
        let id = window.id();
        match effect {
            Effect::Spawn { pane, launch, terminal } if launch.serial.is_some() => {
                let spawned = crate::serial::connect(launch.serial.as_deref().unwrap_or_default());
                let waker = self.waker.clone();
                let session = Session::start(self.session_start(id, pane), crate::stream::wrap(spawned, launch.stream.as_ref()), terminal, move || waker.wake());
                self.sessions.insert((id, pane), session);
            }
            Effect::Spawn { pane, launch, terminal } if launch.socket.is_some() => {
                let spawned = crate::net::connect(launch.socket.as_deref().unwrap_or_default(), launch.cols, launch.rows, launch.env.get("TERM").map(String::as_str), launch.stream.as_ref().is_none_or(|s| s.input == aster_core::profile::InputMode::Normal));
                let waker = self.waker.clone();
                let session = Session::start(self.session_start(id, pane), crate::stream::wrap(spawned, launch.stream.as_ref()), terminal, move || waker.wake());
                self.sessions.insert((id, pane), session);
            }
            Effect::Spawn { pane, launch, terminal } if launch.ssh.is_some() && self.config.ssh.client == SshClient::Native => {
                let destination = launch.ssh.unwrap_or_default();
                let remote = crate::remote::connect(&destination, launch.cols, launch.rows, launch.env.get("TERM").map(String::as_str), P::home_dir(), crate::remote::Options { share: self.config.ssh.share_connections, forward_agent: self.config.ssh.forward_agent, remember: self.config.ssh.remember_passwords, secrets: Arc::new(crate::remote::System::<P>::default()) });
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
                let waker = self.waker.clone();
                let session = Session::start(self.session_start(id, pane), crate::stream::wrap(remote.spawned, launch.stream.as_ref()), terminal, move || waker.wake());
                self.sessions.insert((id, pane), session);
            }
            Effect::Spawn { pane, launch, terminal } => {
                let req = SpawnRequest {
                    command: launch.command,
                    args: launch.args,
                    cwd: launch.cwd.map(PathBuf::from),
                    env: launch.env.into_iter().collect(),
                    cols: launch.cols,
                    rows: launch.rows,
                };
                if launch.elevated {
                    let (tx, waker) = (self.elevated_tx.clone(), self.waker.clone());
                    let started = std::thread::Builder::new().name("aster-elevate".into()).spawn(move || {
                        let result = P::spawn_elevated(&req);
                        let _ = tx.send((id, pane, result, terminal));
                        waker.wake();
                    });
                    if let Err(e) = started {
                        tracing::error!("elevated spawn thread: {e}");
                    }
                    return;
                }
                match P::spawn(&req) {
                    Ok(spawned) => {
                        let pid = spawned.pty.pid();
                        if let Some(s) = self.windows.get_mut(&id) {
                            s.app.set_pane_pid(pane, pid);
                        }
                        let waker = self.waker.clone();
                        let session = Session::start(self.session_start(id, pane), crate::stream::wrap(spawned, launch.stream.as_ref()), terminal, move || waker.wake());
                        self.sessions.insert((id, pane), session);
                    }
                    Err(e) => {
                        tracing::error!("spawn {}: {e}", req.command);
                        if let Some(s) = self.windows.get_mut(&id) {
                            s.app.spawn_failed(pane, &e.message);
                        }
                    }
                }
            }
            Effect::Write { pane, bytes } => {
                if let Some(s) = self.sessions.get(&(id, pane)) {
                    s.write(bytes);
                }
            }
            Effect::Resize { pane, cols, rows } => {
                if let Some(s) = self.sessions.get(&(id, pane)) {
                    s.pty.resize(cols, rows);
                }
            }
            Effect::Kill { pane } => {
                if let Some(s) = self.sessions.remove(&(id, pane)) {
                    s.pty.kill();
                }
            }
            Effect::SetClipboard(text) => window.set_clipboard_text(&text),
            Effect::RequestPaste => {
                if let (Some(text), Some(s)) = (window.clipboard_text(), self.windows.get_mut(&id)) {
                    s.app.paste(text);
                }
            }
            Effect::SaveConfig(text) => {
                self.save_config(text);
                if self.windows.keys().min() == Some(&id) {
                    self.apply_window_prefs(window);
                }
            }
            Effect::Open(target) => P::open(&target),
            Effect::Title(t) => window.set_title(&t),
            Effect::Cursor(c) => window.set_cursor(c),
            Effect::Minimize => window.minimize(),
            Effect::ToggleMaximize => window.toggle_maximize(),
            Effect::ToggleFullscreen => window.toggle_fullscreen(),
            Effect::Progress(progress) => window.set_progress(progress),
            Effect::Close => {
                if self.windows.len() == 1 && !std::mem::take(&mut self.session_saved) {
                    self.save_session();
                }
                window.close();
            }
            Effect::NewWindow => window.open_window(WindowOptions { title: "Aster".into(), placement: None, dark: self.windows.get(&id).is_none_or(|s| s.app.is_dark()) }),
            Effect::Timer(ms) => window.set_timer(ms),
            Effect::Attention => window.request_attention(),
            Effect::CheckSecret { pane, key } => {
                let known = P::secret_get(&key).is_some();
                if let Some(s) = self.windows.get_mut(&id) {
                    s.app.secret_known(pane, known);
                }
            }
            Effect::FillSecret { pane, key } => {
                if let (Some(secret), Some(s)) = (P::secret_get(&key), self.sessions.get(&(id, pane))) {
                    let mut bytes = secret.into_bytes();
                    bytes.push(b'\r');
                    s.write(bytes);
                }
            }
            Effect::SaveSecret { key, value } => {
                if !P::secret_set(&key, &value) {
                    tracing::warn!("could not save the password for {key}");
                }
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
            Effect::Beep => {
                if !P::beep() {
                    if let Some(s) = self.windows.get_mut(&id) {
                        s.app.flash_bell();
                        window.request_redraw();
                    }
                }
            }
            Effect::ImeArea(r) => window.set_ime_area(r),
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
        Some(DockSpec { edge, fill: a.dock_fill, space: a.dock_space, on_top: a.dock_always_on_top })
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
        if capturing() {
            return;
        }
        let Some(path) = crate::session_path::<P>() else { return };
        let state = SessionState { windows: self.windows.values().map(|s| s.app.session_snapshot()).filter(|w| !w.tabs.is_empty()).collect() };
        let tmp = path.with_extension("tmp");
        if let Err(e) = std::fs::write(&tmp, state.to_text()).and_then(|_| std::fs::rename(&tmp, &path)) {
            tracing::warn!("{}: {e}", path.display());
        }
    }

    fn session_start(&self, window: WindowId, pane: PaneId) -> crate::session::Start {
        crate::session::Start { window, pane, downloads: P::downloads_dir().unwrap_or_else(std::env::temp_dir), exits: self.exit_tx.clone(), notes: self.note_tx.clone() }
    }

    fn drain_elevated(&mut self) {
        while let Ok((id, pane, result, terminal)) = self.elevated_rx.try_recv() {
            match result {
                Ok(spawned) => {
                    let waker = self.waker.clone();
                    let session = Session::start(self.session_start(id, pane), spawned, terminal, move || waker.wake());
                    self.sessions.insert((id, pane), session);
                }
                Err(e) => {
                    if let Some(s) = self.windows.get_mut(&id) {
                        s.app.spawn_failed(pane, &e.message);
                    }
                }
            }
        }
    }

    fn drain_exits(&mut self) {
        self.drain_elevated();
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
            self.sessions.remove(&(wid, pane));
            self.remotes.remove(&(wid, pane));
            if let Some(s) = self.windows.get_mut(&wid) {
                s.app.exited(pane, code);
            }
        }
    }
}

impl<P: Platform> WindowHandler<P::Windows> for Handler<P> {
    fn created(&mut self, window: &Win<P>) {
        let text = match P::text() {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("text backend: {e}");
                window.close();
                return;
            }
        };
        let waker = self.waker.clone();
        let home = P::home_dir().map(|p| p.to_string_lossy().into_owned());
        let mut app = App::new(self.config.clone(), self.profiles.clone(), window.is_dark_mode(), home, Arc::new(move || waker.wake()));
        let (w, h) = window.size();
        app.set_viewport(w, h, window.scale());
        app.set_ssh(self.ssh.clone());
        app.set_system_language(&P::ui_language());
        app.set_cwd_probe(Arc::new(P::process_cwd));
        app.set_children_probe(Arc::new(P::child_processes));
        app.set_can_elevate(P::can_elevate());
        let restored = (!self.pending.is_empty()).then(|| self.pending.remove(0)).is_some_and(|state| app.restore(&state));
        let startup = self.startup.take().filter(|r| r.command != crate::cli::Command::Window);
        let opens_tab = startup.as_ref().is_some_and(|r| !matches!(r.command, crate::cli::Command::Paste { .. }));
        if !restored && !opens_tab {
            app.open_default_tab();
        }
        if let Some(request) = startup {
            Self::serve_request(&mut app, request);
        }
        for _ in 0..std::mem::take(&mut self.pending).len() {
            window.open_window(WindowOptions { title: "Aster".into(), placement: None, dark: app.is_dark() });
        }
        window.set_dark(app.is_dark());
        let first = self.windows.is_empty();
        self.windows.insert(window.id(), State { app, gfx: Gfx::new(text, ATLAS), renderer: None });
        if first {
            self.apply_window_prefs(window);
            if self.config.system.explorer_menu {
                self.apply_explorer_menu(true);
            }
        }
        window.request_redraw();
    }

    fn event(&mut self, window: &Win<P>, event: Event) {
        if matches!(event, Event::Focus(true)) {
            self.last_focused = Some(window.id());
            self.reload_config();
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
        for _ in 0..3 {
            s.app.frame(&mut s.gfx, maximized);
            if !s.gfx.atlas_reset {
                break;
            }
        }
        let renderer = s.renderer.as_mut().expect("renderer");
        renderer.resize(w, h);
        for (slot, offset) in &s.gfx.uploads.rects {
            let len = (slot.w * slot.h * 4) as usize;
            renderer.upload(*slot, &s.gfx.uploads.data[*offset..*offset + len]);
        }
        s.gfx.uploads.clear();
        match renderer.render(&s.gfx.dl) {
            Ok(()) => {}
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
        self.drain_requests(window);
        self.drain_exits();
        if let Some(s) = self.windows.get_mut(&window.id()) {
            s.app.woken();
        }
        self.apply(window);
    }

    fn destroyed(&mut self, window: &Win<P>) {
        let id = window.id();
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
        self.remotes.retain(|(w, _), _| *w != id);
        self.sessions.retain(|(w, _), s| {
            if *w == id {
                s.pty.kill();
            }
            *w != id
        });
    }
}
