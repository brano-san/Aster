use windows::core::HSTRING;
use windows::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ};

const PLACES: [(&str, &str); 3] = [(r"Software\Classes\Directory\Background\shell\Aster", "%V"), (r"Software\Classes\Directory\shell\Aster", "%1"), (r"Software\Classes\Drive\shell\Aster", "%1")];

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

pub fn set(label: &str, enabled: bool) -> bool {
    if !enabled {
        for (path, _) in PLACES {
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(path));
            }
        }
        return true;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let exe = exe.to_string_lossy().into_owned();
    PLACES.iter().all(|(path, arg)| {
        let Some(shell) = create(path) else { return false };
        let ok = set_value(shell, None, label) && set_value(shell, Some("Icon"), &exe);
        unsafe {
            let _ = RegCloseKey(shell);
        }
        let Some(command) = create(&format!(r"{path}\command")) else { return false };
        let ok = ok && set_value(command, None, &format!("\"{exe}\" open \"{arg}\""));
        unsafe {
            let _ = RegCloseKey(command);
        }
        ok
    })
}

#[cfg(test)]
#[path = "../test/explorer.rs"]
mod tests;
