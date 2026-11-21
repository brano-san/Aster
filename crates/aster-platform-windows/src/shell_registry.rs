use windows::core::HSTRING;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ};

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
    let result = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(path)) };
    matches!(result, ERROR_SUCCESS | ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND)
}
