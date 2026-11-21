use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};

const RUN: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const ASTER: PCWSTR = w!("Aster");

fn run_value(executable: &Path, args: &[&str]) -> Vec<u16> {
    let quote = u16::from(b'"');
    let space = u16::from(b' ');
    let mut value: Vec<u16> = std::iter::once(quote).chain(executable.as_os_str().encode_wide()).chain([quote]).collect();
    for arg in args {
        value.push(space);
        value.extend(super::conpty::quote_arg(arg).encode_utf16());
    }
    value.push(0);
    value
}

fn status(result: WIN32_ERROR) -> io::Result<()> {
    result.ok().map_err(|_| io::Error::from_raw_os_error(result.0 as i32))
}

pub fn set(enabled: bool, args: &[&str]) -> io::Result<()> {
    if !enabled {
        let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, ASTER) };
        return match result {
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(()),
            _ => status(result),
        };
    }
    let value = run_value(&std::env::current_exe()?, args);
    let size = std::mem::size_of_val(value.as_slice()) as u32;
    status(unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, RUN, ASTER, REG_SZ.0, Some(value.as_ptr().cast()), size) })
}

#[cfg(test)]
#[path = "../test/autostart.rs"]
mod tests;
