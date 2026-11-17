#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Telnet,
    Raw,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
}

pub const TELNET_PORT: u16 = 23;

const SCHEMES: [(&str, Protocol); 3] = [
    ("telnet://", Protocol::Telnet),
    ("tcp://", Protocol::Raw),
    ("raw://", Protocol::Raw),
];

pub fn parse(text: &str) -> Result<Address, String> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let (protocol, rest) = SCHEMES
        .iter()
        .find(|(scheme, _)| lower.starts_with(scheme))
        .map_or((Protocol::Telnet, text), |(scheme, protocol)| {
            (*protocol, &text[scheme.len()..])
        });
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let (host, port) = split_host_port(rest).ok_or_else(|| format!("Not a network address: {text}"))?;
    let port = match (port, protocol) {
        (Some(port), _) => port,
        (None, Protocol::Telnet) => TELNET_PORT,
        (None, Protocol::Raw) => return Err(format!("A TCP address needs a port: {text}")),
    };
    if port == 0 {
        return Err(format!("Port 0 is not valid: {text}"));
    }
    Ok(Address { protocol, host, port })
}

pub fn parse_url(text: &str) -> Option<Address> {
    let lower = text.trim().to_ascii_lowercase();
    SCHEMES
        .iter()
        .any(|(scheme, _)| lower.starts_with(scheme) && lower.len() > scheme.len())
        .then(|| parse(text).ok())
        .flatten()
}

fn split_host_port(text: &str) -> Option<(String, Option<u16>)> {
    if text.is_empty() || text.chars().any(|c| c.is_whitespace() || matches!(c, '/' | '@')) {
        return None;
    }
    let (host, port) = if let Some(inner) = text.strip_prefix('[') {
        let (host, after) = inner.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port.parse().ok()?),
            None if after.is_empty() => None,
            None => return None,
        };
        (host, port)
    } else if text.matches(':').count() == 1 {
        let (host, port) = text.split_once(':')?;
        (host, Some(port.parse().ok()?))
    } else {
        (text, None)
    };
    let valid = !host.is_empty() && host.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '%'));
    valid.then(|| (host.to_owned(), port))
}

impl Address {
    pub fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn display(&self) -> String {
        let scheme = match self.protocol {
            Protocol::Telnet => "telnet",
            Protocol::Raw => "tcp",
        };
        format!("{scheme}://{}", self.authority())
    }
}

#[cfg(test)]
#[path = "../test/address.rs"]
mod tests;
