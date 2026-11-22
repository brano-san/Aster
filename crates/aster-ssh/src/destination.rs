use crate::config;
use aster_proto::ssh::{AlgorithmOptions, Algorithms, SshAuth, SshOptions, SshProxy};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub identity_files: Vec<PathBuf>,
    pub explicit_identities: bool,
    pub keepalive_ms: u64,
    pub keepalive_count: usize,
    pub ready_timeout_ms: u64,
    pub skip_banner: bool,
    pub term: String,
    pub remember_cwd: bool,
    pub x11: bool,
    pub algorithms: Algorithms,
    pub proxy_jump: Vec<String>,
    pub proxy_command: Option<String>,
    pub proxy: Option<SshProxy>,
    pub auth: SshAuth,
    pub forward_agent: Option<bool>,
    pub reuse_session: Option<bool>,
    pub agent: Option<String>,
    pub forwards: Vec<crate::forward::Forward>,
    pub forward_errors: Vec<String>,
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
    pub default_term: &'a str,
}

pub fn resolve(spec: &str, cx: &Context) -> Destination {
    resolve_options(spec, cx, &SshOptions::default())
}

pub fn resolve_file(spec: &str, ssh_dir: &Path, home: &Path, default_term: &str, profile: &SshOptions) -> Destination {
    let config = std::fs::read_to_string(ssh_dir.join("config")).unwrap_or_default();
    let read = |p: &Path| std::fs::read_to_string(p).ok();
    let context = Context { config: &config, ssh_dir, home, read: &read, default_user: "", default_term };
    resolve_options(spec, &context, profile)
}

pub fn resolve_options(spec: &str, cx: &Context, profile: &SshOptions) -> Destination {
    let (user, name, port) = split(spec);
    let entry = config::resolve(cx.config, cx.ssh_dir, cx.home, cx.read, &name);
    let host = entry.host_name.map(|h| h.replace("%h", &name)).unwrap_or_else(|| name.clone());
    let mut identity_files: Vec<PathBuf> = match &profile.identity_files {
        Some(files) => files.iter().map(|f| config::expand_home(f, cx.home)).collect(),
        None => entry.identity_files.into_iter().map(PathBuf::from).collect(),
    };
    let explicit_identities = !identity_files.is_empty();
    if !explicit_identities {
        identity_files = discover_identities(cx.ssh_dir);
    }
    let proxy_jump = entry
        .proxy_jump
        .filter(|j| profile.proxy.is_none() && !j.eq_ignore_ascii_case("none"))
        .map(|j| j.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let proxy_command = match &profile.proxy {
        Some(SshProxy::Command { command }) => Some(command.clone()),
        Some(_) => None,
        None => entry.proxy_command.filter(|c| !c.eq_ignore_ascii_case("none")),
    };
    let agent = entry.identity_agent.filter(|a| a != "SSH_AUTH_SOCK");
    let mut forwards = Vec::new();
    let mut forward_errors = Vec::new();
    if let Some(rules) = &profile.forwards {
        for (index, rule) in rules.iter().enumerate() {
            match rule.resolve() {
                Ok(forward) => forwards.push(forward),
                Err(e) => forward_errors.push(format!("{} forward #{}: {e}", rule.kind.as_str(), index + 1)),
            }
        }
    } else {
        for (kind, specs) in [("local", &entry.local_forwards), ("remote", &entry.remote_forwards), ("dynamic", &entry.dynamic_forwards)] {
            for spec in specs {
                match crate::forward::parse(kind, spec) {
                    Ok(f) => forwards.push(f),
                    Err(e) => forward_errors.push(format!("{kind} forward \"{spec}\": {e}")),
                }
            }
        }
    }
    Destination {
        name,
        host,
        port: profile.port.or(port).or(entry.port).unwrap_or(22),
        user: profile.user.clone().or(user).or(entry.user).unwrap_or_else(|| cx.default_user.to_owned()),
        identity_files,
        explicit_identities,
        keepalive_ms: profile.keepalive_ms.or(entry.keepalive_ms).unwrap_or(5000),
        keepalive_count: profile.keepalive_count.or(entry.keepalive_count).unwrap_or(10),
        ready_timeout_ms: profile.ready_timeout_ms.or(entry.ready_timeout_ms).unwrap_or(20000),
        skip_banner: profile.skip_banner.or(entry.skip_banner).unwrap_or(false),
        term: profile.term.clone().or(entry.term).unwrap_or_else(|| if cx.default_term.is_empty() { "xterm-256color" } else { cx.default_term }.to_owned()),
        remember_cwd: profile.remember_cwd.unwrap_or(false),
        x11: profile.x11.or(entry.x11).unwrap_or(false),
        algorithms: resolve_algorithms(profile.algorithms.as_ref(), entry.algorithms),
        proxy_jump,
        proxy_command,
        proxy: profile.proxy.clone(),
        auth: profile.auth.unwrap_or_default(),
        forward_agent: profile.forward_agent.or(entry.forward_agent),
        reuse_session: profile.reuse_session,
        agent,
        forwards,
        forward_errors,
    }
}

fn discover_identities(directory: &Path) -> Vec<PathBuf> {
    let defaults = ["id_ed25519", "id_ecdsa", "id_rsa"];
    let mut identities: Vec<_> = defaults.iter().map(|name| directory.join(name)).filter(|file| file.is_file()).collect();
    let Ok(entries) = std::fs::read_dir(directory) else { return identities };
    let mut additional: Vec<_> = entries.filter_map(Result::ok).filter_map(|entry| {
        let name = entry.file_name().into_string().ok()?;
        if !name.starts_with("id_") || name.to_ascii_lowercase().ends_with(".pub") || defaults.contains(&name.as_str()) { return None; }
        let path = entry.path();
        path.is_file().then_some(path)
    }).collect();
    additional.sort();
    identities.extend(additional);
    identities
}

fn resolve_algorithms(profile: Option<&AlgorithmOptions>, config: AlgorithmOptions) -> Algorithms {
    Algorithms {
        kex: profile.and_then(|p| p.kex.clone()).or(config.kex).unwrap_or_default(),
        cipher: profile.and_then(|p| p.cipher.clone()).or(config.cipher).unwrap_or_default(),
        mac: profile.and_then(|p| p.mac.clone()).or(config.mac).unwrap_or_default(),
        host_key: profile.and_then(|p| p.host_key.clone()).or(config.host_key).unwrap_or_default(),
        compression: profile.and_then(|p| p.compression.clone()).or(config.compression).unwrap_or_default(),
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
