use crate::i18n::t;
use crate::profile::Profile;
use crate::ssh::{self, Target};
use aster_proto::{address::{self, Address, Protocol}, serial};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    #[default]
    Ssh,
    Telnet,
    Tcp,
    Serial,
}

pub(crate) enum QuickTarget {
    Ssh(Target),
    Socket(Address),
    Serial { spec: String, name: String },
}

impl QuickTarget {
    pub(crate) fn parse(text: &str, default: Option<Provider>, ssh_available: bool) -> Option<(Self, bool)> {
        let text = text.trim();
        if text.is_empty() || text.chars().any(char::is_control) {
            return None;
        }
        let lower = text.to_ascii_lowercase();
        if lower.starts_with("ssh://") {
            return Self::ssh(&text["ssh://".len()..], ssh_available).map(|target| (target, true));
        }
        if lower.starts_with("serial:") {
            return Self::serial(text["serial:".len()..].trim()).map(|target| (target, true));
        }
        if lower.contains("://") {
            return address::parse_url(text).map(|address| (Self::Socket(address), true));
        }
        if let Some((port, _)) = lower.rsplit_once('@') {
            let com_port = port.strip_prefix("com").is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()));
            if com_port || port.starts_with("/dev/") {
                return Self::serial(text).map(|target| (target, true));
            }
        }
        let target = match default? {
            Provider::Ssh => Self::ssh(text, ssh_available),
            Provider::Telnet => address::parse(text).ok().map(Self::Socket),
            Provider::Tcp => address::parse(&format!("tcp://{text}")).ok().map(Self::Socket),
            Provider::Serial if !text.chars().any(char::is_whitespace) => Self::serial(text),
            Provider::Serial => None,
        };
        target.map(|target| (target, text.contains('@') || text.contains(':')))
    }

    fn ssh(text: &str, available: bool) -> Option<Self> {
        available.then(|| ssh::parse_target(text)).flatten().filter(|target| target.port != Some(0)).map(Self::Ssh)
    }

    fn serial(text: &str) -> Option<Self> {
        let spec = match text.rsplit_once('@') {
            Some((port, speed)) => {
                let baud = speed.split(',').next()?;
                if baud != "?" && (baud.is_empty() || !baud.bytes().all(|b| b.is_ascii_digit())) {
                    return None;
                }
                format!("{port}:{speed}")
            }
            None => text.to_owned(),
        };
        let settings = serial::parse(&spec).ok()?;
        if settings.port.contains([':', '@']) {
            return None;
        }
        let baud = if settings.ask_baud { "?".to_owned() } else { settings.baud.to_string() };
        let has_speed = spec.split(',').next().is_some_and(|head| head.contains(':'));
        let name = if has_speed { format!("{}@{baud}", settings.port) } else { settings.port };
        Some(Self::Serial { spec, name })
    }

    pub(crate) fn label(&self) -> String {
        match self {
            Self::Ssh(target) => target.display(),
            Self::Socket(address) => address.display(),
            Self::Serial { name, .. } => name.clone(),
        }
    }

    pub(crate) fn provider_label(&self) -> &'static str {
        match self {
            Self::Ssh(_) => t("SSH"),
            Self::Socket(address) if address.protocol == Protocol::Telnet => t("Telnet"),
            Self::Socket(_) => t("TCP"),
            Self::Serial { .. } => t("Serial port"),
        }
    }

    pub(crate) fn value(&self) -> String {
        match self {
            Self::Serial { spec, .. } => format!("serial:{spec}"),
            Self::Ssh(target) => format!("ssh://{}", target.display()),
            Self::Socket(address) => address.display(),
        }
    }

    pub(crate) fn profile(&self, ssh: Option<&str>) -> Option<Profile> {
        match self {
            Self::Ssh(target) => ssh.map(|command| target.profile(command)),
            Self::Socket(address) => Some(ssh::socket_profile(address)),
            Self::Serial { spec, name } => {
                let mut profile = Profile::new(&format!("serial:{spec}"), name, "", &[]);
                profile.serial = Some(spec.clone());
                Some(profile)
            }
        }
    }
}

#[cfg(test)]
#[path = "../test/quick_connect.rs"]
mod tests;
