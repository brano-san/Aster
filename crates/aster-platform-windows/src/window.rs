use crate::err;
use aster_core::input::{CursorIcon, Event, HitArea, Key, Mods, MouseButton, Preedit};
use aster_gfx::render::Rect;
use aster_platform::{dock_rect, DockSpec, Placement, PlatformError, Waker, Window, WindowHandler, WindowId, WindowOptions, WindowSystem};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, MonitorFromWindow, ScreenToClient, ValidateRect, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::WaitForSingleObjectEx;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::Ime::{ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionWindow, CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT, COMPOSITIONFORM, GCS_COMPSTR, GCS_CURSORPOS, GCS_RESULTSTR, IME_COMPOSITION_STRING, ISC_SHOWUICOMPOSITIONWINDOW};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_WAKE: u32 = WM_APP + 1;
const WM_DESTROY_REQUEST: u32 = WM_APP + 2;
const WM_HUB_EVENT: u32 = WM_APP + 3;
const CAPTURE_WAIT_MS: u64 = 1500;
const TIMER_ID: usize = 1;
const HOTKEY_ID: i32 = 1;

enum HubEvent {
    Open(WindowOptions),
    Capture(Vec<Event>),
    ContinueQuit,
}

#[derive(Clone)]
struct HubQueue {
    hwnd: isize,
    tx: mpsc::Sender<HubEvent>,
}

impl HubQueue {
    fn post(&self, event: HubEvent) {
        if self.tx.send(event).is_ok() {
            unsafe { let _ = PostMessageW(Some(HWND(self.hwnd as _)), WM_HUB_EVENT, WPARAM(0), LPARAM(0)); }
        }
    }
}

#[derive(Default)]
pub(crate) struct FrameSync {
    pub waitable: Cell<HANDLE>,
    pub waited: Cell<bool>,
    pub composition: RefCell<Option<crate::d3d::Composition>>,
}

pub struct Win32Window {
    id: WindowId,
    hub: HWND,
    queue: HubQueue,
    closing: Cell<bool>,
    hwnd: Cell<HWND>,
    redraw: Cell<bool>,
    size: Cell<(u32, u32)>,
    dpi: Cell<u32>,
    cursor: Cell<CursorIcon>,
    tracking: Cell<bool>,
    nc_tracking: Cell<bool>,
    high_surrogate: Cell<u16>,
    ime_area: Cell<Rect>,
    restore: Cell<Option<(WINDOWPLACEMENT, isize)>>,
    pub(crate) sync: Rc<FrameSync>,
}

impl Win32Window {
    fn copy_to_clipboard(&self, text: &str, html: Option<&str>) {
        if text.is_empty() {
            return;
        }
        let unicode: Vec<u8> = text.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect();
        let html = html.map(clipboard_html);
        unsafe {
            if !open_clipboard(self.hwnd()) {
                return;
            }
            if EmptyClipboard().is_ok() {
                if let Some(html) = html {
                    let format = RegisterClipboardFormatW(w!("HTML Format"));
                    if format != 0 {
                        offer_clipboard(format, html.as_bytes());
                    }
                }
                offer_clipboard(CF_UNICODETEXT.0 as u32, &unicode);
            }
            let _ = CloseClipboard();
        }
    }

    fn new(id: WindowId, hub: HWND, queue: HubQueue) -> Self {
        Self {
            id,
            hub,
            queue,
            closing: Cell::new(false),
            hwnd: Cell::new(HWND::default()),
            redraw: Cell::new(true),
            size: Cell::new((0, 0)),
            dpi: Cell::new(96),
            cursor: Cell::new(CursorIcon::Default),
            tracking: Cell::new(false),
            nc_tracking: Cell::new(false),
            high_surrogate: Cell::new(0),
            ime_area: Cell::new(Rect::default()),
            restore: Cell::new(None),
            sync: Rc::new(FrameSync::default()),
        }
    }

    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd.get()
    }

    fn full(&self) -> bool {
        let r = self.restore.take();
        let on = r.is_some();
        self.restore.set(r);
        on
    }

    fn leave_full(&self) {
        let Some((wp, style)) = self.restore.take() else { return };
        let h = self.hwnd();
        unsafe {
            SetWindowLongPtrW(h, GWL_STYLE, style);
            let _ = SetWindowPos(h, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER);
            let _ = SetWindowPlacement(h, &wp);
        }
    }

    fn enter_full(&self) {
        let h = self.hwnd();
        unsafe {
            let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
            let _ = GetWindowPlacement(h, &mut wp);
            let style = GetWindowLongPtrW(h, GWL_STYLE);
            self.restore.set(Some((wp, style)));
            let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mut mi);
            let r = mi.rcMonitor;
            SetWindowLongPtrW(h, GWL_STYLE, style & !(WS_OVERLAPPEDWINDOW.0 as isize));
            let _ = SetWindowPos(h, Some(HWND_TOP), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_FRAMECHANGED | SWP_NOOWNERZORDER);
        }
    }

    fn post(&self, msg: u32, wp: usize) {
        unsafe {
            let _ = PostMessageW(Some(self.hwnd()), msg, WPARAM(wp), LPARAM(0));
        }
    }
}

#[derive(Clone)]
pub struct Win32Waker {
    hwnd: isize,
    pending: Arc<AtomicBool>,
}

impl Waker for Win32Waker {
    fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            unsafe {
                let _ = PostMessageW(Some(HWND(self.hwnd as _)), WM_WAKE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn clipboard_html(fragment: &str) -> String {
    let prefix = "<html><body><!--StartFragment-->";
    let suffix = "<!--EndFragment--></body></html>";
    let header = |start, end, first, last| format!("Version:1.0\r\nStartHTML:{start:010}\r\nEndHTML:{end:010}\r\nStartFragment:{first:010}\r\nEndFragment:{last:010}\r\n");
    let start = header(0, 0, 0, 0).len();
    let first = start + prefix.len();
    let last = first + fragment.len();
    let mut data = header(start, last + suffix.len(), first, last);
    data.push_str(prefix);
    data.push_str(fragment);
    data.push_str(suffix);
    data.push('\0');
    data
}

unsafe fn offer_clipboard(format: u32, bytes: &[u8]) {
    let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return };
    let destination = GlobalLock(memory).cast::<u8>();
    if !destination.is_null() {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
        let _ = GlobalUnlock(memory);
        if SetClipboardData(format, Some(HANDLE(memory.0))).is_ok() {
            return;
        }
    }
    let _ = GlobalFree(Some(memory));
}

unsafe fn open_clipboard(hwnd: HWND) -> bool {
    for _ in 0..10 {
        if OpenClipboard(Some(hwnd)).is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

impl Window for Win32Window {
    fn id(&self) -> WindowId {
        self.id
    }

    fn activate(&self) {
        unsafe {
            if IsIconic(self.hwnd()).as_bool() {
                let _ = ShowWindow(self.hwnd(), SW_RESTORE);
            }
            let _ = SetForegroundWindow(self.hwnd());
        }
    }

    fn set_surface(&self, backdrop: aster_core::config::Backdrop) -> aster_platform::Surface {
        use aster_core::config::Backdrop;
        use windows::Win32::Graphics::Dwm::{DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE};
        let h = self.hwnd();
        let kind = match backdrop {
            Backdrop::None => DWMSBT_NONE,
            Backdrop::Mica => DWMSBT_MAINWINDOW,
            Backdrop::Acrylic => DWMSBT_TRANSIENTWINDOW,
        };
        let applied = unsafe { DwmSetWindowAttribute(h, DWMWA_SYSTEMBACKDROP_TYPE, &kind as *const _ as _, 4).is_ok() };
        aster_platform::Surface { translucent: true, backdrop: applied && backdrop != Backdrop::None }
    }

    fn request_attention(&self) {
        let h = self.hwnd();
        if h.is_invalid() || unsafe { GetForegroundWindow() } == h {
            return;
        }
        let info = FLASHWINFO { cbSize: size_of::<FLASHWINFO>() as u32, hwnd: h, dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG, uCount: 0, dwTimeout: 0 };
        unsafe {
            let _ = FlashWindowEx(&info);
        }
    }

    fn open_window(&self, opts: WindowOptions) {
        self.queue.post(HubEvent::Open(opts));
    }

    fn size(&self) -> (u32, u32) {
        self.size.get()
    }

    fn scale(&self) -> f32 {
        self.dpi.get() as f32 / 96.0
    }

    fn request_redraw(&self) {
        self.redraw.set(true);
    }

    fn set_title(&self, title: &str) {
        let w = wide(title);
        unsafe {
            let _ = SetWindowTextW(self.hwnd(), PCWSTR(w.as_ptr()));
        }
    }

    fn set_cursor(&self, cursor: CursorIcon) {
        if self.cursor.replace(cursor) != cursor {
            unsafe {
                SetCursor(Some(load_cursor(cursor)));
            }
        }
    }

    fn minimize(&self) {
        self.post(WM_SYSCOMMAND, SC_MINIMIZE as usize);
    }

    fn toggle_maximize(&self) {
        if self.full() {
            return self.leave_full();
        }
        let cmd = if self.is_maximized() { SC_RESTORE } else { SC_MAXIMIZE };
        self.post(WM_SYSCOMMAND, cmd as usize);
    }

    fn is_maximized(&self) -> bool {
        unsafe { IsZoomed(self.hwnd()).as_bool() }
    }

    fn toggle_fullscreen(&self) {
        if self.full() {
            self.leave_full();
        } else {
            self.enter_full();
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.full()
    }

    fn close(&self) {
        self.closing.set(true);
        self.post(WM_DESTROY_REQUEST, 0);
    }

    fn placement(&self) -> Placement {
        let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        match self.restore.get() {
            Some((saved, _)) => wp = saved,
            None => unsafe {
                let _ = GetWindowPlacement(self.hwnd(), &mut wp);
            },
        }
        let r = wp.rcNormalPosition;
        Placement { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, maximized: wp.showCmd == SW_SHOWMAXIMIZED.0 as u32 }
    }

    fn clipboard_text(&self, _vault_input: Option<u64>) -> Option<String> {
        unsafe {
            if !open_clipboard(self.hwnd()) {
                return None;
            }
            let text = (|| {
                let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
                let units = GlobalSize(HGLOBAL(h.0)) / 2;
                let p = GlobalLock(HGLOBAL(h.0)) as *const u16;
                if p.is_null() {
                    return None;
                }
                let all = std::slice::from_raw_parts(p, units);
                let n = all.iter().position(|&c| c == 0).unwrap_or(units);
                let s = String::from_utf16_lossy(&all[..n]);
                let _ = GlobalUnlock(HGLOBAL(h.0));
                Some(s)
            })();
            let _ = CloseClipboard();
            text
        }
    }

    fn set_clipboard_text(&self, text: &str) {
        self.copy_to_clipboard(text, None);
    }

    fn set_clipboard_html(&self, text: &str, html: &str) {
        self.copy_to_clipboard(text, Some(html));
    }

    fn set_ime_area(&self, area: Rect) {
        self.ime_area.set(area);
    }

    fn is_dark_mode(&self) -> bool {
        crate::dark_mode()
    }

    fn set_dark(&self, dark: bool) {
        apply_dark(self.hwnd(), dark);
    }

    fn set_timer(&self, ms: u32) {
        unsafe { SetTimer(Some(self.hwnd()), TIMER_ID, ms.max(1), None) };
    }

    fn set_visible(&self, visible: bool) {
        let h = self.hwnd();
        unsafe {
            if visible {
                let _ = ShowWindow(h, if IsIconic(h).as_bool() { SW_RESTORE } else { SW_SHOW });
                let _ = SetForegroundWindow(h);
            } else {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    }

    fn is_visible(&self) -> bool {
        let h = self.hwnd();
        unsafe { IsWindowVisible(h).as_bool() && !IsIconic(h).as_bool() }
    }

    fn is_foreground(&self) -> bool {
        unsafe { GetForegroundWindow() == self.hwnd() }
    }

    fn dock(&self, spec: Option<DockSpec>) {
        let h = self.hwnd();
        unsafe {
            let Some(spec) = spec else {
                let _ = SetWindowPos(h, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                return;
            };
            if IsZoomed(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            if !GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
                return;
            }
            let r = mi.rcWork;
            let (x, y, w, hgt) = dock_rect((r.left, r.top, r.right - r.left, r.bottom - r.top), spec);
            let after = if spec.on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
            let _ = SetWindowPos(h, Some(after), x, y, w, hgt, SWP_NOACTIVATE);
        }
    }

    fn set_progress(&self, progress: Option<aster_core::app::Progress>) {
        use aster_core::app::ProgressState;
        use windows::Win32::UI::Shell::{TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED};
        let Some(list) = taskbar() else { return };
        let h = self.hwnd();
        unsafe {
            let Some(p) = progress else {
                let _ = list.SetProgressState(h, TBPF_NOPROGRESS);
                return;
            };
            let state = match p.state {
                ProgressState::Normal => TBPF_NORMAL,
                ProgressState::Error => TBPF_ERROR,
                ProgressState::Paused => TBPF_PAUSED,
                ProgressState::Indeterminate => TBPF_INDETERMINATE,
            };
            let _ = list.SetProgressState(h, state);
            if p.state != ProgressState::Indeterminate {
                let done = if p.state == ProgressState::Error { 100 } else { u64::from(p.percent) };
                let _ = list.SetProgressValue(h, done, 100);
            }
        }
    }

    fn set_global_hotkey(&self, key: Option<(Key, Mods)>) -> bool {
        unsafe {
            let _ = UnregisterHotKey(Some(self.hub), HOTKEY_ID);
            let Some((key, mods)) = key else { return true };
            let Some(vk) = vk_of(key) else { return false };
            let mut m = MOD_NOREPEAT;
            if mods.ctrl {
                m |= MOD_CONTROL;
            }
            if mods.alt {
                m |= MOD_ALT;
            }
            if mods.shift {
                m |= MOD_SHIFT;
            }
            if mods.meta {
                m |= MOD_WIN;
            }
            RegisterHotKey(Some(self.hub), HOTKEY_ID, m, vk).is_ok()
        }
    }
}

fn fit_rect(r: RECT, area: RECT) -> RECT {
    let w = (r.right - r.left).clamp(0, (area.right - area.left).max(0));
    let h = (r.bottom - r.top).clamp(0, (area.bottom - area.top).max(0));
    let left = r.left.clamp(area.left, area.right - w);
    let top = r.top.clamp(area.top, area.bottom - h);
    RECT { left, top, right: left + w, bottom: top + h }
}

fn within_monitor(r: RECT) -> RECT {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let monitor = unsafe { MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST) };
    if !unsafe { GetMonitorInfoW(monitor, &mut mi) }.as_bool() {
        return r;
    }
    fit_rect(r, mi.rcWork)
}

fn load_cursor(c: CursorIcon) -> HCURSOR {
    let id = match c {
        CursorIcon::Default => IDC_ARROW,
        CursorIcon::Text => IDC_IBEAM,
        CursorIcon::Hand => IDC_HAND,
        CursorIcon::ResizeHorizontal => IDC_SIZEWE,
        CursorIcon::ResizeVertical => IDC_SIZENS,
    };
    unsafe { LoadCursorW(None, id).unwrap_or_default() }
}

fn cloak(hwnd: HWND, on: bool) {
    let v = BOOL::from(on);
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, &v as *const BOOL as _, 4);
    }
}

pub(crate) fn apply_dark(hwnd: HWND, dark: bool) {
    let v = BOOL::from(dark);
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &v as *const BOOL as _, 4);
    }
}

pub(crate) struct Node<H> {
    window: Win32Window,
    queue: RefCell<VecDeque<Event>>,
    ready: Cell<bool>,
    focused_at: Cell<Option<std::time::Instant>>,
    shared: *const Shared<H>,
}

struct Shared<H> {
    tray: crate::tray::Tray,
    new_window: WindowOptions,
    quit_window: Cell<Option<WindowId>>,
    queue: HubQueue,
    events: mpsc::Receiver<HubEvent>,
    handler: RefCell<H>,
    nodes: RefCell<Vec<Rc<Node<H>>>>,
    dead: RefCell<Vec<Rc<Node<H>>>>,
    woken: Cell<bool>,
    next_id: Cell<WindowId>,
    hinst: HINSTANCE,
    hub: HWND,
    wake_pending: Arc<AtomicBool>,
}

impl<H: WindowHandler<Win32>> Node<H> {
    fn shared(&self) -> &Shared<H> {
        unsafe { &*self.shared }
    }

    fn send(&self, ev: Event) {
        self.queue.borrow_mut().push_back(ev);
        self.shared().flush();
    }

    fn hit_test(&self, x: f32, y: f32) -> HitArea {
        if !self.ready.get() {
            return HitArea::Client;
        }
        self.shared().handler.try_borrow_mut().map_or(HitArea::Client, |mut h| h.hit_test(&self.window, x, y))
    }

    fn redraw_now(&self) {
        let sh = self.shared();
        match sh.handler.try_borrow_mut() {
            Ok(mut h) if self.ready.get() => {
                self.window.redraw.set(false);
                h.redraw(&self.window);
            }
            _ => self.window.redraw.set(true),
        }
        sh.flush();
    }
}

impl<H: WindowHandler<Win32>> Shared<H> {
    fn nodes(&self) -> Vec<Rc<Node<H>>> {
        self.nodes.borrow().clone()
    }

    fn flush(&self) {
        let Ok(mut h) = self.handler.try_borrow_mut() else { return };
        loop {
            let dead = std::mem::take(&mut *self.dead.borrow_mut());
            for n in &dead {
                h.destroyed(&n.window);
            }
            if self.woken.replace(false) {
                let nodes = self.nodes();
                if nodes.is_empty() && h.background_woken() {
                    self.queue.post(HubEvent::Open(self.new_window.clone()));
                }
                for n in nodes {
                    h.woken(&n.window);
                }
            }
            let mut idle = dead.is_empty();
            for n in self.nodes() {
                loop {
                    let next = n.queue.borrow_mut().pop_front();
                    let Some(ev) = next else { break };
                    idle = false;
                    h.event(&n.window, ev);
                    if self.quit_window.get() == Some(n.window.id) && !n.window.closing.get() && !h.confirming_close(&n.window) {
                        self.quit_window.set(None);
                        h.quitting(false);
                    }
                }
            }
            if idle && !self.woken.get() && self.dead.borrow().is_empty() {
                break;
            }
        }
        let had_icon = self.tray.installed.get();
        self.tray.set_enabled(h.tray_enabled());
        if had_icon && !self.tray.installed.get() && self.nodes.borrow().is_empty() {
            unsafe { PostQuitMessage(0) };
        }
    }

    fn tray_command(&self, command: crate::tray::Command) {
        use crate::tray::Action;
        let recent = recent_window(&self.nodes.borrow());
        match command.resolve(recent) {
            Action::Exit => unsafe { PostQuitMessage(0) },
            Action::Close(n) => {
                n.window.set_visible(true);
                if let Ok(mut handler) = self.handler.try_borrow_mut() {
                    handler.quitting(true);
                }
                self.quit_window.set(Some(n.window.id));
                n.send(Event::CloseRequested);
            }
            Action::Show(n) => n.window.set_visible(true),
            Action::Open => self.queue.post(HubEvent::Open(self.new_window.clone())),
        }
    }

    unsafe fn open(&self, opts: WindowOptions) -> Result<(), PlatformError> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let node = Rc::new(Node { window: Win32Window::new(id, self.hub, self.queue.clone()), queue: RefCell::default(), ready: Cell::new(false), focused_at: Cell::new(None), shared: self });
        let sys = GetDpiForSystem() as f32 / 96.0;
        let title = wide(&opts.title);
        let hidden = crate::d3d::hidden();
        let (ex, at) = if hidden { (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, -32000) } else { (WS_EX_APPWINDOW, CW_USEDEFAULT) };
        let wanted = std::env::var("ASTER_CAPTURE_SIZE").ok().filter(|_| hidden).and_then(|v| v.split_once(',').and_then(|(w, h)| Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))));
        let (width, height) = wanted.unwrap_or((1000.0, 640.0));
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | ex,
            WINDOW_CLASS,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            at,
            at,
            (width * sys) as i32,
            (height * sys) as i32,
            None,
            None,
            Some(self.hinst),
            Some(Rc::as_ptr(&node) as *const _),
        )
        .map_err(err)?;
        let mut maximized = false;
        if let Some(p) = opts.placement.filter(|p| !hidden && p.width > 100 && p.height > 100) {
            let saved = RECT { left: p.x, top: p.y, right: p.x + p.width, bottom: p.y + p.height };
            let room = within_monitor(saved);
            let wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, showCmd: SW_HIDE.0 as u32, rcNormalPosition: room, ..Default::default() };
            let _ = SetWindowPlacement(hwnd, &wp);
            maximized = p.maximized;
        }
        node.window.dpi.set(GetDpiForWindow(hwnd));
        apply_dark(hwnd, opts.dark);
        windows::Win32::UI::Shell::DragAcceptFiles(hwnd, true);
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        update_size(&node.window);
        self.nodes.borrow_mut().push(node.clone());
        if let Ok(mut h) = self.handler.try_borrow_mut() {
            h.created(&node.window);
        }
        node.ready.set(true);
        self.flush();
        node.redraw_now();
        if hidden {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            if let Ok(keys) = std::env::var("ASTER_CAPTURE_KEYS") {
                let mut batches = capture_keys(&keys).into_iter();
                node.queue.borrow_mut().extend(batches.next().unwrap_or_default());
                self.woken.set(true);
                let rest: Vec<Vec<Event>> = batches.collect();
                let queue = self.queue.clone();
                std::thread::spawn(move || {
                    for batch in rest {
                        std::thread::sleep(std::time::Duration::from_millis(CAPTURE_WAIT_MS));
                        queue.post(HubEvent::Capture(batch));
                    }
                });
            }
        } else {
            cloak(hwnd, true);
            let _ = ShowWindow(hwnd, if maximized { SW_SHOWMAXIMIZED } else { SW_SHOW });
            update_size(&node.window);
            self.flush();
            node.redraw_now();
            cloak(hwnd, false);
        }
        Ok(())
    }

    fn destroyed(&self, hwnd: HWND) {
        let mut nodes = self.nodes.borrow_mut();
        let Some(i) = nodes.iter().position(|n| n.window.hwnd() == hwnd) else { return };
        let node = nodes.remove(i);
        let quitting = self.quit_window.get() == Some(node.window.id);
        self.dead.borrow_mut().push(node);
        let last = nodes.is_empty();
        drop(nodes);
        self.flush();
        if last && (quitting || !self.tray.installed.get()) {
            unsafe { PostQuitMessage(0) };
        } else if quitting {
            self.quit_window.set(None);
            self.queue.post(HubEvent::ContinueQuit);
        }
    }
}

fn recent_window<H>(nodes: &[Rc<Node<H>>]) -> Option<Rc<Node<H>>> {
    nodes.iter().max_by_key(|n| (n.focused_at.get(), n.window.id)).cloned()
}

fn capture_keys(keys: &str) -> Vec<Vec<Event>> {
    keys.split(";Wait").map(capture_batch).collect()
}

fn capture_batch(keys: &str) -> Vec<Event> {
    keys.split(';')
        .filter(|k| !k.is_empty())
        .flat_map(|k| match k.strip_prefix('\'') {
            Some(text) => text.chars().map(Event::Char).collect::<Vec<_>>(),
            None if k.contains('@') => capture_click(k),
            None => {
                let key = capture_key(k);
                let produced = match key {
                    Event::Key { key: Key::Enter, mods, .. } if !mods.ctrl => Some('\r'),
                    Event::Key { key: Key::Tab, .. } => Some('\t'),
                    Event::Key { key: Key::Backspace, .. } => Some('\u{8}'),
                    Event::Key { key: Key::Escape, .. } => Some('\u{1b}'),
                    Event::Key { key: Key::Char(c), mods, .. } if mods.ctrl && c.is_ascii_alphabetic() => Some(((c as u8) & 0x1f) as char),
                    _ => None,
                };
                std::iter::once(key).chain(produced.map(Event::Char)).collect::<Vec<_>>()
            }
        })
        .collect()
}

fn capture_click(text: &str) -> Vec<Event> {
    let Some((kind, at)) = text.split_once('@') else { return Vec::new() };
    let Some((x, y)) = at.split_once(',').and_then(|(x, y)| Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))) else { return Vec::new() };
    let (button, clicks) = match kind {
        "RightClick" => (MouseButton::Right, 1),
        "MiddleClick" => (MouseButton::Middle, 1),
        "DoubleClick" => (MouseButton::Left, 2),
        _ => (MouseButton::Left, 1),
    };
    let mut events = vec![Event::MouseMove { x, y }];
    for n in 1..=clicks {
        events.push(Event::MouseDown { x, y, button, clicks: n });
        events.push(Event::MouseUp { x, y, button });
    }
    events
}

fn capture_key(text: &str) -> Event {
    let (mods, name) = text.rsplit_once('+').unwrap_or(("", text));
    let has = |m: &str| mods.split('+').any(|x| x.eq_ignore_ascii_case(m));
    let key = match name {
        "Up" => Key::Up,
        "Down" => Key::Down,
        "Left" => Key::Left,
        "Right" => Key::Right,
        "Enter" => Key::Enter,
        "Escape" => Key::Escape,
        "Tab" => Key::Tab,
        "Space" => Key::Space,
        "Backspace" => Key::Backspace,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        f if f.len() > 1 && f.starts_with('F') && f[1..].parse::<u8>().is_ok() => Key::F(f[1..].parse().unwrap_or(1)),
        _ => Key::Char(name.chars().next().unwrap_or(' ').to_ascii_uppercase()),
    };
    Event::Key { key, mods: Mods { ctrl: has("ctrl"), shift: has("shift"), alt: has("alt"), meta: false }, pressed: true }
}

const WINDOW_CLASS: PCWSTR = w!("AsterWindow");
const HUB_CLASS: PCWSTR = w!("AsterHub");

type HubDispatch = unsafe fn(*const (), u32, WPARAM, LPARAM);

thread_local! {
    static TASKBAR_CREATED: u32 = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    static HUB: Cell<Option<(*const (), HubDispatch)>> = const { Cell::new(None) };
}

unsafe extern "system" fn hub_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if !matches!(msg, WM_WAKE | WM_HUB_EVENT | WM_HOTKEY | crate::tray::CALLBACK) && !TASKBAR_CREATED.with(|&id| id != 0 && msg == id) {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    if let Some((ctx, dispatch)) = HUB.get() {
        dispatch(ctx, msg, wp, lp);
    }
    LRESULT(0)
}

unsafe fn hub_dispatch<H: WindowHandler<Win32>>(ctx: *const (), msg: u32, wp: WPARAM, lp: LPARAM) {
    let sh = &*(ctx as *const Shared<H>);
    if msg == WM_WAKE {
        sh.wake_pending.store(false, Ordering::Release);
        sh.woken.set(true);
        sh.flush();
        return;
    }
    if TASKBAR_CREATED.with(|&id| msg == id) {
        sh.tray.taskbar_created();
        if !sh.tray.installed.get() && sh.nodes.borrow().is_empty() {
            PostQuitMessage(0);
        }
        return;
    }
    if msg == crate::tray::CALLBACK {
        if let Some(command) = sh.tray.command(wp, lp) {
            sh.tray_command(command);
        }
        return;
    }
    if msg == WM_HOTKEY {
        if let Some(n) = sh.nodes().first() {
            n.send(Event::GlobalHotkey);
            if n.window.redraw.get() {
                n.redraw_now();
            }
        }
        return;
    }
    if sh.handler.try_borrow_mut().is_err() {
        let _ = PostMessageW(Some(sh.hub), WM_HUB_EVENT, WPARAM(0), LPARAM(0));
        return;
    }
    match sh.events.try_recv() {
        Ok(HubEvent::ContinueQuit) => sh.tray_command(crate::tray::Command::Quit),
        Ok(HubEvent::Open(opts)) => {
            if let Err(e) = sh.open(opts) {
                tracing::error!("cannot open window: {e}");
            }
        }
        Ok(HubEvent::Capture(batch)) => {
            if let Some(n) = sh.nodes().first() {
                for ev in batch {
                    n.send(ev);
                }
                if n.window.redraw.get() {
                    n.redraw_now();
                }
            }
        }
        Err(_) => {}
    }
}

pub struct Win32 {
    hinst: HINSTANCE,
    hub: HWND,
    wake_pending: Arc<AtomicBool>,
}

impl WindowSystem for Win32 {
    type Window = Win32Window;
    type Waker = Win32Waker;

    fn new() -> Result<Self, PlatformError> {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let hinst: HINSTANCE = GetModuleHandleW(None).map_err(err)?.into();
            let wc = WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(hub_proc), hInstance: hinst, lpszClassName: HUB_CLASS, ..Default::default() };
            RegisterClassExW(&wc);
            let hub = CreateWindowExW(WS_EX_TOOLWINDOW, HUB_CLASS, PCWSTR::null(), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst), None).map_err(err)?;
            Ok(Self { hinst, hub, wake_pending: Arc::new(AtomicBool::new(false)) })
        }
    }

    fn waker(&self) -> Win32Waker {
        Win32Waker { hwnd: self.hub.0 as isize, pending: self.wake_pending.clone() }
    }

    fn run<H: WindowHandler<Self>>(self, first: WindowOptions, handler: H) -> Result<(), PlatformError> {
        unsafe {
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_DBLCLKS,
                lpfnWndProc: Some(wndproc::<H>),
                hInstance: self.hinst,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hIcon: LoadIconW(Some(self.hinst), PCWSTR(1 as _)).unwrap_or_default(),
                lpszClassName: WINDOW_CLASS,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let (tx, events) = mpsc::channel();
            let shared = Shared {
                queue: HubQueue { hwnd: self.hub.0 as isize, tx },
                events,
                tray: crate::tray::Tray::new(self.hub, self.hinst),
                quit_window: Cell::new(None),
                new_window: WindowOptions { placement: None, ..first.clone() },
                handler: RefCell::new(handler),
                nodes: RefCell::default(),
                dead: RefCell::default(),
                woken: Cell::new(false),
                next_id: Cell::new(1),
                hinst: self.hinst,
                hub: self.hub,
                wake_pending: self.wake_pending.clone(),
            };
            HUB.set(Some((&shared as *const Shared<H> as *const (), hub_dispatch::<H> as HubDispatch)));
            let result = shared.open(first).and_then(|()| message_loop(&shared));
            HUB.set(None);
            for n in shared.nodes() {
                SetWindowLongPtrW(n.window.hwnd(), GWLP_USERDATA, 0);
            }
            result
        }
    }
}

unsafe fn update_size(w: &Win32Window) -> bool {
    let mut rc = RECT::default();
    let _ = GetClientRect(w.hwnd(), &mut rc);
    let size = ((rc.right - rc.left).max(0) as u32, (rc.bottom - rc.top).max(0) as u32);
    let changed = size != w.size.get();
    w.size.set(size);
    changed
}

unsafe fn pump(msg: &mut MSG) -> bool {
    while PeekMessageW(msg, None, 0, 0, PM_REMOVE).as_bool() {
        if msg.message == WM_QUIT {
            return false;
        }
        let _ = TranslateMessage(msg);
        DispatchMessageW(msg);
    }
    true
}

unsafe fn message_loop<H: WindowHandler<Win32>>(sh: &Shared<H>) -> Result<(), PlatformError> {
    let mut msg = MSG::default();
    loop {
        if !pump(&mut msg) && sh.nodes().is_empty() {
            return Ok(());
        }
        let dirty: Vec<Rc<Node<H>>> = sh.nodes().into_iter().filter(|n| n.window.redraw.get()).collect();
        if dirty.is_empty() {
            let _ = WaitMessage();
            continue;
        }
        let paced: Vec<&Rc<Node<H>>> = dirty.iter().filter(|n| !n.window.sync.waitable.get().is_invalid()).collect();
        if !paced.is_empty() {
            let handles: Vec<HANDLE> = paced.iter().map(|n| n.window.sync.waitable.get()).collect();
            let r = MsgWaitForMultipleObjectsEx(Some(&handles), 1000, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            let k = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            if k == handles.len() {
                continue;
            }
            for (i, n) in paced.iter().enumerate() {
                if i == k || WaitForSingleObjectEx(handles[i], 0, false) == WAIT_OBJECT_0 {
                    n.window.sync.waited.set(true);
                }
            }
            if !pump(&mut msg) && sh.nodes().is_empty() {
                return Ok(());
            }
        }
        for n in &dirty {
            let s = &n.window.sync;
            if n.window.redraw.get() && (s.waited.get() || s.waitable.get().is_invalid()) {
                n.redraw_now();
            }
        }
    }
}

fn frame_thickness(dpi: u32) -> i32 {
    unsafe { GetSystemMetricsForDpi(SM_CXFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi) }
}

fn lparam_point(lp: LPARAM) -> POINT {
    POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 }
}

fn mods() -> Mods {
    let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(vk.0 as i32) } < 0;
    Mods { ctrl: down(VK_CONTROL), shift: down(VK_SHIFT), alt: down(VK_MENU), meta: down(VK_LWIN) || down(VK_RWIN) }
}

pub(crate) fn vk_of(key: Key) -> Option<u32> {
    let vk = match key {
        Key::Up => VK_UP,
        Key::Down => VK_DOWN,
        Key::Left => VK_LEFT,
        Key::Right => VK_RIGHT,
        Key::PageUp => VK_PRIOR,
        Key::PageDown => VK_NEXT,
        Key::Home => VK_HOME,
        Key::End => VK_END,
        Key::Enter => VK_RETURN,
        Key::Escape => VK_ESCAPE,
        Key::Backspace => VK_BACK,
        Key::Delete => VK_DELETE,
        Key::Insert => VK_INSERT,
        Key::Tab => VK_TAB,
        Key::Space => VK_SPACE,
        Key::F(n) if (1..=24).contains(&n) => VIRTUAL_KEY(VK_F1.0 + n as u16 - 1),
        Key::Char('`') => VK_OEM_3,
        Key::Char(';') => VK_OEM_1,
        Key::Char('/') => VK_OEM_2,
        Key::Char('[') => VK_OEM_4,
        Key::Char('\\') => VK_OEM_5,
        Key::Char(']') => VK_OEM_6,
        Key::Char('\'') => VK_OEM_7,
        Key::Char(',') => VK_OEM_COMMA,
        Key::Char('.') => VK_OEM_PERIOD,
        Key::Char('=') => VK_OEM_PLUS,
        Key::Char('-') => VK_OEM_MINUS,
        Key::Char(c) if c.is_ascii_uppercase() || c.is_ascii_digit() => VIRTUAL_KEY(c as u16),
        _ => return None,
    };
    Some(vk.0 as u32)
}

fn map_key(vk: u16) -> Key {
    let vk = VIRTUAL_KEY(vk);
    match vk {
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_PRIOR => Key::PageUp,
        VK_NEXT => Key::PageDown,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_RETURN => Key::Enter,
        VK_ESCAPE => Key::Escape,
        VK_BACK => Key::Backspace,
        VK_DELETE => Key::Delete,
        VK_INSERT => Key::Insert,
        VK_TAB => Key::Tab,
        VK_SPACE => Key::Space,
        VK_APPS => Key::Apps,
        VK_OEM_1 => Key::Char(';'),
        VK_OEM_2 => Key::Char('/'),
        VK_OEM_3 => Key::Char('`'),
        VK_OEM_4 => Key::Char('['),
        VK_OEM_5 => Key::Char('\\'),
        VK_OEM_6 => Key::Char(']'),
        VK_OEM_7 => Key::Char('\''),
        VK_OEM_COMMA => Key::Char(','),
        VK_OEM_PERIOD => Key::Char('.'),
        VK_OEM_PLUS => Key::Char('='),
        VK_OEM_MINUS => Key::Char('-'),
        VK_ADD => Key::NumPlus,
        VK_SUBTRACT => Key::NumMinus,
        VK_MULTIPLY => Key::NumStar,
        VK_DIVIDE => Key::NumSlash,
        _ if (VK_F1.0..=VK_F24.0).contains(&vk.0) => Key::F((vk.0 - VK_F1.0 + 1) as u8),
        _ if (VK_NUMPAD0.0..=VK_NUMPAD9.0).contains(&vk.0) => Key::Char((b'0' + (vk.0 - VK_NUMPAD0.0) as u8) as char),
        _ if (b'A' as u16..=b'Z' as u16).contains(&vk.0) || (b'0' as u16..=b'9' as u16).contains(&vk.0) => Key::Char(vk.0 as u8 as char),
        _ => Key::Other,
    }
}

fn preedit_from_utf16(units: &[u16], cursor: i32) -> Preedit {
    let text = String::from_utf16_lossy(units);
    let cursor = usize::try_from(cursor).ok().filter(|&c| c <= units.len()).map(|c| String::from_utf16_lossy(&units[..c]).len());
    Preedit::new(text, cursor)
}

unsafe fn composition(hwnd: HWND) -> Preedit {
    let himc = ImmGetContext(hwnd);
    if himc.is_invalid() {
        return Preedit::default();
    }
    let bytes = ImmGetCompositionStringW(himc, GCS_COMPSTR, None, 0);
    let mut units = vec![0u16; usize::try_from(bytes).unwrap_or(0) / 2];
    let got = if units.is_empty() { 0 } else { ImmGetCompositionStringW(himc, GCS_COMPSTR, Some(units.as_mut_ptr() as _), (units.len() * 2) as u32) };
    units.truncate(usize::try_from(got).unwrap_or(0) / 2);
    let cursor = ImmGetCompositionStringW(himc, GCS_CURSORPOS, None, 0);
    let _ = ImmReleaseContext(hwnd, himc);
    preedit_from_utf16(&units, cursor)
}

fn is_caption_button(wp: WPARAM) -> bool {
    matches!(wp.0 as u32, HTMINBUTTON | HTMAXBUTTON | HTCLOSE)
}

unsafe extern "system" fn wndproc<H: WindowHandler<Win32>>(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp.0 as *const CREATESTRUCTW);
        let state = cs.lpCreateParams as *const Node<H>;
        (*state).window.hwnd.set(hwnd);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Node<H>;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    Rc::increment_strong_count(ptr);
    let node = Rc::from_raw(ptr);
    match handle(&node, hwnd, msg, wp, lp) {
        Some(r) => r,
        None => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn utf16_char(w: &Win32Window, u: u16) -> Option<char> {
    if (0xD800..0xDC00).contains(&u) {
        w.high_surrogate.set(u);
        None
    } else if (0xDC00..0xE000).contains(&u) {
        let hi = w.high_surrogate.replace(0);
        char::decode_utf16([hi, u]).next().and_then(Result::ok)
    } else {
        char::from_u32(u as u32)
    }
}

unsafe fn handle<H: WindowHandler<Win32>>(s: &Node<H>, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    let w = &s.window;
    let client_xy = || {
        let p = lparam_point(lp);
        (p.x as f32, p.y as f32)
    };
    let screen_xy = || {
        let mut p = lparam_point(lp);
        let _ = ScreenToClient(hwnd, &mut p);
        (p.x as f32, p.y as f32)
    };
    match msg {
        WM_NCCALCSIZE if wp.0 != 0 => {
            let p = &mut *(lp.0 as *mut NCCALCSIZE_PARAMS);
            let top = p.rgrc[0].top;
            let r = DefWindowProcW(hwnd, msg, wp, lp);
            p.rgrc[0].top = top;
            if IsZoomed(hwnd).as_bool() && !w.full() {
                p.rgrc[0].top += frame_thickness(w.dpi.get());
            }
            Some(r)
        }
        WM_NCHITTEST => {
            let def = DefWindowProcW(hwnd, msg, wp, lp);
            if def.0 as u32 != HTCLIENT {
                return Some(def);
            }
            let (x, y) = screen_xy();
            if !IsZoomed(hwnd).as_bool() && !w.full() && y < frame_thickness(w.dpi.get()) as f32 / 2.0 {
                return Some(LRESULT(HTTOP as isize));
            }
            let ht = match s.hit_test(x, y) {
                HitArea::Client => HTCLIENT,
                HitArea::Caption => HTCAPTION,
                HitArea::Minimize => HTMINBUTTON,
                HitArea::Maximize => HTMAXBUTTON,
                HitArea::Close => HTCLOSE,
            };
            Some(LRESULT(ht as isize))
        }
        WM_NCMOUSEMOVE => {
            if !w.nc_tracking.get() {
                let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE | TME_NONCLIENT, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut t);
                w.nc_tracking.set(true);
            }
            let (x, y) = screen_xy();
            s.send(Event::MouseMove { x, y });
            None
        }
        WM_NCMOUSELEAVE => {
            w.nc_tracking.set(false);
            s.send(Event::MouseLeave);
            None
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if is_caption_button(wp) => {
            let (x, y) = screen_xy();
            s.send(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
            Some(LRESULT(0))
        }
        WM_NCLBUTTONUP if is_caption_button(wp) => {
            let (x, y) = screen_xy();
            s.send(Event::MouseUp { x, y, button: MouseButton::Left });
            Some(LRESULT(0))
        }
        WM_NCMBUTTONDOWN | WM_NCMBUTTONDBLCLK if wp.0 as u32 == HTCAPTION => {
            let (x, y) = screen_xy();
            s.send(Event::MouseDown { x, y, button: MouseButton::Middle, clicks: 1 });
            Some(LRESULT(0))
        }
        WM_NCMBUTTONUP if wp.0 as u32 == HTCAPTION => {
            let (x, y) = screen_xy();
            s.send(Event::MouseUp { x, y, button: MouseButton::Middle });
            Some(LRESULT(0))
        }
        WM_MOUSEMOVE => {
            if !w.tracking.get() {
                let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut t);
                w.tracking.set(true);
            }
            let (x, y) = client_xy();
            s.send(Event::MouseMove { x, y });
            Some(LRESULT(0))
        }
        WM_MOUSELEAVE => {
            w.tracking.set(false);
            s.send(Event::MouseLeave);
            Some(LRESULT(0))
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_RBUTTONDBLCLK | WM_MBUTTONDOWN | WM_MBUTTONDBLCLK | WM_XBUTTONDOWN | WM_XBUTTONDBLCLK => {
            let button = match msg {
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => MouseButton::Left,
                WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => MouseButton::Right,
                WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => MouseButton::Middle,
                _ if (wp.0 >> 16) & 0xFFFF == XBUTTON1 as usize => MouseButton::Back,
                _ => MouseButton::Forward,
            };
            let clicks = if matches!(msg, WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK | WM_XBUTTONDBLCLK) { 2 } else { 1 };
            SetCapture(hwnd);
            let (x, y) = client_xy();
            s.send(Event::Modifiers(mods()));
            s.send(Event::MouseDown { x, y, button, clicks });
            Some(LRESULT(if matches!(msg, WM_XBUTTONDOWN | WM_XBUTTONDBLCLK) { 1 } else { 0 }))
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP => {
            let button = match msg {
                WM_LBUTTONUP => MouseButton::Left,
                WM_RBUTTONUP => MouseButton::Right,
                WM_MBUTTONUP => MouseButton::Middle,
                _ if (wp.0 >> 16) & 0xFFFF == XBUTTON1 as usize => MouseButton::Back,
                _ => MouseButton::Forward,
            };
            const ANY: usize = 0x0001 | 0x0002 | 0x0010 | 0x0020 | 0x0040;
            if wp.0 & ANY == 0 {
                let _ = ReleaseCapture();
            }
            let (x, y) = client_xy();
            s.send(Event::MouseUp { x, y, button });
            Some(LRESULT(if msg == WM_XBUTTONUP { 1 } else { 0 }))
        }
        WM_DROPFILES => {
            use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};
            let drop = HDROP(wp.0 as _);
            let count = DragQueryFileW(drop, u32::MAX, None);
            let mut paths = Vec::new();
            for i in 0..count {
                let len = DragQueryFileW(drop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                paths.push(String::from_utf16_lossy(&buf[..got]));
            }
            DragFinish(drop);
            if !paths.is_empty() {
                s.send(Event::DroppedFiles(paths));
            }
            Some(LRESULT(0))
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32 / WHEEL_DELTA as f32;
            s.send(if msg == WM_MOUSEWHEEL { Event::Wheel { dx: 0.0, dy: delta } } else { Event::Wheel { dx: delta, dy: 0.0 } });
            Some(LRESULT(0))
        }
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
            let vk = wp.0 as u16;
            let sys = matches!(msg, WM_SYSKEYDOWN | WM_SYSKEYUP);
            if sys && vk == VK_F4.0 {
                return None;
            }
            let pressed = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
            s.send(Event::Key { key: map_key(vk), mods: mods(), pressed });
            Some(LRESULT(0))
        }
        WM_CHAR => {
            if let Some(c) = utf16_char(w, wp.0 as u16) {
                s.send(Event::Char(c));
            }
            Some(LRESULT(0))
        }
        WM_SYSCHAR => {
            if let Some(c) = utf16_char(w, wp.0 as u16) {
                s.send(Event::AltChar(c));
            }
            Some(LRESULT(0))
        }
        WM_SIZE => {
            if wp.0 as u32 != SIZE_MINIMIZED && update_size(w) {
                let (width, height) = w.size.get();
                s.send(Event::Resized { width, height });
                s.redraw_now();
            }
            Some(LRESULT(0))
        }
        WM_DPICHANGED => {
            let dpi = ((wp.0 >> 16) & 0xFFFF) as u32;
            w.dpi.set(dpi);
            s.send(Event::ScaleChanged(dpi as f32 / 96.0));
            if !IsZoomed(hwnd).as_bool() {
                let r = within_monitor(*(lp.0 as *const RECT));
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            Some(LRESULT(0))
        }
        WM_GETMINMAXINFO => {
            let mmi = &mut *(lp.0 as *mut MINMAXINFO);
            let sc = w.dpi.get() as f32 / 96.0;
            mmi.ptMinTrackSize = POINT { x: (360.0 * sc) as i32, y: (200.0 * sc) as i32 };
            Some(LRESULT(0))
        }
        WM_PAINT => {
            let _ = ValidateRect(Some(hwnd), None);
            s.redraw_now();
            Some(LRESULT(0))
        }
        WM_ENTERIDLE => {
            if w.redraw.get() {
                s.redraw_now();
            }
            None
        }
        WM_ERASEBKGND => Some(LRESULT(1)),
        WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
            SetCursor(Some(load_cursor(w.cursor.get())));
            Some(LRESULT(1))
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            if msg == WM_SETFOCUS {
                s.focused_at.set(Some(std::time::Instant::now()));
            }
            s.send(Event::Focus(msg == WM_SETFOCUS));
            None
        }
        WM_SETTINGCHANGE => {
            if lp.0 != 0 && PCWSTR(lp.0 as *const u16).to_string().is_ok_and(|v| v == "ImmersiveColorSet") {
                s.send(Event::ThemeChanged { dark: crate::dark_mode() });
            }
            None
        }
        WM_IME_SETCONTEXT => Some(DefWindowProcW(hwnd, msg, wp, LPARAM(lp.0 & !(ISC_SHOWUICOMPOSITIONWINDOW as isize)))),
        WM_IME_STARTCOMPOSITION => {
            let a = w.ime_area.get();
            let himc = ImmGetContext(hwnd);
            let cf = COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: POINT { x: a.x as i32, y: a.y as i32 }, ..Default::default() };
            let _ = ImmSetCompositionWindow(himc, &cf);
            let area = RECT { left: a.x as i32, top: a.y as i32, right: a.right() as i32, bottom: a.bottom() as i32 };
            let cand = CANDIDATEFORM { dwIndex: 0, dwStyle: CFS_EXCLUDE, ptCurrentPos: POINT { x: a.x as i32, y: a.bottom() as i32 }, rcArea: area };
            let _ = ImmSetCandidateWindow(himc, &cand);
            let _ = ImmReleaseContext(hwnd, himc);
            Some(LRESULT(0))
        }
        WM_IME_COMPOSITION => {
            let flags = IME_COMPOSITION_STRING(lp.0 as u32);
            let committed = (flags.0 & GCS_RESULTSTR.0) != 0;
            let r = if committed { DefWindowProcW(hwnd, msg, wp, lp) } else { LRESULT(0) };
            let preedit = if (flags.0 & GCS_COMPSTR.0) != 0 { composition(hwnd) } else { Preedit::default() };
            if committed || (flags.0 & GCS_COMPSTR.0) != 0 {
                s.send(Event::Preedit(preedit));
            }
            Some(r)
        }
        WM_IME_ENDCOMPOSITION => {
            s.send(Event::Preedit(Preedit::default()));
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            s.send(Event::CloseRequested);
            Some(LRESULT(0))
        }
        WM_DESTROY_REQUEST => {
            let _ = DestroyWindow(hwnd);
            Some(LRESULT(0))
        }
        WM_ENDSESSION => {
            if wp.0 != 0 {
                s.send(Event::SessionEnding);
                let _ = DestroyWindow(hwnd);
            }
            Some(LRESULT(0))
        }
        WM_DESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            s.shared().destroyed(hwnd);
            Some(LRESULT(0))
        }
        WM_TIMER if wp.0 == TIMER_ID => {
            let _ = KillTimer(Some(hwnd), TIMER_ID);
            s.send(Event::Timer);
            Some(LRESULT(0))
        }
        _ => None,
    }
}

thread_local! {
    static TASKBAR: Option<windows::Win32::UI::Shell::ITaskbarList3> = unsafe {
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let list: Option<windows::Win32::UI::Shell::ITaskbarList3> = CoCreateInstance(&windows::Win32::UI::Shell::TaskbarList, None, CLSCTX_INPROC_SERVER).ok();
        list.filter(|l| l.HrInit().is_ok())
    };
}

fn taskbar() -> Option<windows::Win32::UI::Shell::ITaskbarList3> {
    TASKBAR.with(Clone::clone)
}

#[cfg(test)]
#[path = "../test/window.rs"]
mod tests;
