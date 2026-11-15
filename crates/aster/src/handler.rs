use crate::session::{Exit, Session};
use aster_core::app::{App, Effect};
use aster_core::config::Config;
use aster_core::input::{Event, HitArea};
use aster_core::panes::PaneId;
use aster_core::profile::Profile;
use aster_core::session::{SessionState, WindowState};
use aster_gfx::text::Gfx;
use aster_platform::{Platform, RenderError, Renderer, SpawnRequest, Waker, Window, WindowHandler, WindowId, WindowOptions, WindowSystem};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;
use std::sync::Arc;

const ATLAS: u32 = 2048;

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
    exit_tx: Sender<Exit>,
    config_stamp: Option<SystemTime>,
    ssh: Option<String>,
    pending: Vec<WindowState>,
    session_saved: bool,
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
        let ssh = P::ssh_command();
        if let Some(cmd) = &ssh {
            profiles.extend(ssh_hosts::<P>(cmd));
        }
        let (exit_tx, exits) = unbounded();
        let config_stamp = stamp(&crate::config_path::<P>());
        let pending = if config.terminal.recover_tabs { load_session::<P>() } else { Vec::new() };
        Self { config, profiles, waker, device: None, windows: HashMap::new(), sessions: HashMap::new(), exits, exit_tx, config_stamp, ssh, pending, session_saved: false }
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
        self.config = config;
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
            Effect::Spawn { pane, launch, terminal } => {
                let req = SpawnRequest {
                    command: launch.command,
                    args: launch.args,
                    cwd: launch.cwd.map(PathBuf::from),
                    env: launch.env.into_iter().collect(),
                    cols: launch.cols,
                    rows: launch.rows,
                };
                match P::spawn(&req) {
                    Ok(spawned) => {
                        let waker = self.waker.clone();
                        let session = Session::start(id, pane, spawned, terminal, move || waker.wake(), self.exit_tx.clone());
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
            Effect::SaveConfig(text) => self.save_config(text),
            Effect::Open(target) => P::open(&target),
            Effect::Title(t) => window.set_title(&t),
            Effect::Cursor(c) => window.set_cursor(c),
            Effect::Minimize => window.minimize(),
            Effect::ToggleMaximize => window.toggle_maximize(),
            Effect::ToggleFullscreen => window.toggle_fullscreen(),
            Effect::Close => {
                if self.windows.len() == 1 && !std::mem::take(&mut self.session_saved) {
                    self.save_session();
                }
                window.close();
            }
            Effect::NewWindow => window.open_window(WindowOptions { title: "Aster".into(), placement: None, dark: self.windows.get(&id).is_none_or(|s| s.app.is_dark()) }),
            Effect::Timer(ms) => window.set_timer(ms),
            Effect::Attention => window.request_attention(),
            Effect::ImeArea(r) => window.set_ime_area(r),
        }
    }

    fn save_session(&self) {
        if capturing() {
            return;
        }
        let Some(path) = crate::session_path::<P>() else { return };
        let state = SessionState { windows: self.windows.values().map(|s| s.app.snapshot()).filter(|w| !w.tabs.is_empty()).collect() };
        let tmp = path.with_extension("tmp");
        if let Err(e) = std::fs::write(&tmp, state.to_text()).and_then(|_| std::fs::rename(&tmp, &path)) {
            tracing::warn!("{}: {e}", path.display());
        }
    }

    fn drain_exits(&mut self) {
        while let Ok((wid, pane, code)) = self.exits.try_recv() {
            self.sessions.remove(&(wid, pane));
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
        let restored = (!self.pending.is_empty()).then(|| self.pending.remove(0)).is_some_and(|state| app.restore(&state));
        if !restored {
            app.open_default_tab();
        }
        for _ in 0..std::mem::take(&mut self.pending).len() {
            window.open_window(WindowOptions { title: "Aster".into(), placement: None, dark: app.is_dark() });
        }
        window.set_dark(app.is_dark());
        self.windows.insert(window.id(), State { app, gfx: Gfx::new(text, ATLAS), renderer: None });
        window.request_redraw();
    }

    fn event(&mut self, window: &Win<P>, event: Event) {
        if matches!(event, Event::Focus(true)) {
            self.reload_config();
        }
        if matches!(event, Event::SessionEnding) || (matches!(event, Event::CloseRequested) && self.windows.len() == 1) {
            self.save_session();
            self.session_saved = true;
        }
        if matches!(event, Event::CloseRequested) && !window.is_fullscreen() {
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
        self.sessions.retain(|(w, _), s| {
            if *w == id {
                s.pty.kill();
            }
            *w != id
        });
    }
}
