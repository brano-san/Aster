#[derive(Default)]
pub struct Sniffer {
    state: State,
    buf: Vec<u8>,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum State {
    #[default]
    Ground,
    Escape,
    Osc,
    OscEscape,
}

const MAX: usize = 4096;

impl Sniffer {
    pub fn feed(&mut self, bytes: &[u8], mut found: impl FnMut(&[u8])) {
        for &b in bytes {
            self.state = match (self.state, b) {
                (State::Ground, 0x1b) => State::Escape,
                (State::Ground, _) => State::Ground,
                (State::Escape, b']') => {
                    self.buf.clear();
                    State::Osc
                }
                (State::Escape, 0x1b) => State::Escape,
                (State::Escape, _) => State::Ground,
                (State::Osc, 0x07) => {
                    found(&self.buf);
                    State::Ground
                }
                (State::Osc, 0x1b) => State::OscEscape,
                (State::Osc, _) => {
                    if self.buf.len() < MAX {
                        self.buf.push(b);
                    }
                    State::Osc
                }
                (State::OscEscape, b'\\') => {
                    found(&self.buf);
                    State::Ground
                }
                (State::OscEscape, b']') => {
                    self.buf.clear();
                    State::Osc
                }
                (State::OscEscape, _) => State::Ground,
            };
        }
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn cwd(payload: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(payload).ok()?;
    let path = if let Some(uri) = text.strip_prefix("7;") {
        let rest = uri.strip_prefix("file://")?;
        let path = &rest[rest.find('/')?..];
        let path = percent_decode(path);
        let drive = path.as_bytes();
        if drive.len() >= 3 && drive[0] == b'/' && drive[2] == b':' && drive[1].is_ascii_alphabetic() {
            path[1..].replace('/', "\\")
        } else {
            path
        }
    } else if let Some(p) = text.strip_prefix("9;9;") {
        p.trim_matches('"').to_owned()
    } else {
        text.strip_prefix("1337;CurrentDir=")?.to_owned()
    };
    (!path.is_empty()).then_some(path)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressState {
    Normal,
    Error,
    Indeterminate,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub state: ProgressState,
    pub percent: u8,
}

pub fn progress(payload: &[u8]) -> Option<Option<Progress>> {
    let text = std::str::from_utf8(payload).ok()?.strip_prefix("9;4")?;
    let mut parts = text.strip_prefix(';').unwrap_or(text).split(';');
    let state = parts.next().unwrap_or("0").trim();
    let percent = parts.next().and_then(|p| p.trim().parse::<u32>().ok()).unwrap_or(0).min(100) as u8;
    let state = match state {
        "" | "0" => return Some(None),
        "1" => ProgressState::Normal,
        "2" => ProgressState::Error,
        "3" => ProgressState::Indeterminate,
        "4" => ProgressState::Paused,
        _ => return None,
    };
    Some(Some(Progress { state, percent }))
}
