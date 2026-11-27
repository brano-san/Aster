use aster_platform::Platform;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn open<P: Platform>(target: &str, cwd: Option<&str>) -> bool {
    let plain = aster_core::app::open_target(target);
    let (path, location) = if let Some(uri) = target.strip_prefix("file://") {
        let Some(local) = local_uri(uri) else { return false };
        (local, None)
    } else {
        let mut position = target[plain.len()..].split(':').skip(1);
        let line = position.next().and_then(|s| s.parse::<u32>().ok()).filter(|&n| n > 0);
        let column = position.next().and_then(|s| s.parse::<u32>().ok()).filter(|&n| n > 0);
        (PathBuf::from(plain), line.map(|line| (line, column.unwrap_or(1))))
    };
    let Some(path) = resolve(path, cwd) else { return false };
    let Ok(real_path) = path.canonicalize() else { return false };
    let Ok(metadata) = real_path.metadata() else { return false };
    let Some(path) = display_path(&real_path) else { return false };
    if metadata.is_file() {
        let editor = std::env::var_os("ASTER_FILE_EDITOR").filter(|editor| !editor.is_empty());
        if let Some(editor) = editor {
            let mut command = Command::new(editor);
            if let Some((line, column)) = location {
                command.arg("--goto").arg(format!("{path}:{line}:{column}"));
            } else {
                command.arg(path);
            }
            return command.spawn().is_ok();
        }
        if !open_text_editor(path) {
            let Some(parent) = Path::new(path).parent().and_then(Path::to_str) else { return false };
            P::open(parent);
        }
        return true;
    }
    if metadata.is_dir() {
        P::open(path);
        return true;
    }
    false
}

fn resolve(path: PathBuf, cwd: Option<&str>) -> Option<PathBuf> {
    let expanded = if let Ok(rest) = path.strip_prefix("~") {
        PathBuf::from(std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?).join(rest)
    } else if path.is_absolute() {
        path
    } else {
        let cwd = Path::new(cwd?);
        if !cwd.is_absolute() { return None }
        cwd.join(path)
    };
    if !expanded.is_absolute() {
        return None;
    }
    #[cfg(windows)]
    if !local_windows_path(&expanded) {
        return None;
    }
    Some(expanded)
}

#[cfg(windows)]
fn local_windows_path(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_))) &&
        !path.to_string_lossy().get(2..).is_some_and(|rest| rest.contains(':'))
}

fn display_path(path: &Path) -> Option<&str> {
    let text = path.to_str()?;
    #[cfg(windows)]
    {
        let text = text.strip_prefix(r"\\?\").unwrap_or(text);
        local_windows_path(Path::new(text)).then_some(text)
    }
    #[cfg(not(windows))]
    Some(text)
}

fn local_uri(uri: &str) -> Option<PathBuf> {
    let (host, encoded) = uri.split_once('/')?;
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let mut path = vec![b'/'];
    let mut bytes = encoded.bytes();
    while let Some(b) = bytes.next() {
        path.push(if b == b'%' {
            let hi = (bytes.next()? as char).to_digit(16)?;
            let lo = (bytes.next()? as char).to_digit(16)?;
            (hi * 16 + lo) as u8
        } else { b });
    }
    let text = String::from_utf8(path).ok()?;
    let path = if cfg!(windows) { text.strip_prefix('/')? } else { &text };
    Some(PathBuf::from(path))
}

#[cfg(target_os = "linux")]
fn open_text_editor(path: &str) -> bool {
    ["gnome-text-editor", "kate", "mousepad", "gedit", "xed", "kwrite", "pluma"]
        .iter().any(|editor| Command::new(Path::new("/usr/bin").join(editor)).arg(path).spawn().is_ok())
}

#[cfg(windows)]
fn open_text_editor(path: &str) -> bool {
    std::env::var_os("SystemRoot").is_some_and(|root| {
        Command::new(PathBuf::from(root).join("System32").join("notepad.exe")).arg(path).spawn().is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_only_resolve_local_paths() {
        let (uri, expected) = if cfg!(windows) {
            ("localhost/C:/work/tree%20name/main.rs", "C:/work/tree name/main.rs")
        } else {
            ("localhost/work/tree%20name/main.rs", "/work/tree name/main.rs")
        };
        assert_eq!(local_uri(uri).as_deref(), Some(Path::new(expected)));
        assert!(local_uri("remote.example/work/main.rs").is_none());
        assert!(local_uri("/work/bad%zz").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn output_paths_reject_device_remote_and_stream_names() {
        assert!(local_windows_path(Path::new(r"C:\work\main.rs")));
        assert!(!local_windows_path(Path::new(r"\\remote\share\main.rs")));
        assert!(!local_windows_path(Path::new(r"\\?\GLOBALROOT\Device\NamedPipe\something")));
        assert!(!local_windows_path(Path::new(r"C:\work\main.rs:secret")));
        assert!(display_path(Path::new(r"\\?\UNC\remote\share\main.rs")).is_none());
        assert_eq!(display_path(Path::new(r"\\?\C:\work\main.rs")), Some(r"C:\work\main.rs"));
    }
}
