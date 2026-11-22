use std::process::Command;

const SCHEMA: &str = "org.gnome.desktop.interface";
const KEY: &str = "enable-animations";

pub(crate) fn read() -> bool {
    let portal = [
        "call", "--session", "--timeout", "1",
        "--dest", "org.freedesktop.portal.Desktop",
        "--object-path", "/org/freedesktop/portal/desktop",
        "--method", "org.freedesktop.portal.Settings.Read",
        SCHEMA, KEY,
    ];
    query("gdbus", &portal).or_else(|| query("gsettings", &["get", SCHEMA, KEY])).unwrap_or(true)
}

fn query(program: &str, args: &[&str]) -> Option<bool> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse(std::str::from_utf8(&output.stdout).ok()?)
}

fn parse(output: &str) -> Option<bool> {
    let mut value = output.trim();
    if let Some(tuple) = value.strip_prefix('(') {
        value = tuple.strip_suffix(')')?.trim_end().strip_suffix(',')?.trim();
    }
    while let Some(variant) = value.strip_prefix('<') {
        value = variant.strip_suffix('>')?.trim();
    }
    value.parse().ok()
}

#[cfg(test)]
#[path = "../test/animations.rs"]
mod tests;
