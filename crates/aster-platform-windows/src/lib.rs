#![cfg(windows)]

mod autostart;
mod conpty;
mod d3d;
mod elevate;
mod environment;
mod explorer;
mod instance;
mod jump_list;
mod process;
mod secrets;
mod dwrite;
mod shells;
mod shell_link;
mod shell_registry;
mod shortcut;
mod tray;
mod update;
mod url_scheme;
mod window;

pub use d3d::{D3dDevice, D3dRenderer};
pub use dwrite::DWriteText;
pub use window::{Win32, Win32Waker, Win32Window};

use aster_core::profile::Profile;
use aster_platform::{Platform, PlatformError, SpawnRequest, Spawned};
use std::path::PathBuf;
use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

pub(crate) fn err(e: windows::core::Error) -> PlatformError {
    PlatformError::new(e.code().0 as i64, e.message())
}

pub fn replace_file(target: &std::path::Path, replacement: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::ReplaceFileW;
    let wide = |path: &std::path::Path| path.as_os_str().encode_wide().chain([0]).collect::<Vec<_>>();
    let (target, replacement) = (wide(target), wide(replacement));
    unsafe { ReplaceFileW(PCWSTR(target.as_ptr()), PCWSTR(replacement.as_ptr()), None, Default::default(), None, None) }
        .map_err(std::io::Error::other)
}

pub(crate) fn dark_mode() -> bool {
    let mut v = 1u32;
    let mut n = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as _),
            Some(&mut n),
        )
    };
    r.is_ok() && v == 0
}

pub struct Windows;

impl Platform for Windows {
    type Windows = Win32;
    type Renderer = D3dRenderer;
    type Text = DWriteText;

    fn text() -> Result<DWriteText, PlatformError> {
        DWriteText::new()
    }

    fn font_families(monospace: bool) -> Vec<String> {
        dwrite::font_families(monospace).unwrap_or_else(|e| {
            tracing::warn!("system font collection: {e}");
            Vec::new()
        })
    }

    fn shells() -> Vec<Profile> {
        shells::detect()
    }

    fn spawn(req: &SpawnRequest) -> Result<Spawned, PlatformError> {
        conpty::spawn(req)
    }

    fn config_dir() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("Aster"))
    }

    fn home_dir() -> Option<PathBuf> {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }

    fn process_cwd(pid: u32) -> Option<String> {
        process::current_directory(pid)
    }

    fn child_processes(pid: u32) -> Vec<String> {
        process::children(pid)
    }

    fn instance_listen(wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Option<std::sync::mpsc::Receiver<String>> {
        instance::listen(wake)
    }

    fn set_explorer_menu(label: &str, enabled: bool) -> bool {
        explorer::set(label, enabled)
    }

    fn set_autostart(enabled: bool, args: &[&str]) -> std::io::Result<()> {
        autostart::set(enabled, args)
    }

    fn set_url_scheme(enabled: bool) -> bool {
        url_scheme::set(enabled)
    }

    fn set_jump_list(list: &aster_core::jump_list::JumpList) -> Result<(), PlatformError> {
        jump_list::apply(list)
    }

    fn start_menu_shortcut(change: Option<bool>) -> Result<bool, PlatformError> {
        shortcut::apply(change)
    }

    fn set_explorer_paste_menu(label: &str, enabled: bool) -> bool {
        explorer::set_paste(label, enabled)
    }

    fn instance_send(message: &str) -> bool {
        instance::send(message)
    }

    fn downloads_dir() -> Option<PathBuf> {
        use windows::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
        let known = unsafe {
            SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok().map(|p| {
                let path = p.to_string().ok();
                windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
                path
            })
        };
        known.flatten().map(PathBuf::from).or_else(|| Self::home_dir().map(|h| h.join("Downloads")))
    }

    fn secret_get(key: &str) -> Option<String> {
        secrets::get(key)
    }

    fn secret_set(key: &str, value: &str) -> bool {
        secrets::set(key, value)
    }

    fn secret_delete(key: &str) {
        secrets::delete(key)
    }

    fn can_elevate() -> bool {
        true
    }

    fn spawn_elevated(req: &SpawnRequest) -> Result<Spawned, PlatformError> {
        elevate::spawn_elevated(req)
    }

    fn connect_host(base: &str) -> Result<(aster_platform::host::Reader, aster_platform::host::Writer), PlatformError> {
        elevate::connect(base)
    }

    fn ui_language() -> String {
        use windows::Win32::Globalization::{GetUserDefaultLocaleName, GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
        let mut count = 0u32;
        let mut len = 0u32;
        let mut preferred = None;
        if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len) }.is_ok() && len > 0 {
            let mut buf = vec![0u16; len as usize];
            if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len) }.is_ok() {
                let first = buf.split(|&c| c == 0).next().unwrap_or(&[]);
                preferred = Some(String::from_utf16_lossy(first)).filter(|s| !s.is_empty());
            }
        }
        preferred.unwrap_or_else(|| {
            let mut buf = [0u16; 85];
            let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
            if n > 1 {
                String::from_utf16_lossy(&buf[..n as usize - 1])
            } else {
                "en".to_owned()
            }
        })
    }

    fn beep() -> bool {
        unsafe { windows::Win32::System::Diagnostics::Debug::MessageBeep(windows::Win32::UI::WindowsAndMessaging::MB_OK) }.is_ok()
    }

    fn ssh_command() -> Option<String> {
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:/Windows"));
        let bundled = root.join("System32").join("OpenSSH").join("ssh.exe");
        let found = Some(bundled).filter(|p| p.is_file()).or_else(|| std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join("ssh.exe")).find(|p| p.is_file()));
        found.map(|p| p.to_string_lossy().into_owned())
    }

    fn download(url: &str, out: &mut dyn std::io::Write, progress: &mut dyn FnMut(u64, Option<u64>) -> bool) -> Result<(), PlatformError> {
        update::download(url, out, progress)
    }

    fn wait_exit(pid: u32, limit: std::time::Duration) {
        update::wait_exit(pid, limit)
    }

    fn run_elevated(args: &[String]) -> Result<u32, PlatformError> {
        update::run_elevated(args)
    }

    fn delete_at_reboot(path: &std::path::Path) {
        update::delete_at_reboot(path)
    }

    fn open(target: &str) {
        let wide: Vec<u16> = target.encode_utf16().chain([0]).collect();
        unsafe {
            ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
        }
    }
}

pub fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
