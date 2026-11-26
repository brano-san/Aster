use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use windows::core::HSTRING;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ};

#[derive(Debug, PartialEq, Eq)]
pub enum Value {
    Text(OsString),
    Dword(u32),
}

impl Value {
    pub fn text(value: impl Into<OsString>) -> Self { Self::Text(value.into()) }
}

fn status(error: WIN32_ERROR) -> io::Result<()> {
    error.ok().map_err(|_| io::Error::from_raw_os_error(error.0 as i32))
}

pub fn read_text(path: &str, name: &str) -> io::Result<Option<OsString>> {
    let (path, name) = (HSTRING::from(path), HSTRING::from(name));
    let mut len = 0;
    let result = unsafe { RegGetValueW(HKEY_CURRENT_USER, &path, &name, RRF_RT_REG_SZ, None, None, Some(&mut len)) };
    if matches!(result, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
        return Ok(None);
    }
    status(result)?;
    let mut text = vec![0u16; (len as usize).div_ceil(2)];
    status(unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &path, &name, RRF_RT_REG_SZ, None, Some(text.as_mut_ptr().cast()), Some(&mut len))
    })?;
    let end = text.iter().position(|&unit| unit == 0).unwrap_or(text.len());
    Ok(Some(OsString::from_wide(&text[..end])))
}

pub fn write(path: &str, values: &[(&str, Value)]) -> io::Result<()> {
    let path = HSTRING::from(path);
    for (name, value) in values {
        let name = HSTRING::from(*name);
        let (kind, bytes) = match value {
            Value::Text(value) => (REG_SZ, value.encode_wide().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<_>>()),
            Value::Dword(value) => (REG_DWORD, value.to_le_bytes().to_vec()),
        };
        status(unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, &path, &name, kind.0, Some(bytes.as_ptr().cast()), bytes.len() as u32) })?;
    }
    Ok(())
}

fn set_value(key: HKEY, name: Option<&str>, value: &str) -> bool {
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) };
    let name = name.map(HSTRING::from);
    unsafe { RegSetValueExW(key, name.as_ref().map_or(windows::core::PCWSTR::null(), |n| windows::core::PCWSTR(n.as_ptr())), None, REG_SZ, Some(bytes)).is_ok() }
}

fn create(path: &str) -> Option<HKEY> {
    let mut key = HKEY::default();
    let ok = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None).is_ok() };
    ok.then_some(key)
}

pub fn set(path: &str, values: &[(Option<&str>, &str)]) -> bool {
    let Some(key) = create(path) else { return false };
    let ok = values.iter().all(|&(name, value)| set_value(key, name, value));
    unsafe { let _ = RegCloseKey(key); }
    ok
}

pub fn remove(path: &str) -> bool {
    delete(path).is_ok()
}

pub fn delete(path: &str) -> io::Result<()> {
    match unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(path)) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(()),
        error => status(error),
    }
}
