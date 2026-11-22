use aster_core::input::{CursorIcon, Event, HitArea, Key, Mods, MouseButton, PasteTarget, Preedit};
use aster_core::notifications::Notification;
use aster_gfx::render::Rect;
use aster_platform::monitors::{choose_monitor, correct_offscreen, monitor_at, Bounds, Monitor};
use aster_platform::{dock_rect, DockSlide, DockSpec, Edge, Placement, PlatformError, Waker, Window, WindowHandler, WindowId, WindowOptions, WindowSystem};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::cursor::CursorIcon as WinitCursor;
use winit::data_transfer::{DataTransferSendBuilder, TypeHint};
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton as WinitButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::run_on_demand::EventLoopExtRunOnDemand;
use winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle};
use winit::keyboard::{Key as WinitKey, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::monitor::Fullscreen;
use winit::platform::{wayland::WindowAttributesWayland, x11::WindowAttributesX11};
use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
use winit::window::{ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData, ResizeDirection, Theme, UserAttentionType, WindowAttributes, WindowLevel};

const DEFAULT_SIZE: (f64, f64) = (1000.0, 640.0);
const MIN_SIZE: (f64, f64) = (360.0, 200.0);
const RESIZE_BORDER: f32 = 5.0;
const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const WHEEL_PIXELS_PER_NOTCH: f32 = 40.0;

static DISPLAY: OnceLock<OwnedDisplayHandle> = OnceLock::new();

pub(crate) fn display() -> Option<OwnedDisplayHandle> {
    DISPLAY.get().cloned()
}

enum Command {
    Open(WindowOptions),
    Close(WindowId),
    Paste(WindowId, Option<PasteTarget>),
    Copy(String),
}

type Commands = Rc<RefCell<Vec<Command>>>;

pub struct LinuxWindow {
    id: WindowId,
    raw: Arc<dyn winit::window::Window>,
    commands: Commands,
    notification_tx: mpsc::Sender<(WindowId, u64)>,
    notification_waker: LinuxWaker,
    size: Cell<(u32, u32)>,
    scale: Cell<f32>,
    cursor: Cell<CursorIcon>,
    edge: Cell<Option<ResizeDirection>>,
    timer: Cell<Option<Instant>>,
    dock_spec: Cell<Option<DockSpec>>,
    dock_slide: Cell<Option<DockSlide>>,
    position: Cell<Option<(i32, i32)>>,
    translucent: bool,
}

impl LinuxWindow {
    pub(crate) fn winit(&self) -> &Arc<dyn winit::window::Window> {
        &self.raw
    }

    pub(crate) fn is_wayland(&self) -> bool {
        self.raw.display_handle().is_ok_and(|h| matches!(h.as_raw(), RawDisplayHandle::Wayland(_)))
    }

    fn free_size(&self) -> bool {
        !self.raw.is_maximized() && self.raw.fullscreen().is_none()
    }

    fn place(&self, (x, y, w, h): Bounds) {
        let _ = self.raw.request_surface_size(PhysicalSize::new(w.max(1) as u32, h.max(1) as u32).into());
        self.raw.set_outer_position(PhysicalPosition::new(x, y).into());
    }

    fn apply_dock(&self, spec: DockSpec, monitors: &[Monitor]) {
        self.dock_slide.set(None);
        let current = self.raw.current_monitor().and_then(|monitor| monitor.position())
            .and_then(|point| monitor_at(monitors, (point.x, point.y)));
        let Some(monitor) = choose_monitor(monitors, current, spec.screen) else { return };
        if self.raw.is_maximized() { self.raw.set_maximized(false); }
        self.raw.set_window_level(if spec.on_top { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal });
        let rect = dock_rect(monitor.work, spec);
        self.place(rect);
        if spec.edge == Edge::Top {
            self.dock_slide.set(Some(DockSlide::top(rect, monitor.bounds.1)));
        }
    }

    fn displays_changed(&self, monitors: &[Monitor]) {
        if self.is_wayland() { return; }
        if let Some(spec) = self.dock_spec.get() {
            self.apply_dock(spec, monitors);
        } else if self.free_size() && self.raw.is_minimized() != Some(true) {
            if let Ok(at) = self.raw.outer_position() {
                let (w, h) = self.size.get();
                if let Some(bounds) = correct_offscreen((at.x, at.y, w as i32, h as i32), monitors) {
                    self.place(bounds);
                }
            }
        }
    }

    fn track_edge(&self, x: f32, y: f32) {
        let (w, h) = self.size.get();
        let edge = if self.free_size() { resize_edge(x, y, (w as f32, h as f32), RESIZE_BORDER * self.scale.get()) } else { None };
        if edge == self.edge.replace(edge) {
            return;
        }
        let icon = edge.map_or_else(|| cursor(self.cursor.get()), WinitCursor::from);
        self.raw.set_cursor(icon.into());
    }
}

impl Window for LinuxWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn size(&self) -> (u32, u32) {
        self.size.get()
    }

    fn scale(&self) -> f32 {
        self.scale.get()
    }

    fn request_redraw(&self) {
        self.raw.request_redraw();
    }

    fn set_title(&self, title: &str) {
        self.raw.set_title(title);
    }

    fn set_cursor(&self, icon: CursorIcon) {
        if self.cursor.replace(icon) != icon && self.edge.get().is_none() {
            self.raw.set_cursor(cursor(icon).into());
        }
    }

    fn minimize(&self) {
        self.raw.set_minimized(true);
    }

    fn toggle_maximize(&self) {
        if self.raw.fullscreen().is_some() {
            return self.raw.set_fullscreen(None);
        }
        self.raw.set_maximized(!self.raw.is_maximized());
    }

    fn is_maximized(&self) -> bool {
        self.raw.is_maximized()
    }

    fn toggle_fullscreen(&self) {
        let full = self.raw.fullscreen().is_some();
        self.raw.set_fullscreen((!full).then_some(Fullscreen::Borderless(None)));
    }

    fn is_fullscreen(&self) -> bool {
        self.raw.fullscreen().is_some()
    }

    fn close(&self) {
        self.commands.borrow_mut().push(Command::Close(self.id));
    }

    fn activate(&self) {
        self.raw.focus_window();
    }

    fn notify(&self, notification: Notification) {
        let window = self.id;
        let tx = self.notification_tx.clone();
        let waker = self.notification_waker.clone();
        let started = std::thread::Builder::new().name("aster-notification".into()).spawn(move || {
            match crate::notifications::show(&notification) {
                Ok(true) => {
                    if tx.send((window, notification.tab)).is_ok() { waker.wake(); }
                }
                Ok(false) => {}
                Err(error) => tracing::warn!("desktop notification: {error}"),
            }
        });
        if let Err(error) = started {
            tracing::warn!("cannot start desktop notification: {error}");
        }
    }

    fn open_window(&self, opts: WindowOptions) {
        self.commands.borrow_mut().push(Command::Open(opts));
    }

    fn placement(&self) -> Placement {
        let (w, h) = self.size.get();
        let at = self.raw.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
        Placement { x: at.x, y: at.y, width: w as i32, height: h as i32, maximized: self.raw.is_maximized() }
    }

    fn clipboard_text(&self, target: Option<PasteTarget>) -> Option<String> {
        self.commands.borrow_mut().push(Command::Paste(self.id, target));
        None
    }

    fn set_clipboard_text(&self, text: &str) {
        if !text.is_empty() {
            self.commands.borrow_mut().push(Command::Copy(text.to_owned()));
        }
    }

    fn set_clipboard_html(&self, text: &str, _: &str) {
        self.set_clipboard_text(text);
    }

    fn set_ime_area(&self, area: Rect) {
        let data = ImeRequestData::default().with_cursor_area(PhysicalPosition::new(area.x as i32, area.y as i32).into(), PhysicalSize::new(area.w as u32, area.h as u32).into());
        let _ = self.raw.request_ime_update(ImeRequest::Update(data));
    }

    fn is_dark_mode(&self) -> bool {
        self.raw.theme().is_none_or(|t| t == Theme::Dark)
    }

    fn set_dark(&self, dark: bool) {
        self.raw.set_theme(Some(if dark { Theme::Dark } else { Theme::Light }));
    }

    fn set_timer(&self, ms: u32) {
        self.timer.set(Some(Instant::now() + Duration::from_millis(ms.into())));
    }

    fn set_surface(&self, _backdrop: aster_core::config::Backdrop) -> aster_platform::Surface {
        aster_platform::Surface { translucent: self.translucent, backdrop: false }
    }

    fn request_attention(&self) {
        self.raw.request_user_attention(Some(UserAttentionType::Informational));
    }

    fn set_visible(&self, visible: bool) {
        self.raw.set_visible(visible);
        if visible {
            self.raw.focus_window();
        }
    }

    fn is_visible(&self) -> bool {
        self.raw.is_visible().unwrap_or(true)
    }

    fn is_foreground(&self) -> bool {
        self.raw.has_focus()
    }

    fn dock(&self, spec: Option<DockSpec>) {
        self.dock_spec.set(spec);
        if self.is_wayland() { return; }
        if let Some(spec) = spec {
            let monitors = crate::monitors::collect(self.raw.available_monitors(), self.raw.primary_monitor());
            self.apply_dock(spec, &monitors);
        } else {
            self.dock_slide.set(None);
            self.raw.set_window_level(WindowLevel::Normal);
        }
    }

    fn dock_slide(&self) -> Option<DockSlide> {
        self.dock_slide.get().filter(|_| self.free_size() && self.raw.is_minimized() != Some(true))
    }

    fn screen_position(&self) -> Option<(i32, i32)> {
        self.position.get().or_else(|| self.dock_slide.get().map(|slide| (slide.x, slide.shown_y)))
    }

    fn set_position(&self, x: i32, y: i32) -> bool {
        if !self.raw.display_handle().is_ok_and(|h| matches!(h.as_raw(), RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_))) {
            return false;
        }
        self.raw.set_outer_position(PhysicalPosition::new(x, y).into());
        true
    }

    fn set_global_hotkey(&self, _key: Option<(Key, Mods)>) -> bool {
        false
    }

    fn set_progress(&self, _progress: Option<aster_core::app::Progress>) {}
}

fn cursor(icon: CursorIcon) -> WinitCursor {
    match icon {
        CursorIcon::Default => WinitCursor::Default,
        CursorIcon::Text => WinitCursor::Text,
        CursorIcon::Hand => WinitCursor::Pointer,
        CursorIcon::ResizeHorizontal => WinitCursor::EwResize,
        CursorIcon::ResizeVertical => WinitCursor::NsResize,
    }
}

pub(crate) fn resize_edge(x: f32, y: f32, (w, h): (f32, f32), border: f32) -> Option<ResizeDirection> {
    let corner = border * 3.0;
    let (left, right, top, bottom) = (x < border, x >= w - border, y < border, y >= h - border);
    let (near_left, near_right, near_top, near_bottom) = (x < corner, x >= w - corner, y < corner, y >= h - corner);
    Some(match () {
        _ if (top && near_left) || (left && near_top) => ResizeDirection::NorthWest,
        _ if (top && near_right) || (right && near_top) => ResizeDirection::NorthEast,
        _ if (bottom && near_left) || (left && near_bottom) => ResizeDirection::SouthWest,
        _ if (bottom && near_right) || (right && near_bottom) => ResizeDirection::SouthEast,
        _ if top => ResizeDirection::North,
        _ if bottom => ResizeDirection::South,
        _ if left => ResizeDirection::West,
        _ if right => ResizeDirection::East,
        _ => return None,
    })
}

pub(crate) fn map_key(physical: PhysicalKey, unmodified: &WinitKey) -> Key {
    if let WinitKey::Named(named) = unmodified {
        let key = match named {
            NamedKey::ArrowUp => Some(Key::Up),
            NamedKey::ArrowDown => Some(Key::Down),
            NamedKey::ArrowLeft => Some(Key::Left),
            NamedKey::ArrowRight => Some(Key::Right),
            NamedKey::PageUp => Some(Key::PageUp),
            NamedKey::PageDown => Some(Key::PageDown),
            NamedKey::Home => Some(Key::Home),
            NamedKey::End => Some(Key::End),
            NamedKey::Enter => Some(Key::Enter),
            NamedKey::Escape => Some(Key::Escape),
            NamedKey::Backspace => Some(Key::Backspace),
            NamedKey::Delete => Some(Key::Delete),
            NamedKey::Insert => Some(Key::Insert),
            NamedKey::Tab => Some(Key::Tab),
            NamedKey::ContextMenu => Some(Key::Apps),
            NamedKey::Control => Some(Key::Control),
            NamedKey::Shift => Some(Key::Shift),
            NamedKey::Alt | NamedKey::AltGraph => Some(Key::Alt),
            NamedKey::Meta => Some(Key::Meta),
            other => function_key(other),
        };
        if let Some(k) = key {
            return k;
        }
    }
    if let PhysicalKey::Code(code) = physical {
        if let Some(k) = us_position(code) {
            return k;
        }
    }
    match unmodified {
        WinitKey::Character(text) => text.chars().next().filter(|c| c.is_ascii_graphic()).map_or(Key::Other, |c| Key::Char(c.to_ascii_uppercase())),
        _ => Key::Other,
    }
}

fn function_key(named: &NamedKey) -> Option<Key> {
    const KEYS: [NamedKey; 12] = [NamedKey::F1, NamedKey::F2, NamedKey::F3, NamedKey::F4, NamedKey::F5, NamedKey::F6, NamedKey::F7, NamedKey::F8, NamedKey::F9, NamedKey::F10, NamedKey::F11, NamedKey::F12];
    KEYS.iter().position(|k| k == named).map(|i| Key::F(i as u8 + 1))
}

fn us_position(code: KeyCode) -> Option<Key> {
    let letters = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let letter_codes = [
        KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF, KeyCode::KeyG, KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM,
        KeyCode::KeyN, KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR, KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU, KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY, KeyCode::KeyZ,
    ];
    if let Some(i) = letter_codes.iter().position(|&k| k == code) {
        return letters.chars().nth(i).map(Key::Char);
    }
    let digit_codes = [KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9];
    if let Some(i) = digit_codes.iter().position(|&k| k == code) {
        return Some(Key::Char((b'0' + i as u8) as char));
    }
    Some(match code {
        KeyCode::Space => Key::Space,
        KeyCode::Backquote => Key::Char('`'),
        KeyCode::Comma => Key::Char(','),
        KeyCode::Period => Key::Char('.'),
        KeyCode::Equal => Key::Char('='),
        KeyCode::Minus => Key::Char('-'),
        KeyCode::Slash => Key::Char('/'),
        KeyCode::Backslash => Key::Char('\\'),
        KeyCode::BracketLeft => Key::Char('['),
        KeyCode::BracketRight => Key::Char(']'),
        KeyCode::Semicolon => Key::Char(';'),
        KeyCode::Quote => Key::Char('\''),
        KeyCode::NumpadAdd => Key::NumPlus,
        KeyCode::NumpadSubtract => Key::NumMinus,
        KeyCode::NumpadMultiply => Key::NumStar,
        KeyCode::NumpadDivide => Key::NumSlash,
        _ => return None,
    })
}

pub(crate) fn mods(state: ModifiersState) -> Mods {
    Mods { ctrl: state.control_key(), shift: state.shift_key(), alt: state.alt_key(), meta: state.meta_key() }
}

pub(crate) fn control_char(key: Key) -> Option<char> {
    match key {
        Key::Char(c) if c.is_ascii_uppercase() => Some(((c as u8) & 0x1f) as char),
        Key::Char('[') => Some('\u{1b}'),
        Key::Char('\\') => Some('\u{1c}'),
        Key::Char(']') => Some('\u{1d}'),
        _ => None,
    }
}

pub(crate) fn chars_for(key: Key, m: Mods, text: Option<&str>) -> Vec<Event> {
    if m.ctrl && !m.alt {
        return control_char(key).map(Event::Char).into_iter().collect();
    }
    let Some(text) = text else { return Vec::new() };
    let alt_prefix = m.alt && !m.ctrl;
    text.chars().map(|c| if alt_prefix { Event::AltChar(c) } else { Event::Char(c) }).collect()
}

fn capture_keys(keys: &str) -> Vec<Event> {
    let mut out = Vec::new();
    for k in keys.split(';').filter(|k| !k.is_empty()) {
        if let Some(text) = k.strip_prefix('\'') {
            out.extend(text.chars().map(Event::Char));
            continue;
        }
        let (mod_names, name) = k.rsplit_once('+').unwrap_or(("", k));
        let has = |m: &str| mod_names.split('+').any(|x| x.eq_ignore_ascii_case(m));
        let m = Mods { ctrl: has("ctrl"), shift: has("shift"), alt: has("alt"), meta: false };
        let (key, text) = match name {
            "Enter" => (Key::Enter, Some("\r")),
            "Tab" => (Key::Tab, Some("\t")),
            "Escape" => (Key::Escape, Some("\u{1b}")),
            "Up" => (Key::Up, None),
            "Down" => (Key::Down, None),
            _ => (Key::Char(name.chars().next().unwrap_or(' ').to_ascii_uppercase()), None),
        };
        out.push(Event::Key { key, mods: m, pressed: true });
        out.extend(chars_for(key, m, text));
    }
    out
}

fn wheel(delta: MouseScrollDelta, scale: f32) -> (f32, f32) {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => (-x, y),
        MouseScrollDelta::PixelDelta(p) => {
            let notch = WHEEL_PIXELS_PER_NOTCH * scale;
            (-p.x as f32 / notch, p.y as f32 / notch)
        }
        _ => (0.0, 0.0),
    }
}

#[derive(Clone)]
pub struct LinuxWaker {
    proxy: EventLoopProxy,
    pending: Arc<AtomicBool>,
}

impl Waker for LinuxWaker {
    fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            self.proxy.wake_up();
        }
    }
}

pub struct LinuxWindows {
    event_loop: EventLoop,
    pending: Arc<AtomicBool>,
}

impl WindowSystem for LinuxWindows {
    type Window = LinuxWindow;
    type Waker = LinuxWaker;

    fn new() -> Result<Self, PlatformError> {
        let event_loop = EventLoop::new().map_err(|e| PlatformError::new(-1, format!("cannot connect to the display server: {e}")))?;
        let _ = DISPLAY.set(event_loop.owned_display_handle());
        Ok(Self { event_loop, pending: Arc::default() })
    }

    fn waker(&self) -> LinuxWaker {
        LinuxWaker { proxy: self.event_loop.create_proxy(), pending: self.pending.clone() }
    }

    fn run<H: WindowHandler<Self>>(mut self, first: WindowOptions, handler: H) -> Result<(), PlatformError> {
        let (commands, waiting) = if first.visible {
            (vec![Command::Open(first)], None)
        } else {
            (Vec::new(), Some(WindowOptions { visible: true, ..first }))
        };
        let commands = Rc::new(RefCell::new(commands));
        let (notification_tx, notification_clicks) = mpsc::channel();
        let (animation_tx, animation_result) = mpsc::channel();
        let waker = self.waker();
        let animation_waker = waker.clone();
        let _ = std::thread::Builder::new().name("aster-animations".into()).spawn(move || {
            if animation_tx.send(crate::animations::read()).is_ok() {
                animation_waker.wake();
            }
        });
        let mut app = Loop { handler, nodes: Vec::new(), commands, waiting, notification_tx, notification_clicks, notification_waker: waker, pastes: HashMap::new(), pending: self.pending.clone(), next_id: 1, mods: ModifiersState::empty(), started: false, error: None, monitors: Vec::new(), next_monitor_scan: None, animation_result, system_animations: true };
        self.event_loop.run_app_on_demand(&mut app).map_err(|e| PlatformError::new(-1, format!("event loop: {e}")))?;
        app.error.map_or(Ok(()), Err)
    }
}

struct Node {
    window: LinuxWindow,
    last_click: Option<(MouseButton, Instant, f32, f32, u8)>,
    pointer: (f32, f32),
}

struct Loop<H> {
    handler: H,
    nodes: Vec<Node>,
    commands: Commands,
    waiting: Option<WindowOptions>,
    notification_tx: mpsc::Sender<(WindowId, u64)>,
    notification_clicks: mpsc::Receiver<(WindowId, u64)>,
    notification_waker: LinuxWaker,
    animation_result: mpsc::Receiver<bool>,
    system_animations: bool,
    monitors: Vec<Monitor>,
    next_monitor_scan: Option<Instant>,
    pastes: HashMap<AsyncRequestSerial, (WindowId, Option<PasteTarget>)>,
    pending: Arc<AtomicBool>,
    next_id: WindowId,
    mods: ModifiersState,
    started: bool,
    error: Option<PlatformError>,
}

impl<H: WindowHandler<LinuxWindows>> Loop<H> {
    fn settle(&mut self, el: &dyn ActiveEventLoop) {
        if !self.started {
            return;
        }
        loop {
            let commands = std::mem::take(&mut *self.commands.borrow_mut());
            if commands.is_empty() {
                break;
            }
            for command in commands {
                match command {
                    Command::Open(opts) => self.open(el, opts),
                    Command::Close(id) => self.close(id),
                    Command::Paste(window, input) => {
                        if let Ok(Some(id)) = el.clipboard() {
                            match el.fetch_data_transfer(id, &TypeHint::Plaintext) {
                                Ok(serial) => { self.pastes.insert(serial, (window, input)); }
                                Err(e) => tracing::debug!("clipboard has no text: {e}"),
                            }
                        }
                    }
                    Command::Copy(text) => {
                        let data = DataTransferSendBuilder::new(text).with_type(TypeHint::Plaintext, |text: &String, _| Some(text.clone())).build();
                        if let Err(e) = el.set_clipboard(data) {
                            tracing::warn!("cannot set the clipboard: {e}");
                        }
                    }
                }
            }
        }
        if self.nodes.is_empty() && self.waiting.is_none() {
            el.exit();
        }
    }

    fn scan_monitors(&mut self, el: &dyn ActiveEventLoop) {
        if self.nodes.iter().all(|node| node.window.is_wayland()) {
            self.next_monitor_scan = None;
            return;
        }
        let now = Instant::now();
        if self.next_monitor_scan.is_some_and(|next| now < next) {
            return;
        }
        self.next_monitor_scan = Some(now + Duration::from_secs(1));
        let monitors = crate::monitors::collect(el.available_monitors(), el.primary_monitor());
        if self.monitors != monitors {
            self.monitors = monitors;
            for node in &self.nodes {
                node.window.displays_changed(&self.monitors);
            }
        }
    }

    fn open(&mut self, el: &dyn ActiveEventLoop, opts: WindowOptions) {
        let mut attrs = WindowAttributes::default()
            .with_title(opts.title)
            .with_decorations(false)
            .with_min_surface_size(LogicalSize::new(MIN_SIZE.0, MIN_SIZE.1))
            .with_theme(Some(if opts.dark { Theme::Dark } else { Theme::Light }));
        attrs = match opts.placement.filter(|p| p.width > 100 && p.height > 100) {
            Some(p) => attrs.with_surface_size(PhysicalSize::new(p.width as u32, p.height as u32)).with_maximized(p.maximized),
            None => attrs.with_surface_size(LogicalSize::new(DEFAULT_SIZE.0, DEFAULT_SIZE.1)),
        };
        let display = display().and_then(|d| d.display_handle().ok().map(|h| h.as_raw()));
        let visual = display.and_then(crate::gl::window_visual);
        let translucent = visual.is_some_and(|v| v.transparent);
        attrs = attrs.with_transparent(translucent);
        let app_id = crate::desktop::APP_ID;
        if matches!(display, Some(RawDisplayHandle::Wayland(_))) {
            attrs = attrs.with_platform_attributes(Box::new(WindowAttributesWayland::default().with_name(app_id, app_id)));
        } else {
            let mut x11 = WindowAttributesX11::default().with_name(app_id, app_id);
            if let Some(id) = visual.filter(|v| v.transparent).and_then(|v| v.x11_visual) {
                x11 = x11.with_x11_visual(id);
            }
            attrs = attrs.with_platform_attributes(Box::new(x11));
        }
        let raw: Arc<dyn winit::window::Window> = match el.create_window(attrs) {
            Ok(raw) => Arc::from(raw),
            Err(e) => {
                self.error.get_or_insert_with(|| PlatformError::new(-1, format!("cannot open a window: {e}")));
                return;
            }
        };
        let data = ImeRequestData::default().with_cursor_area(PhysicalPosition::new(0, 0).into(), PhysicalSize::new(0, 0).into());
        if let Some(request) = ImeEnableRequest::new(ImeCapabilities::new().with_cursor_area(), data) {
            let _ = raw.request_ime_update(ImeRequest::Enable(request));
        }
        let size = raw.surface_size();
        let id = self.next_id;
        self.next_id += 1;
        let window = LinuxWindow {
            id,
            scale: Cell::new(raw.scale_factor() as f32),
            raw,
            commands: self.commands.clone(),
            notification_tx: self.notification_tx.clone(),
            notification_waker: self.notification_waker.clone(),
            size: Cell::new((size.width, size.height)),
            cursor: Cell::new(CursorIcon::Default),
            edge: Cell::new(None),
            timer: Cell::new(None),
            dock_spec: Cell::new(None),
            dock_slide: Cell::new(None),
            position: Cell::new(None),
            translucent,
        };
        self.nodes.push(Node { window, last_click: None, pointer: (0.0, 0.0) });
        let node = self.nodes.last().expect("node");
        self.handler.created(&node.window);
        self.handler.event(&node.window, Event::Animations(self.system_animations));
        if crate::gl::capture_path().is_some() {
            for ev in capture_keys(&std::env::var("ASTER_CAPTURE_KEYS").unwrap_or_default()) {
                self.handler.event(&node.window, ev);
            }
        }
    }

    fn close(&mut self, id: WindowId) {
        let Some(i) = self.nodes.iter().position(|n| n.window.id == id) else { return };
        let node = self.nodes.remove(i);
        self.pastes.retain(|_, (window, _)| *window != id);
        self.handler.destroyed(&node.window);
    }

    fn fire_timers(&mut self) {
        let now = Instant::now();
        for node in &self.nodes {
            if node.window.timer.get().is_some_and(|at| at <= now) {
                node.window.timer.set(None);
                self.handler.event(&node.window, Event::Timer);
            }
        }
    }

    fn button(&mut self, i: usize, pressed: bool, button: MouseButton) {
        let (handler, node) = (&mut self.handler, &mut self.nodes[i]);
        let w = &node.window;
        let (x, y) = node.pointer;
        if !pressed {
            return handler.event(w, Event::MouseUp { x, y, button });
        }
        if let (MouseButton::Left, Some(edge)) = (button, w.edge.get()) {
            let _ = w.raw.drag_resize_window(edge);
            return;
        }
        let now = Instant::now();
        let slop = 4.0 * w.scale.get();
        let clicks = match node.last_click {
            Some((b, t, lx, ly, n)) if b == button && now.duration_since(t) <= DOUBLE_CLICK && (x - lx).abs() <= slop && (y - ly).abs() <= slop => n % 3 + 1,
            _ => 1,
        };
        node.last_click = Some((button, now, x, y, clicks));
        match (handler.hit_test(w, x, y), button) {
            (HitArea::Caption, MouseButton::Left) if clicks == 2 => w.toggle_maximize(),
            (HitArea::Caption, MouseButton::Left) => {
                let _ = w.raw.drag_window();
            }
            (HitArea::Caption, MouseButton::Right) => w.raw.show_window_menu(PhysicalPosition::new(x as f64, y as f64).into()),
            _ => handler.event(w, Event::MouseDown { x, y, button, clicks: clicks.min(2) }),
        }
    }

    fn dispatch(&mut self, i: usize, event: WindowEvent) {
        let held = self.mods;
        let (handler, node) = (&mut self.handler, &mut self.nodes[i]);
        let w = &node.window;
        match event {
            WindowEvent::RedrawRequested => handler.redraw(w),
            WindowEvent::Moved(at) => w.position.set(Some((at.x, at.y))),
            WindowEvent::SurfaceResized(size) => {
                w.size.set((size.width, size.height));
                handler.event(w, Event::Resized { width: size.width, height: size.height });
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                w.scale.set(scale_factor as f32);
                handler.event(w, Event::ScaleChanged(scale_factor as f32));
            }
            WindowEvent::CloseRequested => handler.event(w, Event::CloseRequested),
            WindowEvent::Focused(focused) => {
                self.mods = ModifiersState::empty();
                handler.event(w, Event::Focus(focused));
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
                handler.event(w, Event::Modifiers(mods(m.state())));
            }
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let key = map_key(event.physical_key, &event.key_without_modifiers);
                let m = mods(held);
                handler.event(w, Event::Key { key, mods: m, pressed });
                if pressed && !key.is_modifier() {
                    for ev in chars_for(key, m, event.text.as_deref()) {
                        handler.event(w, ev);
                    }
                }
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => handler.event(w, Event::Preedit(Preedit::new(text, cursor.map(|(start, _)| start)))),
            WindowEvent::Ime(Ime::Commit(text)) => {
                handler.event(w, Event::Preedit(Preedit::default()));
                for c in text.chars() {
                    handler.event(w, Event::Char(c));
                }
            }
            WindowEvent::PointerMoved { position, primary, .. } if primary => {
                let (x, y) = (position.x as f32, position.y as f32);
                node.pointer = (x, y);
                w.track_edge(x, y);
                handler.event(w, Event::MouseMove { x, y });
            }
            WindowEvent::PointerLeft { .. } => {
                w.edge.set(None);
                handler.event(w, Event::MouseLeave);
            }
            WindowEvent::PointerButton { state, position, button, .. } => {
                node.pointer = (position.x as f32, position.y as f32);
                let mapped = match button.mouse_button() {
                    Some(WinitButton::Left) => Some(MouseButton::Left),
                    Some(WinitButton::Right) => Some(MouseButton::Right),
                    Some(WinitButton::Middle) => Some(MouseButton::Middle),
                    Some(WinitButton::Back) => Some(MouseButton::Back),
                    Some(WinitButton::Forward) => Some(MouseButton::Forward),
                    _ => None,
                };
                if let Some(b) = mapped {
                    self.button(i, state == ElementState::Pressed, b);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = wheel(delta, w.scale.get());
                handler.event(w, Event::Wheel { dx, dy });
            }
            WindowEvent::ThemeChanged(theme) => handler.event(w, Event::ThemeChanged { dark: theme == Theme::Dark }),
            _ => {}
        }
    }
}

impl<H: WindowHandler<LinuxWindows>> ApplicationHandler for Loop<H> {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        self.started = true;
        self.settle(el);
    }

    fn proxy_wake_up(&mut self, el: &dyn ActiveEventLoop) {
        self.pending.store(false, Ordering::Release);
        if let Ok(on) = self.animation_result.try_recv() {
            self.system_animations = on;
            for node in &self.nodes {
                self.handler.event(&node.window, Event::Animations(on));
            }
        }
        if self.waiting.is_some() && self.handler.background_woken() {
            self.commands.borrow_mut().extend(self.waiting.take().map(Command::Open));
        }
        for (window, tab) in self.notification_clicks.try_iter() {
            if let Some(node) = self.nodes.iter().find(|node| node.window.id == window) {
                self.handler.event(&node.window, Event::NotificationClicked(tab));
            }
        }
        for node in &self.nodes {
            self.handler.woken(&node.window);
        }
        self.settle(el);
    }

    fn window_event(&mut self, el: &dyn ActiveEventLoop, id: winit::window::WindowId, event: WindowEvent) {
        if let WindowEvent::DataTransferReceived { serial, value, .. } = event {
            if let Some((window, input)) = self.pastes.remove(&serial) {
                if let Some(node) = self.nodes.iter().find(|n| n.window.id == window) {
                    match value.try_as_string() {
                        Ok(text) => self.handler.event(&node.window, match input {
                            Some(target) => target.event(text),
                            None => Event::Pasted(text),
                        }),
                        Err(e) => tracing::debug!("clipboard text is unreadable: {e}"),
                    }
                }
            }
        } else if let Some(i) = self.nodes.iter().position(|n| n.window.raw.id() == id) {
            self.dispatch(i, event);
        }
        self.settle(el);
    }

    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        self.fire_timers();
        self.settle(el);
        self.scan_monitors(el);
        let next = self.nodes.iter().filter_map(|n| n.window.timer.get()).chain(self.next_monitor_scan).min();
        el.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

#[cfg(test)]
#[path = "../test/window.rs"]
mod tests;
