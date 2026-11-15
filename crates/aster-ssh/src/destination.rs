use crate::config;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub identity_files: Vec<PathBuf>,
    pub explicit_identities: bool,
    pub proxy_jump: Vec<String>,
    pub proxy_command: Option<String>,
}

pub fn split(spec: &str) -> (Option<String>, String, Option<u16>) {
    let spec = spec.trim();
    let (user, rest) = match spec.rsplit_once('@') {
        Some((u, r)) if !u.is_empty() => (Some(u.to_owned()), r),
        _ => (None, spec),
    };
    if let Some(inner) = rest.strip_prefix('[') {
        if let Some((host, after)) = inner.split_once(']') {
            let port = after.strip_prefix(':').and_then(|p| p.parse().ok());
            return (user, host.to_owned(), port);
        }
    }
    match rest.split_once(':') {
        Some((host, port)) if !port.contains(':') => (user, host.to_owned(), port.parse().ok()),
        _ => (user, rest.to_owned(), None),
    }
}

pub struct Context<'a> {
    pub config: &'a str,
    pub ssh_dir: &'a Path,
    pub home: &'a Path,
    pub read: &'a dyn Fn(&Path) -> Option<String>,
    pub default_user: &'a str,
}

pub fn resolve(spec: &str, cx: &Context) -> Destination {
    let (user, name, port) = split(spec);
    let entry = config::resolve(cx.config, cx.ssh_dir, cx.home, cx.read, &name);
    let host = entry.host_name.map(|h| h.replace("%h", &name)).unwrap_or_else(|| name.clone());
    let explicit_identities = !entry.identity_files.is_empty();
    let identity_files = if explicit_identities {
        entry.identity_files.iter().map(PathBuf::from).collect()
    } else {
        ["id_ed25519", "id_ecdsa", "id_rsa"].iter().map(|f| cx.ssh_dir.join(f)).collect()
    };
    let proxy_jump = entry
        .proxy_jump
        .filter(|j| !j.eq_ignore_ascii_case("none"))
        .map(|j| j.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let proxy_command = entry.proxy_command.filter(|c| !c.eq_ignore_ascii_case("none"));
    Destination {
        name,
        host,
        port: port.or(entry.port).unwrap_or(22),
        user: user.or(entry.user).unwrap_or_else(|| cx.default_user.to_owned()),
        identity_files,
        explicit_identities,
        proxy_jump,
        proxy_command,
    }
}

impl Destination {
    pub fn proxy_command_line(&self) -> Option<String> {
        let template = self.proxy_command.as_ref()?;
        let mut out = String::new();
        let mut chars = template.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('h') => out.push_str(&self.host),
                Some('p') => out.push_str(&self.port.to_string()),
                Some('r') => out.push_str(&self.user),
                Some('n') => out.push_str(&self.name),
                Some('%') => out.push('%'),
                Some(other) => {
                    out.push('%');
                    out.push(other);
                }
                None => out.push('%'),
            }
        }
        Some(out)
    }
}

#[cfg(test)]
#[path = "../test/destination.rs"]
mod tests;
