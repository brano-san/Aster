use crate::session::{Exit, Session};
use aster_core::app::{App, Effect};
use aster_core::config::Config;
use aster_core::input::{Event, HitArea};
use aster_core::panes::PaneId;
use aster_core::profile::Profile;
use aster_gfx::text::Gfx;
use aster_platform::{Platform, RenderError, Renderer, SpawnRequest, Waker, Window, WindowHandler, WindowId, WindowOptions, WindowSystem};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::path::PathBuf;
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
}

impl<P: Platform> Handler<P> {
    pub fn new(config: Config, waker: <P::Windows as WindowSystem>::Waker) -> Self {
        let profiles = P::shells();
        let (exit_tx, exits) = unbounded();
        Self { config, profiles, waker, device: None, windows: HashMap::new(), sessions: HashMap::new(), exits, exit_tx }
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
            Effect::Title(t) => window.set_title(&t),
            Effect::Cursor(c) => window.set_cursor(c),
            Effect::Minimize => window.minimize(),
            Effect::ToggleMaximize => window.toggle_maximize(),
            Effect::ToggleFullscreen => window.toggle_fullscreen(),
            Effect::Close => window.close(),
            Effect::NewWindow => window.open_window(WindowOptions { title: "Aster".into(), placement: None, dark: self.windows.get(&id).is_none_or(|s| s.app.is_dark()) }),
            Effect::Timer(ms) => window.set_timer(ms),
            Effect::Attention => window.request_attention(),
            Effect::ImeArea(r) => window.set_ime_area(r),
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
        app.open_default_tab();
        window.set_dark(app.is_dark());
        self.windows.insert(window.id(), State { app, gfx: Gfx::new(text, ATLAS), renderer: None });
        window.request_redraw();
    }

    fn event(&mut self, window: &Win<P>, event: Event) {
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
