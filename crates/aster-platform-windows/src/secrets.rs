use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Security::Credentials::{CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC};

fn target(key: &str) -> Vec<u16> {
    format!("Aster/{key}").encode_utf16().chain([0]).collect()
}

pub fn get(key: &str) -> Option<String> {
    let name = target(key);
    let mut found: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None, &mut found).ok()?;
        let cred = &*found;
        let blob = if cred.CredentialBlob.is_null() { &[][..] } else { std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize) };
        let value = String::from_utf8(blob.to_vec()).ok();
        CredFree(found as *const _);
        value
    }
}

pub fn set(key: &str, value: &str) -> bool {
    let mut name = target(key);
    let mut user: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let mut blob = value.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(name.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user.as_mut_ptr()),
        ..Default::default()
    };
    unsafe { CredWriteW(&cred, 0).is_ok() }
}

pub fn delete(key: &str) {
    let name = target(key);
    unsafe {
        let _ = CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None);
    }
}

#[cfg(test)]
#[path = "../test/secrets.rs"]
mod tests;
