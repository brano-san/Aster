#![cfg(windows)]

mod conpty;
mod d3d;
mod dwrite;
mod shells;
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

    fn ssh_command() -> Option<String> {
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:/Windows"));
        let bundled = root.join("System32").join("OpenSSH").join("ssh.exe");
        let found = Some(bundled).filter(|p| p.is_file()).or_else(|| std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join("ssh.exe")).find(|p| p.is_file()));
        found.map(|p| p.to_string_lossy().into_owned())
    }

    fn open(target: &str) {
        let wide: Vec<u16> = target.encode_utf16().chain([0]).collect();
        unsafe {
            ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
        }
    }
}
