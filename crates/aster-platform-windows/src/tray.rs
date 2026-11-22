use aster_core::notifications::Notification;
use aster_platform::WindowId;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const CALLBACK: u32 = WM_APP + 5;
const ICON_ID: u32 = 1;
const KEY_SELECT: u32 = NIN_SELECT + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Show,
    NewWindow,
    Quit,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action<T> {
    Show(T),
    Open,
    Close(T),
    Exit,
}

impl Command {
    pub fn resolve<T>(self, recent: Option<T>) -> Action<T> {
        match (self, recent) {
            (Self::Quit, None) => Action::Exit,
            (Self::Quit, Some(window)) => Action::Close(window),
            (Self::Show, Some(window)) => Action::Show(window),
            _ => Action::Open,
        }
    }
}

pub struct Tray {
    data: NOTIFYICONDATAW,
    enabled: Cell<bool>,
    pub installed: Cell<bool>,
    balloons: RefCell<BTreeMap<u16, Balloon>>,
    next_balloon: Cell<u16>,
}

struct Balloon {
    window: WindowId,
    tab: u64,
    data: NOTIFYICONDATAW,
}

fn copy_text(text: &str, dest: &mut [u16]) {
    let max = dest.len() - 1;
    let mut at = 0;
    for c in text.chars().filter(|c| !c.is_control()) {
        let end = at + c.len_utf16();
        if end > max { break; }
        c.encode_utf16(&mut dest[at..end]);
        at = end;
    }
    dest[at] = 0;
}

impl Tray {
    pub fn new(hub: HWND, instance: HINSTANCE) -> Self {
        let icon = unsafe { LoadIconW(Some(instance), PCWSTR(1 as _)).or_else(|_| LoadIconW(None, IDI_APPLICATION)) };
        let mut tip = [0u16; 128];
        for (unit, letter) in tip.iter_mut().zip("Aster".encode_utf16()) {
            *unit = letter;
        }
        let data = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hub, uID: ICON_ID, uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: CALLBACK, hIcon: icon.unwrap_or_default(), szTip: tip, Anonymous: NOTIFYICONDATAW_0 { uVersion: NOTIFYICON_VERSION_4 }, ..Default::default() };
        Self { data, enabled: Cell::new(false), installed: Cell::new(false), balloons: RefCell::default(), next_balloon: Cell::new(2) }
    }

    pub fn balloon(&self, window: WindowId, notification: Notification) {
        let mut balloons = self.balloons.borrow_mut();
        if balloons.len() >= 64 {
            if let Some((_, balloon)) = balloons.pop_first() {
                unsafe { let _ = Shell_NotifyIconW(NIM_DELETE, &balloon.data); }
            }
        }
        let mut id = self.next_balloon.get();
        while balloons.contains_key(&id) {
            id = id.wrapping_add(1).max(2);
        }
        self.next_balloon.set(id.wrapping_add(1).max(2));
        let mut data = NOTIFYICONDATAW { uID: u32::from(id), ..self.data };
        unsafe {
            if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
                return;
            }
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
            data.uFlags = NIF_INFO;
            data.dwInfoFlags = NIIF_INFO;
            copy_text(&notification.title, &mut data.szInfoTitle);
            copy_text(&notification.body, &mut data.szInfo);
            if Shell_NotifyIconW(NIM_MODIFY, &data).as_bool() {
                balloons.insert(id, Balloon { window, tab: notification.tab, data });
            } else {
                let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            }
        }
    }

    pub fn balloon_click(&self, wp: WPARAM, lp: LPARAM) -> Option<(WindowId, u64)> {
        let event = lp.0 as u32 & 0xffff;
        if !matches!(event, NIN_BALLOONHIDE | NIN_BALLOONTIMEOUT | NIN_BALLOONUSERCLICK) {
            return None;
        }
        let id = (lp.0 as u32 >> 16) as u16;
        let balloon = self.balloons.borrow_mut().remove(&if id == 0 { wp.0 as u16 } else { id })?;
        unsafe { let _ = Shell_NotifyIconW(NIM_DELETE, &balloon.data); }
        (event == NIN_BALLOONUSERCLICK).then_some((balloon.window, balloon.tab))
    }

    pub fn set_enabled(&self, enabled: bool) {
        if self.enabled.replace(enabled) == enabled {
            return;
        }
        unsafe {
            if enabled {
                let added = Shell_NotifyIconW(NIM_ADD, &self.data).as_bool();
                self.installed.set(added);
                if added {
                    let _ = Shell_NotifyIconW(NIM_SETVERSION, &self.data);
                } else {
                    tracing::warn!("could not add the notification area icon");
                }
            } else if self.installed.replace(false) {
                let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
            }
        }
    }

    pub fn taskbar_created(&self) {
        self.balloons.borrow_mut().clear();
        self.installed.set(false);
        let enabled = self.enabled.replace(false);
        self.set_enabled(enabled);
    }

    pub fn command(&self, wp: WPARAM, lp: LPARAM) -> Option<Command> {
        let id = lp.0 as u32 >> 16;
        if !self.installed.get() || (id != 0 && id != ICON_ID) {
            return None;
        }
        match lp.0 as u32 & 0xffff {
            NIN_SELECT | KEY_SELECT => Some(Command::Show),
            WM_CONTEXTMENU => self.menu(POINT { x: wp.0 as i16 as i32, y: (wp.0 >> 16) as i16 as i32 }),
            WM_LBUTTONUP if lp.0 as u32 == WM_LBUTTONUP => Some(Command::Show),
            WM_RBUTTONUP if lp.0 as u32 == WM_RBUTTONUP => {
                let mut pos = POINT::default();
                unsafe { GetCursorPos(&mut pos) }.ok()?;
                self.menu(pos)
            }
            _ => None,
        }
    }

    fn menu(&self, pos: POINT) -> Option<Command> {
        unsafe {
            let menu = CreatePopupMenu().ok()?;
            let labels = aster_core::settings::tray_menu_labels();
            let result = (|| {
                for (i, label) in labels.into_iter().enumerate() {
                    let text: Vec<_> = label.encode_utf16().chain([0]).collect();
                    AppendMenuW(menu, MF_STRING, i + 1, PCWSTR(text.as_ptr())).ok()?;
                }
                let _ = SetForegroundWindow(self.data.hWnd);
                match TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pos.x, pos.y, None, self.data.hWnd, None).0 {
                    1 => Some(Command::NewWindow),
                    2 => Some(Command::Quit),
                    _ => None,
                }
            })();
            let _ = DestroyMenu(menu);
            let _ = PostMessageW(Some(self.data.hWnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = Shell_NotifyIconW(NIM_SETFOCUS, &self.data);
            result
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.set_enabled(false);
        for balloon in self.balloons.get_mut().values() {
            unsafe { let _ = Shell_NotifyIconW(NIM_DELETE, &balloon.data); }
        }
    }
}
