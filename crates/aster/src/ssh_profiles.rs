use aster_core::profile::{Profile, SshOptions, SshProxy};
use aster_core::ssh::{parse_target, Target};
use aster_platform::Platform;
use aster_proto::ssh::{AlgorithmOptions, SshAuth};
use aster_proto::ssh_forward::{ForwardKind, ForwardRule};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

const FILE: &str = "ssh-profiles.yaml";
const FILE_PROFILE_PREFIX: &str = "file-config:";

fn hashed_id(prefix: &str, name: &str) -> String {
    let mut id = String::with_capacity(prefix.len() + 64);
    id.push_str(prefix);
    for byte in Sha256::digest(name.as_bytes()) {
        write!(id, "{byte:02x}").expect("writing to a string");
    }
    id
}

pub(crate) fn discover<P: Platform>(ssh: &str, app_dir: Option<&Path>) -> Vec<Profile> {
    let home = P::home_dir();
    let mut profiles = home
        .as_deref()
        .map_or_else(Vec::new, |home| openssh_profiles(ssh, home));
    let aliases: HashMap<String, String> = profiles
        .iter()
        .filter_map(|p| Some((hashed_id("openssh-config:", p.ssh.as_deref()?), p.id.clone())))
        .collect();
    let extra_dir = std::env::var_os("ASTER_SSH_PROFILES_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let mut seen = HashSet::new();
    for dir in app_dir
        .into_iter()
        .chain(extra_dir.as_deref())
        .filter(|dir| seen.insert(dir.to_path_buf()))
    {
        let path = dir.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_file_profiles(&text, ssh, &aliases) {
                Ok(imported) => {
                    let existing: HashSet<_> = profiles.iter().map(|p| p.id.clone()).collect();
                    profiles.extend(imported.into_iter().filter(|p| !existing.contains(&p.id)));
                }
                Err(_) => tracing::warn!("{}: invalid SSH profile list", path.display()),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!("{}: {error}", path.display()),
        }
    }
    profiles
}

fn openssh_profiles(ssh: &str, home: &Path) -> Vec<Profile> {
    let dir = home.join(".ssh");
    let Ok(text) = std::fs::read_to_string(dir.join("config")) else {
        return Vec::new();
    };
    let read = |path: &Path| std::fs::read_to_string(path).ok();
    aster_ssh::config::parse(&text, &dir, home, &read)
        .iter()
        .map(|h| aster_core::ssh::host_profile(&h.alias, ssh))
        .collect()
}

#[derive(Deserialize)]
struct FileProfile {
    name: String,
    group: Option<String>,
    color: Option<String>,
    hidden: Option<bool>,
    options: FileOptions,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileOptions {
    host: String,
    port: Option<u16>,
    user: Option<String>,
    auth: Option<String>,
    private_keys: Option<Vec<String>>,
    agent_forward: Option<bool>,
    keepalive_interval: Option<u64>,
    keepalive_count_max: Option<usize>,
    ready_timeout: Option<u64>,
    x11: Option<bool>,
    skip_banner: Option<bool>,
    term: Option<String>,
    cwd: Option<String>,
    remember_cwd: Option<bool>,
    reuse_session: Option<bool>,
    proxy_command: Option<String>,
    jump_host: Option<String>,
    socks_proxy_host: Option<String>,
    socks_proxy_port: Option<u16>,
    http_proxy_host: Option<String>,
    http_proxy_port: Option<u16>,
    forwarded_ports: Option<Vec<FileForward>>,
    algorithms: Option<FileAlgorithms>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileAlgorithms {
    kex: Option<Vec<String>>,
    cipher: Option<Vec<String>>,
    hmac: Option<Vec<String>>,
    server_host_key: Option<Vec<String>>,
    compression: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileForward {
    #[serde(rename = "type")]
    kind: String,
    host: String,
    port: u16,
    target_address: Option<String>,
    target_port: Option<u16>,
    description: Option<String>,
}

fn endpoint(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

impl FileForward {
    fn into_rule(self) -> Option<ForwardRule> {
        let kind = match self.kind.as_str() {
            "Local" => ForwardKind::Local,
            "Remote" => ForwardKind::Remote,
            "Dynamic" => ForwardKind::Dynamic,
            _ => return None,
        };
        let listen = if self.host.is_empty() {
            self.port.to_string()
        } else {
            endpoint(&self.host, self.port)
        };
        let destination = (kind != ForwardKind::Dynamic)
            .then(|| self.target_address.zip(self.target_port))
            .flatten()
            .map(|(host, port)| endpoint(&host, port));
        let rule = ForwardRule {
            kind,
            listen,
            destination,
            description: self.description.unwrap_or_default(),
        };
        rule.resolve().ok().map(|_| rule)
    }
}

fn parse_file_profiles(
    text: &str,
    ssh: &str,
    aliases: &HashMap<String, String>,
) -> Result<Vec<Profile>, serde_yaml_ng::Error> {
    let entries =
        serde_yaml_ng::from_str::<Option<Vec<serde_yaml_ng::Value>>>(text)?.unwrap_or_default();
    let mut seen = HashSet::new();
    Ok(entries
        .into_iter()
        .filter_map(|entry| serde_yaml_ng::from_value::<FileProfile>(entry).ok())
        .filter_map(|entry| entry.into_profile(ssh, aliases))
        .filter(|p| seen.insert(p.id.clone()))
        .collect())
}

impl FileProfile {
    fn into_profile(self, ssh: &str, aliases: &HashMap<String, String>) -> Option<Profile> {
        let name = self.name.trim();
        if name.is_empty() {
            return None;
        }
        let options = self.options;
        let mut target = parse_target(&options.host)?;
        target.user = options.user.filter(|user| !user.is_empty()).or(target.user);
        target.port = options.port.or(target.port);
        if target.port == Some(0)
            || target
                .user
                .as_ref()
                .is_some_and(|user| user.contains('@') || user.chars().any(char::is_whitespace))
        {
            return None;
        }

        let id = hashed_id(FILE_PROFILE_PREFIX, &self.name);
        let mut profile = Profile::new(&id, name, ssh, &[]);
        profile.ssh = Some(target.display());
        profile.icon = Some("ssh".into());
        profile.group = self.group.filter(|group| !group.trim().is_empty());
        profile.color = self.color;
        profile.hidden = self.hidden.unwrap_or(false);
        profile.cwd = options.cwd;
        let proxy = options
            .jump_host
            .filter(|jump| !jump.is_empty())
            .map(|jump| SshProxy::Jump {
                profile: aliases.get(&jump).cloned().unwrap_or(jump),
            })
            .or_else(|| {
                options
                    .proxy_command
                    .filter(|command| !command.trim().is_empty())
                    .map(|command| {
                        if command.trim().eq_ignore_ascii_case("none") {
                            SshProxy::Direct
                        } else {
                            SshProxy::Command { command }
                        }
                    })
            })
            .or_else(|| {
                options
                    .socks_proxy_host
                    .zip(options.socks_proxy_port)
                    .map(|(host, port)| SshProxy::Socks {
                        address: endpoint(&host, port),
                    })
            })
            .or_else(|| {
                options
                    .http_proxy_host
                    .zip(options.http_proxy_port)
                    .map(|(host, port)| SshProxy::Http {
                        address: endpoint(&host, port),
                    })
            });
        let options = SshOptions {
            auth: options.auth.as_deref().and_then(|auth| match auth {
                "password" => Some(SshAuth::Password),
                "publicKey" => Some(SshAuth::Key),
                "agent" => Some(SshAuth::Agent),
                "keyboardInteractive" => Some(SshAuth::Interactive),
                _ => None,
            }),
            proxy,
            identity_files: options.private_keys.filter(|keys| !keys.is_empty()),
            forward_agent: options.agent_forward,
            keepalive_ms: options.keepalive_interval,
            keepalive_count: options.keepalive_count_max,
            ready_timeout_ms: options.ready_timeout,
            x11: options.x11,
            skip_banner: options.skip_banner,
            term: options.term,
            remember_cwd: options.remember_cwd,
            reuse_session: options.reuse_session,
            forwards: options.forwarded_ports.map(|rules| {
                rules
                    .into_iter()
                    .filter_map(FileForward::into_rule)
                    .collect()
            }),
            algorithms: options.algorithms.map(|a| AlgorithmOptions {
                kex: a.kex,
                cipher: a.cipher,
                mac: a.hmac,
                host_key: a.server_host_key,
                compression: a.compression,
            }),
            ..SshOptions::default()
        };
        profile.args = system_args(&target, &options);
        profile.ssh_options = Some(options);
        Some(profile)
    }
}

fn system_args(target: &Target, options: &SshOptions) -> Vec<String> {
    let mut args = Vec::new();
    for key in options.identity_files.iter().flatten() {
        args.extend(["-i".into(), key.clone()]);
    }
    for (name, value) in [
        ("ForwardAgent", options.forward_agent),
        ("ForwardX11", options.x11),
    ] {
        if let Some(value) = value {
            args.extend([
                "-o".into(),
                format!("{name}={}", if value { "yes" } else { "no" }),
            ]);
        }
    }
    for (name, value) in [
        (
            "ServerAliveInterval",
            options.keepalive_ms.map(|ms| ms.div_ceil(1000)),
        ),
        (
            "ServerAliveCountMax",
            options.keepalive_count.map(|count| count as u64),
        ),
        (
            "ConnectTimeout",
            options.ready_timeout_ms.map(|ms| ms.div_ceil(1000)),
        ),
    ] {
        if let Some(value) = value {
            args.extend(["-o".into(), format!("{name}={value}")]);
        }
    }
    match &options.proxy {
        Some(SshProxy::Command { command }) => {
            args.extend(["-o".into(), format!("ProxyCommand={command}")])
        }
        Some(SshProxy::Direct) => args.extend(["-o".into(), "ProxyCommand=none".into()]),
        _ => {}
    }
    for rule in options.forwards.iter().flatten() {
        let flag = match rule.kind {
            ForwardKind::Local => "-L",
            ForwardKind::Remote => "-R",
            ForwardKind::Dynamic => "-D",
        };
        let address = rule.destination.as_ref().map_or_else(
            || rule.listen.clone(),
            |dest| format!("{}:{dest}", rule.listen),
        );
        args.extend([flag.into(), address]);
    }
    let mut target_args = target.args();
    if target_args.last().is_some_and(|host| host.starts_with('-')) {
        target_args.insert(target_args.len() - 1, "--".into());
    }
    args.extend(target_args);
    args
}

#[cfg(test)]
#[path = "../test/ssh_profiles.rs"]
mod tests;
