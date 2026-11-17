use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

const LIMIT: u64 = 1 << 20;

fn path() -> PathBuf {
    if let Some(own) = std::env::var_os("ASTER_INSTANCE") {
        return std::env::temp_dir().join(own);
    }
    match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()) {
        Some(dir) => dir.join("aster-instance.sock"),
        None => std::env::temp_dir().join(format!("aster-instance-{}.sock", unsafe { libc::getuid() })),
    }
}

pub fn listen(wake: Arc<dyn Fn() + Send + Sync>) -> Option<Receiver<String>> {
    let path = path();
    if UnixStream::connect(&path).is_ok() {
        return None;
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).ok()?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("aster-instance".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let mut text = String::new();
            if stream.take(LIMIT).read_to_string(&mut text).is_ok() && !text.is_empty() {
                if tx.send(text).is_err() {
                    return;
                }
                wake();
            }
        }
    });
    spawned.ok().map(|_| rx)
}

pub fn send(message: &str) -> bool {
    match UnixStream::connect(path()) {
        Ok(mut stream) => stream.write_all(message.as_bytes()).and_then(|()| stream.flush()).is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
#[path = "../test/instance.rs"]
mod tests;
