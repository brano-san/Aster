use std::ffi::c_void;
use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_BASIC_INFORMATION, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

const PEB_PROCESS_PARAMETERS: usize = 0x20;
const PARAMETERS_CURRENT_DIRECTORY: usize = 0x38;
const MAX_PATH_BYTES: u16 = 32_768;

unsafe fn read<T: Default>(process: windows::Win32::Foundation::HANDLE, at: usize) -> Option<T> {
    let mut value = T::default();
    ReadProcessMemory(process, at as *const c_void, &mut value as *mut T as *mut c_void, size_of::<T>(), None).ok()?;
    Some(value)
}

pub fn current_directory(pid: u32) -> Option<String> {
    if cfg!(not(target_pointer_width = "64")) {
        return None;
    }
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()?;
        let result = (|| {
            let mut info = PROCESS_BASIC_INFORMATION::default();
            let status = NtQueryInformationProcess(process, ProcessBasicInformation, &mut info as *mut _ as *mut c_void, size_of::<PROCESS_BASIC_INFORMATION>() as u32, std::ptr::null_mut());
            if status.is_err() || info.PebBaseAddress.is_null() {
                return None;
            }
            let params: usize = read(process, info.PebBaseAddress as usize + PEB_PROCESS_PARAMETERS)?;
            let at = params + PARAMETERS_CURRENT_DIRECTORY;
            let length: u16 = read(process, at)?;
            let buffer: usize = read(process, at + 8)?;
            if length == 0 || length > MAX_PATH_BYTES || buffer == 0 {
                return None;
            }
            let mut units = vec![0u16; length as usize / 2];
            ReadProcessMemory(process, buffer as *const c_void, units.as_mut_ptr() as *mut c_void, length as usize, None).ok()?;
            let path = String::from_utf16_lossy(&units);
            let trimmed = path.trim_end_matches('\\');
            Some(if trimmed.ends_with(':') { format!("{trimmed}\\") } else { trimmed.to_owned() })
        })();
        let _ = CloseHandle(process);
        result
    }
}

#[cfg(test)]
#[path = "../test/process.rs"]
mod tests;

pub fn children(pid: u32) -> Vec<String> {
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    let mut own = String::new();
    let mut children = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return children };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if entry.th32ProcessID == pid {
                own = name;
            } else if entry.th32ParentProcessID == pid {
                children.push(name);
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    children.retain(|name| !name.eq_ignore_ascii_case(&own) && !name.eq_ignore_ascii_case("conhost.exe") && !name.eq_ignore_ascii_case("OpenConsole.exe"));
    children
}
