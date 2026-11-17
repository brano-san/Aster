use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

const LIMIT: u64 = 1 << 20;

#[derive(Clone)]
pub struct Log(Option<Arc<Mutex<File>>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(file) = &self.0 {
            if let Ok(mut f) = file.lock() {
                let _ = f.write_all(buf);
            }
        }
        let _ = io::stderr().write_all(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = &self.0 {
            if let Ok(mut f) = file.lock() {
                let _ = f.flush();
            }
        }
        Ok(())
    }
}

pub fn open(path: Option<&Path>) -> Log {
    let file = path.and_then(|path| {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::metadata(path).is_ok_and(|m| m.len() > LIMIT) {
            let _ = std::fs::rename(path, path.with_extension("log.1"));
        }
        OpenOptions::new().create(true).append(true).open(path).ok()
    });
    Log(file.map(|f| Arc::new(Mutex::new(f))))
}

#[cfg(test)]
#[path = "../test/log.rs"]
mod tests;
