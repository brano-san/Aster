use crate::desktop;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;

const ENTRY: &str = "autostart/aster.desktop";
const MARKER: &str = "X-Aster-Managed=true";

fn entry(executable: &str, args: &[&str]) -> String {
    let command = desktop::exec(executable, args);
    format!("[Desktop Entry]\nType=Application\nName=Aster\nExec={command}\nIcon=aster\nTerminal=false\n{MARKER}\n")
}

pub fn set(config: &Path, launch: Option<(&Path, &[&str])>) -> io::Result<()> {
    let path = config.join(ENTRY);
    let managed = match fs::read_to_string(&path) {
        Ok(text) if text.lines().any(|line| line == MARKER) => true,
        Ok(_) if launch.is_none() => return Ok(()),
        Ok(_) => return Err(io::Error::new(ErrorKind::AlreadyExists, path.display().to_string())),
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    match launch {
        Some((exe, args)) => desktop::write(&path, entry(desktop::executable(exe)?, args).as_bytes()),
        None if managed => desktop::remove(&path),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "../test/autostart.rs"]
mod tests;
