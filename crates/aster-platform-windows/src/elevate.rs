use aster_platform::host::{self, Reader, Writer};
use aster_platform::{PlatformError, SpawnRequest, Spawned};
use std::fs::{File, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::os::windows::io::FromRawHandle;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND, PIPE_ACCESS_OUTBOUND};
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(120);

fn pipe_name(base: &str, side: &str) -> String {
    format!(r"\\.\pipe\{base}-{side}")
}

fn token() -> String {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    let a = h.finish();
    let mut h2 = std::collections::hash_map::RandomState::new().build_hasher();
    h2.write_u64(a ^ u64::from(std::process::id()));
    format!("{a:016x}{:016x}", h2.finish())
}

struct Pipe(HANDLE);

unsafe impl Send for Pipe {}

impl Pipe {
    fn create(name: &str, inbound: bool) -> Result<Self, PlatformError> {
        let access = if inbound { PIPE_ACCESS_INBOUND } else { PIPE_ACCESS_OUTBOUND };
        let handle = unsafe { CreateNamedPipeW(&HSTRING::from(name), access | FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 64 * 1024, 64 * 1024, 0, None) };
        if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
            return Err(PlatformError::new(-1, format!("cannot create {name}: {:?}", unsafe { GetLastError() })));
        }
        Ok(Self(handle))
    }

    fn connect(&self) -> bool {
        match unsafe { ConnectNamedPipe(self.0, None) } {
            Ok(()) => true,
            Err(_) => (unsafe { GetLastError() }) == ERROR_PIPE_CONNECTED,
        }
    }

    fn into_file(self) -> File {
        let file = unsafe { File::from_raw_handle(self.0 .0) };
        std::mem::forget(self);
        file
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn hosted(req: &SpawnRequest, launch: impl FnOnce(&str, &str) -> Result<(), PlatformError>) -> Result<Spawned, PlatformError> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let base = format!("aster-host-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let token = token();
    let to_client = Pipe::create(&pipe_name(&base, "c"), true)?;
    let to_host = Pipe::create(&pipe_name(&base, "h"), false)?;
    launch(&base, &token)?;
    let (done, connected) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("aster-host-connect".into())
        .spawn(move || {
            let ok = to_client.connect() && to_host.connect();
            let _ = done.send(ok.then_some((to_client, to_host)));
        })
        .map_err(|e| PlatformError::new(-1, e.to_string()))?;
    let pipes = match connected.recv_timeout(CONNECT_TIMEOUT) {
        Ok(Some(pipes)) => pipes,
        Ok(None) => return Err(PlatformError::new(-1, "the terminal host could not connect")),
        Err(_) => {
            let _ = OpenOptions::new().write(true).open(pipe_name(&base, "c"));
            let _ = OpenOptions::new().read(true).open(pipe_name(&base, "h"));
            return Err(PlatformError::new(-1, "the terminal host did not start in time"));
        }
    };
    let reader: Reader = Box::new(pipes.0.into_file());
    let writer: Writer = Box::new(pipes.1.into_file());
    host::client(reader, writer, &token, req)
}

pub fn connect(base: &str) -> Result<(Reader, Writer), PlatformError> {
    let fail = |side: &str, e: std::io::Error| PlatformError::new(-1, format!("{}: {e}", pipe_name(base, side)));
    let writer = OpenOptions::new().write(true).open(pipe_name(base, "c")).map_err(|e| fail("c", e))?;
    let reader = OpenOptions::new().read(true).open(pipe_name(base, "h")).map_err(|e| fail("h", e))?;
    Ok((Box::new(reader), Box::new(writer)))
}

fn run_as_administrator(base: &str, token: &str) -> Result<(), PlatformError> {
    let exe = std::env::current_exe().map_err(|e| PlatformError::new(-1, e.to_string()))?;
    let file = HSTRING::from(exe.as_os_str());
    let params = HSTRING::from(format!("--host {base} {token}"));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    match unsafe { ShellExecuteExW(&mut info) } {
        Ok(()) => {
            if !info.hProcess.is_invalid() {
                unsafe {
                    let _ = CloseHandle(info.hProcess);
                }
            }
            Ok(())
        }
        Err(_) if (unsafe { GetLastError() }) == ERROR_CANCELLED => Err(PlatformError::new(1223, "The administrator request was declined")),
        Err(e) => Err(PlatformError::new(-1, format!("cannot start the administrator host: {e}"))),
    }
}

pub fn spawn_elevated(req: &SpawnRequest) -> Result<Spawned, PlatformError> {
    hosted(req, run_as_administrator)
}

#[cfg(test)]
#[path = "../test/elevate.rs"]
mod tests;
