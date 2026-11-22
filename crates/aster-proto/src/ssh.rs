use serde::{Deserialize, Serialize};

pub const PRIVATE_KEY_KIND: &str = "key-file";
pub const VAULT_IDENTITY_PREFIX: &str = "vault:";

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SshOptions {
    pub user: Option<String>,
    pub port: Option<u16>,
    pub proxy: Option<SshProxy>,
    pub auth: Option<SshAuth>,
    pub forwards: Option<Vec<crate::ssh_forward::ForwardRule>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub inherit_forwards: bool,
    pub identity_files: Option<Vec<String>>,
    pub keepalive_ms: Option<u64>,
    pub keepalive_count: Option<usize>,
    pub ready_timeout_ms: Option<u64>,
    pub skip_banner: Option<bool>,
    pub term: Option<String>,
    pub remember_cwd: Option<bool>,
    pub forward_agent: Option<bool>,
    pub reuse_session: Option<bool>,
    pub x11: Option<bool>,
    pub algorithms: Option<AlgorithmOptions>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case")]
pub enum SshProxy {
    Direct,
    Command { command: String },
    Jump { profile: String },
    Socks { address: String },
    Http { address: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SshAuth {
    #[default]
    Auto,
    Password,
    Key,
    Agent,
    Interactive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowsAgent {
    #[default]
    Auto,
    Pageant,
    NamedPipe,
}

pub fn proxy_endpoint(address: &str) -> Option<(&str, u16)> {
    let (host, port) = address.rsplit_once(':')?;
    let port = port.parse().ok().filter(|p| *p != 0)?;
    if let Some(host) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        host.parse::<std::net::Ipv6Addr>().ok()?;
        return Some((host, port));
    }
    (!host.is_empty() && host.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))).then_some((host, port))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct AlgorithmOptions {
    pub kex: Option<Vec<String>>,
    pub cipher: Option<Vec<String>>,
    pub mac: Option<Vec<String>>,
    pub host_key: Option<Vec<String>>,
    pub compression: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Algorithms {
    pub kex: Vec<String>,
    pub cipher: Vec<String>,
    pub mac: Vec<String>,
    pub host_key: Vec<String>,
    pub compression: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlgorithmCatalog {
    pub supported: Algorithms,
    pub defaults: Algorithms,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractivePrompt {
    pub label: String,
    pub echo: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractiveChallenge {
    pub id: u64,
    pub name: String,
    pub instructions: String,
    pub prompts: Vec<InteractivePrompt>,
    pub remember: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SshEvent {
    Challenge(InteractiveChallenge),
    Finished,
    Forwards(Vec<crate::ssh_forward::ForwardStatus>),
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractiveAnswer {
    pub id: u64,
    pub answers: Option<Vec<String>>,
    pub remember: bool,
}

impl std::fmt::Debug for InteractiveAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractiveAnswer").field("id", &self.id).field("remember", &self.remember).finish_non_exhaustive()
    }
}
