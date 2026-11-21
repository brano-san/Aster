#[derive(Debug, PartialEq, Eq)]
pub enum UrlCommand {
    Profile(String),
    Open(String),
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode(value: &str) -> Option<String> {
    if value.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '&' | '=' | '#' | '"')) {
        return None;
    }
    let mut bytes = value.bytes();
    let mut decoded = Vec::with_capacity(value.len());
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' { (hex(bytes.next()?)? << 4) | hex(bytes.next()?)? } else { byte });
    }
    let decoded = String::from_utf8(decoded).ok()?;
    (!decoded.trim().is_empty() && !decoded.chars().any(char::is_control)).then_some(decoded)
}

pub fn parse(url: &str) -> Option<UrlCommand> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("aster") {
        return None;
    }
    let (route, query) = rest.split_once('?')?;
    let (parameter, value) = query.split_once('=')?;
    match (route, parameter) {
        ("profile", "profileName") => decode(value).map(UrlCommand::Profile),
        ("open", "path") => decode(value).map(UrlCommand::Open),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../test/url_scheme.rs"]
mod tests;
