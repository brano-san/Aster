#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    Unix,
    PowerShell,
    Cmd,
}

pub fn shell_kind(command: &str) -> ShellKind {
    let name = command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match name.strip_suffix(".exe").unwrap_or(&name) {
        "powershell" | "pwsh" => ShellKind::PowerShell,
        "cmd" => ShellKind::Cmd,
        _ => ShellKind::Unix,
    }
}

pub fn quote_path(path: &str, kind: ShellKind) -> String {
    match kind {
        ShellKind::Unix => {
            if !path.is_empty()
                && path
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&byte))
            {
                path.to_owned()
            } else {
                format!("'{}'", path.replace('\'', r"'\''"))
            }
        }
        ShellKind::PowerShell => {
            if !path.is_empty()
                && !path.contains([
                    ' ', '\'', '"', '`', '$', '&', '(', ')', '{', '}', ';', '@', '#', ',',
                ])
            {
                path.to_owned()
            } else {
                format!("'{}'", path.replace('\'', "''"))
            }
        }
        ShellKind::Cmd => {
            if path.is_empty()
                || path.contains([
                    ' ', '&', '(', ')', '[', ']', '{', '}', '^', '=', ';', '!', '\'', '+', ',',
                    '`', '~',
                ])
            {
                format!("\"{path}\"")
            } else {
                path.to_owned()
            }
        }
    }
}

pub fn quote_paths(paths: &[String], kind: ShellKind) -> String {
    paths
        .iter()
        .map(|path| quote_path(path, kind))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn login_shell_in(cwd: Option<&str>) -> Option<String> {
    let dir = cwd.filter(|d| !d.contains('\0'))?;
    let drive_path = matches!(dir.as_bytes(), [letter, b':', b'/' | b'\\', ..] if letter.is_ascii_alphabetic());
    if !dir.starts_with('/') && !drive_path {
        return None;
    }
    Some(format!("cd {} && exec \"$SHELL\" -l", quote_path(dir, ShellKind::Unix)))
}

#[cfg(test)]
#[path = "../test/quote.rs"]
mod tests;
