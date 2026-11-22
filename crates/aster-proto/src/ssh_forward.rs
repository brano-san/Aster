use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForwardKind {
    #[default]
    Local,
    Remote,
    Dynamic,
}

impl ForwardKind {
    pub fn as_str(self) -> &'static str {
        match self { Self::Local => "local", Self::Remote => "remote", Self::Dynamic => "dynamic" }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ForwardRule {
    pub kind: ForwardKind,
    pub listen: String,
    pub destination: Option<String>,
    #[serde(default)]
    pub description: String,
}

impl ForwardRule {
    pub fn resolve(&self) -> Result<Forward, String> {
        if self.kind == ForwardKind::Dynamic {
            return parse(self.kind.as_str(), &self.listen);
        }
        let destination = self.destination.as_deref().filter(|s| !s.trim().is_empty()).ok_or("missing forward destination")?;
        if self.listen.split_whitespace().count() != 1 || destination.split_whitespace().count() != 1 {
            return Err("expected one address per forward field".into());
        }
        parse(self.kind.as_str(), &format!("{} {destination}", self.listen))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Forward {
    Local { bind: String, port: u16, host: String, host_port: u16 },
    Remote { bind: String, port: u16, host: String, host_port: u16 },
    Dynamic { bind: String, port: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardStatus {
    pub index: usize,
    pub forward: Forward,
    pub bound: Result<u32, String>,
    pub stop_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    AddForward(Forward),
    StopForward(usize),
}

fn fields(value: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new(); let mut field = String::new(); let mut bracket = false; let mut closed = false;
    for c in value.chars() {
        match c {
            '[' if field.is_empty() && !bracket && !closed => bracket = true,
            ']' if bracket => { bracket = false; closed = true; }
            ':' if !bracket => { fields.push(std::mem::take(&mut field)); closed = false; }
            '[' | ']' => return Err("invalid address brackets".into()),
            _ if closed => return Err("unexpected text after address".into()),
            _ => field.push(c),
        }
    }
    if bracket { return Err("unclosed address bracket".into()); }
    fields.push(field);
    if fields.iter().any(String::is_empty) { return Err("missing address or port".into()); }
    Ok(fields)
}

fn port(value: &str, zero: bool) -> Result<u16, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) { return Err("port must be a decimal number".into()); }
    let port = value.parse::<u16>().map_err(|_| "port is outside 0..65535".to_owned())?;
    if port == 0 && !zero { return Err("destination port must be greater than zero".into()); }
    Ok(port)
}

pub fn parse(kind: &str, spec: &str) -> Result<Forward, String> {
    let words: Vec<_> = spec.split_whitespace().collect();
    let kind = kind.to_ascii_lowercase();
    if !matches!(kind.as_str(), "local" | "remote" | "dynamic") { return Err("unknown forward kind".into()); }
    let pieces = match words.as_slice() {
        [one] => fields(one)?,
        [left, right] if kind != "dynamic" => {
            let mut a = fields(left)?; let b = fields(right)?;
            if !(1..=2).contains(&a.len()) || b.len() != 2 { return Err("expected bind port and destination host:port".into()); }
            a.extend(b); a
        }
        _ => return Err("invalid forward specification".into()),
    };
    let needed = if kind == "dynamic" { 1 } else { 3 };
    if pieces.len() != needed && pieces.len() != needed + 1 { return Err("missing or extra forward fields".into()); }
    let offset = pieces.len() - needed;
    let bind = if offset == 0 { "127.0.0.1" } else if pieces[0] == "*" { "0.0.0.0" } else { &pieces[0] }.to_owned();
    let port = port(&pieces[offset], true)?;
    if kind == "dynamic" { return Ok(Forward::Dynamic { bind, port }); }
    let host = pieces[offset + 1].clone(); let host_port = self::port(&pieces[offset + 2], false)?;
    Ok(if kind == "local" { Forward::Local { bind, port, host, host_port } } else { Forward::Remote { bind, port, host, host_port } })
}

