use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::io::FromRawHandle;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use windows::core::HSTRING;
use windows::Win32::Foundation::{ERROR_NO_DATA, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND};
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT};

const LIMIT: u64 = 1 << 20;

fn name() -> String {
    if let Some(own) = std::env::var_os("ASTER_INSTANCE") {
        return format!(r"\\.\pipe\{}", own.to_string_lossy());
    }
    let user: String = std::env::var("USERNAME").unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let session = std::env::var("SESSIONNAME").unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
    format!(r"\\.\pipe\aster-instance-{user}-{session}")
}

fn create(first: bool) -> Option<HANDLE> {
    let flags = if first { PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE } else { PIPE_ACCESS_INBOUND };
    let handle = unsafe { CreateNamedPipeW(&HSTRING::from(name()), flags, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, PIPE_UNLIMITED_INSTANCES, 0, 64 * 1024, 0, None) };
    (handle != INVALID_HANDLE_VALUE && !handle.is_invalid()).then_some(handle)
}

pub fn listen(wake: Arc<dyn Fn() + Send + Sync>) -> Option<Receiver<String>> {
    let mut pipe = create(true)?.0 as isize;
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("aster-instance".into()).spawn(move || loop {
        let handle = HANDLE(pipe as _);
        let connected = match unsafe { ConnectNamedPipe(handle, None) } {
            Ok(()) => true,
            Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult() || e.code() == ERROR_NO_DATA.to_hresult(),
        };
        let mut file = unsafe { File::from_raw_handle(handle.0) };
        if connected {
            let mut bytes = Vec::new();
            let mut chunk = [0u8; 4096];
            while (bytes.len() as u64) < LIMIT {
                match file.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                }
            }
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if !text.is_empty() {
                if tx.send(text).is_err() {
                    return;
                }
                wake();
            }
        }
        let next = create(false);
        drop(file);
        match next {
            Some(next) => pipe = next.0 as isize,
            None => return,
        }
    });
    spawned.ok().map(|_| rx)
}

pub fn send(message: &str) -> bool {
    for _ in 0..40 {
        match OpenOptions::new().write(true).open(name()) {
            Ok(mut pipe) => return pipe.write_all(message.as_bytes()).and_then(|()| pipe.flush()).is_ok(),
            Err(e) if e.raw_os_error() == Some(231) => std::thread::sleep(std::time::Duration::from_millis(25)),
            Err(_) => return false,
        }
    }
    false
}

#[cfg(test)]
#[path = "../test/instance.rs"]
mod tests;
