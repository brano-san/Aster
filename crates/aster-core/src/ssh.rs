use crate::profile::Profile;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
}

fn host_ok(h: &str) -> bool {
    !h.is_empty() && h.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '%'))
}

pub fn parse_target(text: &str) -> Option<Target> {
    let text = text.trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return None;
    }
    let (user, rest) = match text.rsplit_once('@') {
        Some((u, r)) if !u.is_empty() => (Some(u.to_owned()), r),
        Some(_) => return None,
        None => (None, text),
    };
    let (host, port) = if let Some(inner) = rest.strip_prefix('[') {
        let (host, after) = inner.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p.parse().ok()?),
            None if after.is_empty() => None,
            None => return None,
        };
        (host.to_owned(), port)
    } else if rest.matches(':').count() == 1 {
        let (h, p) = rest.split_once(':')?;
        (h.to_owned(), Some(p.parse().ok()?))
    } else {
        (rest.to_owned(), None)
    };
    host_ok(&host).then_some(Target { user, host, port })
}

impl Target {
    pub fn display(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        let base = match &self.user {
            Some(u) => format!("{u}@{host}"),
            None => host,
        };
        match self.port {
            Some(p) => format!("{base}:{p}"),
            None => base,
        }
    }

    pub fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(p) = self.port {
            args.push("-p".to_owned());
            args.push(p.to_string());
        }
        args.push(match &self.user {
            Some(u) => format!("{u}@{}", self.host),
            None => self.host.clone(),
        });
        args
    }

    pub fn profile(&self, ssh: &str) -> Profile {
        let name = self.display();
        let mut p = Profile::new(&format!("ssh:{name}"), &name, ssh, &[]);
        p.args = self.args();
        p.icon = Some("ssh".into());
        p.ssh = Some(name);
        p
    }
}

pub fn socket_profile(address: &aster_proto::address::Address) -> Profile {
    let name = address.display();
    let mut p = Profile::new(&format!("socket:{name}"), &name, "", &[]);
    p.socket = Some(name);
    p
}

pub fn host_profile(alias: &str, ssh: &str) -> Profile {
    let mut p = Profile::new(&format!("ssh:{alias}"), &format!("SSH: {alias}"), ssh, &[alias]);
    p.icon = Some("ssh".into());
    p.ssh = Some(alias.to_owned());
    p
}

#[cfg(test)]
#[path = "../test/ssh.rs"]
mod tests;
