use crate::err;
use aster_platform::{PlatformError, Pty, SpawnRequest, Spawned};
use std::ffi::c_void;
use std::fs::File;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::sync::{Arc, Mutex};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Console::{ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

const INHERIT_CURSOR: u32 = 0x1;
const RESIZE_QUIRK: u32 = 0x2;

struct Inner {
    pcon: Mutex<Option<HPCON>>,
    process: HANDLE,
    pid: u32,
}

unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl Inner {
    fn close_console(&self) {
        if let Some(pcon) = self.pcon.lock().ok().and_then(|mut p| p.take()) {
            unsafe { ClosePseudoConsole(pcon) };
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.close_console();
        unsafe {
            let _ = CloseHandle(self.process);
        }
    }
}

struct ConPty(Arc<Inner>);

impl Pty for ConPty {
    fn resize(&self, cols: u16, rows: u16) {
        if let Ok(p) = self.0.pcon.lock() {
            if let Some(pcon) = *p {
                let _ = unsafe { ResizePseudoConsole(pcon, COORD { X: cols.max(1) as i16, Y: rows.max(1) as i16 }) };
            }
        }
    }

    fn kill(&self) {
        unsafe {
            let _ = TerminateProcess(self.0.process, 1);
        }
    }

    fn pid(&self) -> u32 {
        self.0.pid
    }
}

pub fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '"']) {
        return arg.to_owned();
    }
    let mut out = String::from('"');
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', slashes));
                out.push(c);
                slashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

pub fn command_line(command: &str, args: &[String]) -> String {
    std::iter::once(quote_arg(command)).chain(args.iter().map(|a| quote_arg(a))).collect::<Vec<_>>().join(" ")
}

fn environment(overrides: &[(String, String)], local: Option<&aster_core::environment::LocalEnvironment>) -> Vec<u16> {
    let mut inherited: aster_core::environment::Variables = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect();
    if local.is_some_and(|spec| spec.refresh) {
        inherited = crate::environment::refresh(inherited);
    }
    inherited.retain(|key, _| !key.eq_ignore_ascii_case("TERM_PROGRAM") && !key.eq_ignore_ascii_case("TERM_PROGRAM_VERSION"));
    let vars = if let Some(spec) = local {
        spec.apply(inherited, overrides, true)
    } else {
        for (key, value) in overrides {
            inherited.retain(|name, _| !name.eq_ignore_ascii_case(key));
            inherited.insert(key.clone(), value.clone());
        }
        inherited
    };
    let mut vars: Vec<_> = vars.into_iter().collect();
    vars.sort_by_key(|(key, _)| key.to_uppercase());
    let mut block: Vec<u16> = Vec::new();
    for (key, value) in vars {
        block.extend(format!("{key}={value}").encode_utf16());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

fn pipe() -> Result<(HANDLE, HANDLE), PlatformError> {
    let (mut r, mut w) = (HANDLE::default(), HANDLE::default());
    unsafe { CreatePipe(&mut r, &mut w, None, 0).map_err(err)? };
    Ok((r, w))
}

unsafe fn file(h: HANDLE) -> File {
    File::from_raw_handle(h.0 as RawHandle)
}

pub fn spawn(req: &SpawnRequest) -> Result<Spawned, PlatformError> {
    unsafe {
        let (in_read, in_write) = pipe()?;
        let (out_read, out_write) = pipe()?;
        let size = COORD { X: req.cols.max(1) as i16, Y: req.rows.max(1) as i16 };
        let pcon = CreatePseudoConsole(size, in_read, out_write, INHERIT_CURSOR | RESIZE_QUIRK);
        let _ = CloseHandle(in_read);
        let _ = CloseHandle(out_write);
        let pcon = match pcon {
            Ok(p) => p,
            Err(e) => {
                let _ = CloseHandle(in_write);
                let _ = CloseHandle(out_read);
                return Err(err(e));
            }
        };
        let startup = crate::shells::startup(req);
        let launched = launch(startup.as_ref().map_or(req, |(request, _)| request), pcon);
        let (process, pid) = match launched {
            Ok(v) => v,
            Err(e) => {
                ClosePseudoConsole(pcon);
                let _ = CloseHandle(in_write);
                let _ = CloseHandle(out_read);
                return Err(e);
            }
        };
        let inner = Arc::new(Inner { pcon: Mutex::new(Some(pcon)), process, pid });
        let waiter = inner.clone();
        let wait = Box::new(move || {
            let _startup = startup;
            let code = if WaitForSingleObject(waiter.process, INFINITE) == WAIT_OBJECT_0 {
                let mut code = 0u32;
                GetExitCodeProcess(waiter.process, &mut code).ok().map(|_| code)
            } else {
                None
            };
            waiter.close_console();
            code
        });
        Ok(Spawned { pty: Box::new(ConPty(inner)), reader: Box::new(file(out_read)), writer: Box::new(file(in_write)), raw: None, wait })
    }
}

unsafe fn launch(req: &SpawnRequest, pcon: HPCON) -> Result<(HANDLE, u32), PlatformError> {
    let mut size = 0usize;
    let _ = InitializeProcThreadAttributeList(None, 1, None, &mut size);
    let mut attrs = vec![0u8; size];
    let list = LPPROC_THREAD_ATTRIBUTE_LIST(attrs.as_mut_ptr() as *mut c_void);
    InitializeProcThreadAttributeList(Some(list), 1, None, &mut size).map_err(err)?;
    let result = (|| {
        UpdateProcThreadAttribute(list, 0, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize, Some(pcon.0 as *const c_void), size_of::<HPCON>(), None, None).map_err(err)?;
        let mut si = STARTUPINFOEXW::default();
        si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.lpAttributeList = list;
        let mut cmd: Vec<u16> = command_line(&req.command, &req.args).encode_utf16().chain([0]).collect();
        let env = environment(&req.env, req.local_environment.as_deref());
        let cwd: Option<Vec<u16>> = req.cwd.as_ref().filter(|p| p.is_dir()).map(|p| p.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect());
        let mut pi = PROCESS_INFORMATION::default();
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            Some(env.as_ptr() as *const c_void),
            cwd.as_ref().map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr())),
            &si.StartupInfo,
            &mut pi,
        )
        .map_err(|e| PlatformError::new(e.code().0 as i64, format!("cannot start {}: {}", req.command, e.message())))?;
        let _ = CloseHandle(pi.hThread);
        Ok((pi.hProcess, pi.dwProcessId))
    })();
    DeleteProcThreadAttributeList(list);
    result
}

#[cfg(test)]
#[path = "../test/conpty.rs"]
mod tests;
