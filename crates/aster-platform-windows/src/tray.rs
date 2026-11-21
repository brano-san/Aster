use std::cell::Cell;
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
        Self { data, enabled: Cell::new(false), installed: Cell::new(false) }
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
        self.installed.set(false);
        let enabled = self.enabled.replace(false);
        self.set_enabled(enabled);
    }

    pub fn command(&self, wp: WPARAM, lp: LPARAM) -> Option<Command> {
        if !self.installed.get() {
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
    }
}
