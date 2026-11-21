use crate::{desktop, thunar};
use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;

const SCRIPT: &str = "nautilus/scripts/Open in Aster";
const NEMO: &str = "nemo/actions/aster-open.nemo_action";
const SERVICE: &str = "kio/servicemenus/aster-open.desktop";
const SCRIPT_MARK: &str = ": aster-open-v1";
const DESKTOP_MARK: &str = "X-Aster-Managed=true";

pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn script(path: &str) -> String {
    let path = shell_quote(path);
    format!("#!/bin/sh\n{SCRIPT_MARK}\ncase $NAUTILUS_SCRIPT_CURRENT_URI in\n    file://*) ;;\n    *) exit 0 ;;\nesac\ndir=$PWD\nif [ \"$#\" -eq 1 ] && [ -d \"$1\" ]; then\n    dir=$1\nfi\nexec {path} open \"$dir\"\n")
}

fn nemo(path: &str, label: &str) -> String {
    let exec = desktop::value(&shell_quote(&path.replace('%', "%%")));
    let label = desktop::value(label);
    format!("[Nemo Action]\n{DESKTOP_MARK}\nName={label}\nExec={exec} open %F\nIcon-Name=aster\nSelection=s\nExtensions=dir;\nQuote=double\n")
}

fn service(path: &str, label: &str) -> String {
    let exec = desktop::exec(path, &["open"]);
    let label = desktop::value(label);
    format!("[Desktop Entry]\nType=Service\nServiceTypes=KonqPopupMenu/Plugin\nMimeType=inode/directory;\nActions=aster-open;\nX-KDE-Priority=TopLevel\n{DESKTOP_MARK}\n\n[Desktop Action aster-open]\nName={label}\nIcon=aster\nExec={exec} %f\n")
}

fn set_entry(path: &Path, marker: &str, contents: Option<String>, executable: bool) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(text) if text.lines().any(|line| line == marker) => {}
        Ok(_) if contents.is_none() => return Ok(()),
        Ok(_) => return Err(io::Error::new(ErrorKind::AlreadyExists, path.display().to_string())),
        Err(e) if e.kind() == ErrorKind::NotFound && contents.is_none() => return Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let Some(contents) = contents else { return desktop::remove(path) };
    desktop::write(path, contents.as_bytes())?;
    if executable {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(path)?.permissions();
            if permissions.mode() & 0o100 == 0 {
                permissions.set_mode(permissions.mode() | 0o100);
                fs::set_permissions(path, permissions)?;
            }
        }
    }
    Ok(())
}

pub fn set(data: &Path, config: &Path, executable: &Path, label: &str, enabled: bool) -> io::Result<()> {
    let path = if enabled { Some(desktop::executable(executable)?) } else { None };
    let script = set_entry(&data.join(SCRIPT), SCRIPT_MARK, path.map(script), true);
    let nemo = set_entry(&data.join(NEMO), DESKTOP_MARK, path.map(|path| nemo(path, label)), false);
    let service = set_entry(&data.join(SERVICE), DESKTOP_MARK, path.map(|path| service(path, label)), true);
    let xml = thunar::set(config, path.map(|path| (path, label)));
    script.and(nemo).and(service).and(xml)
}

#[cfg(test)]
#[path = "../test/explorer.rs"]
mod tests;
