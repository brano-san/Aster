use aster_core::profile::Profile;
use std::path::Path;

fn login_shell() -> Option<String> {
    let from_passwd = unsafe {
        let pw = libc::getpwuid(libc::getuid());
        (!pw.is_null() && !(*pw).pw_shell.is_null()).then(|| std::ffi::CStr::from_ptr((*pw).pw_shell).to_string_lossy().into_owned())
    };
    from_passwd.filter(|s| Path::new(s).is_file()).or_else(|| std::env::var("SHELL").ok().filter(|s| Path::new(s).is_file()))
}

pub fn listed_shells(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| l.starts_with('/')) {
        let name = Path::new(line).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !out.iter().any(|s| Path::new(s).file_name().map(|n| n.to_string_lossy() == name).unwrap_or(false)) {
            out.push(line.to_owned());
        }
    }
    out
}

pub fn detect() -> Vec<Profile> {
    let mut out = Vec::new();
    let default = login_shell().unwrap_or_else(|| "/bin/sh".into());
    let name = Path::new(&default).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "sh".into());
    out.push(Profile::new("local:default", &format!("User default ({name})"), &default, &["--login"]));
    let listed = std::fs::read_to_string("/etc/shells").or_else(|_| std::fs::read_to_string("/usr/share/defaults/etc/shells")).unwrap_or_default();
    for shell in listed_shells(&listed).into_iter().filter(|s| Path::new(s).is_file()) {
        let base = Path::new(&shell).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if base == name || base == "nologin" || base == "false" {
            continue;
        }
        out.push(Profile::new(&format!("local:{base}"), &base, &shell, &["-l"]));
    }
    out
}
