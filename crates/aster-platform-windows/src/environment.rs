use aster_core::environment::{merge_registry, RegistryValue, Variables};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, REG_EXPAND_SZ, REG_SZ};

fn values(key: HKEY) -> windows::core::Result<Vec<RegistryValue>> {
    let mut entries = Vec::new();
    let mut name = vec![0u16; 16384];
    let mut value = vec![0u16; 1024];
    let mut index = 0;
    loop {
        let mut name_len = name.len() as u32;
        let mut value_bytes = (value.len() * 2) as u32;
        let mut kind = 0;
        let status = unsafe {
            RegEnumValueW(key, index, Some(PWSTR(name.as_mut_ptr())), &mut name_len, None, Some(&mut kind),
                Some(value.as_mut_ptr().cast()), Some(&mut value_bytes))
        };
        match status {
            ERROR_NO_MORE_ITEMS => return Ok(entries),
            ERROR_MORE_DATA => {
                let words = (value_bytes as usize).div_ceil(2).max(value.len() + 1);
                if words <= 32768 {
                    value.resize(words, 0);
                    continue;
                }
            }
            ERROR_SUCCESS if (kind == REG_SZ.0 || kind == REG_EXPAND_SZ.0) && name_len != 0 && value_bytes.is_multiple_of(2) => {
                let data = &value[..value_bytes as usize / 2];
                let end = data.iter().position(|&word| word == 0).unwrap_or(data.len());
                entries.push(RegistryValue {
                    name: String::from_utf16_lossy(&name[..name_len as usize]),
                    value: String::from_utf16_lossy(&data[..end]),
                    expandable: kind == REG_EXPAND_SZ.0,
                });
            }
            ERROR_SUCCESS => {}
            error => error.ok()?,
        }
        index += 1;
    }
}

fn read(root: HKEY, path: &str) -> windows::core::Result<Vec<RegistryValue>> {
    let mut key = HKEY::default();
    let status = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_QUERY_VALUE, &mut key) };
    if matches!(status, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
        return Ok(Vec::new());
    }
    status.ok()?;
    let result = values(key);
    unsafe { let _ = RegCloseKey(key); }
    result
}

pub fn refresh(process: Variables) -> Variables {
    let read = |root, path| read(root, path).unwrap_or_else(|error| {
        tracing::warn!("could not refresh Windows environment: {error}");
        Vec::new()
    });
    let machine = read(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment");
    let user = read(HKEY_CURRENT_USER, "Environment");
    merge_registry(process, &machine, &user)
}
