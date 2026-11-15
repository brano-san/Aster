#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backspace {
    #[default]
    Backspace,
    CtrlH,
    CtrlQuestion,
    Delete,
}

pub fn map_backspace(keys: &[u8], mode: Backspace) -> Vec<u8> {
    let mut output = Vec::with_capacity(keys.len());
    for &byte in keys {
        if byte == 0x7f {
            output.extend_from_slice(match mode {
                Backspace::Backspace | Backspace::CtrlQuestion => b"\x7f",
                Backspace::CtrlH => b"\x08",
                Backspace::Delete => b"\x1b[3~",
            });
        } else {
            output.push(byte);
        }
    }
    output
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputMode {
    #[default]
    Normal,
    LocalEcho,
    Readline,
    ReadlineHex,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Processed {
    pub send: Vec<u8>,
    pub echo: Vec<u8>,
}

pub struct InputProcessor {
    mode: InputMode,
    line: String,
    pending_utf8: Vec<u8>,
    escape: Escape,
}

enum Escape {
    None,
    Start,
    Sequence,
}

impl InputProcessor {
    pub fn new(mode: InputMode) -> Self {
        Self {
            mode,
            line: String::new(),
            pending_utf8: Vec::new(),
            escape: Escape::None,
        }
    }

    pub fn feed(&mut self, keys: &[u8]) -> Processed {
        let mut result = Processed::default();
        if matches!(self.mode, InputMode::Normal | InputMode::LocalEcho) {
            result.send.extend_from_slice(keys);
        }
        if self.mode == InputMode::Normal {
            return result;
        }
        self.pending_utf8.extend_from_slice(keys);
        let mut text = String::new();
        crate::utf8::decode(&mut self.pending_utf8, &mut text);
        for character in text.chars() {
            match self.escape {
                Escape::Start => {
                    self.escape = if matches!(character, '[' | 'O') {
                        Escape::Sequence
                    } else {
                        Escape::None
                    };
                    continue;
                }
                Escape::Sequence => {
                    if ('\u{40}'..='\u{7e}').contains(&character) {
                        self.escape = Escape::None;
                    }
                    continue;
                }
                Escape::None => {}
            }
            if character == '\u{1b}' {
                self.escape = Escape::Start;
                continue;
            }
            if character >= ' ' && character != '\u{7f}' {
                let mut bytes = [0; 4];
                result
                    .echo
                    .extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
                if self.mode != InputMode::LocalEcho {
                    self.line.push(character);
                }
                continue;
            }
            match character {
                '\u{08}' | '\u{7f}' => {
                    if self.mode == InputMode::LocalEcho || self.line.pop().is_some() {
                        result.echo.extend_from_slice(b"\x08 \x08");
                    }
                }
                '\r' | '\n' => {
                    if self.mode == InputMode::LocalEcho {
                        if character == '\r' {
                            result.echo.extend_from_slice(b"\r\n");
                        }
                        continue;
                    }
                    result.echo.extend_from_slice(b"\r\n");
                    if self.mode == InputMode::ReadlineHex {
                        match parse_hex(&self.line) {
                            Some(bytes) => result.send.extend(bytes),
                            None => result.echo.extend_from_slice(b"[invalid hex]\r\n"),
                        }
                    } else {
                        result.send.extend_from_slice(self.line.as_bytes());
                        result.send.push(b'\r');
                    }
                    self.line.clear();
                }
                '\u{03}' if self.mode != InputMode::LocalEcho => {
                    self.line.clear();
                    result.echo.extend_from_slice(b"^C\r\n");
                    result.send.push(3);
                }
                '\u{15}' if self.mode != InputMode::LocalEcho => {
                    for _ in self.line.chars() {
                        result.echo.extend_from_slice(b"\x08 \x08");
                    }
                    self.line.clear();
                }
                _ => {}
            }
        }
        result
    }
}

fn parse_hex(line: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    for token in line.split(' ').filter(|token| !token.is_empty()) {
        let digits = token
            .strip_prefix("0x")
            .or_else(|| token.strip_prefix("0X"))
            .unwrap_or(token);
        if digits.is_empty()
            || digits.len() % 2 != 0
            || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        for index in (0..digits.len()).step_by(2) {
            bytes.push(u8::from_str_radix(&digits[index..index + 2], 16).ok()?);
        }
    }
    Some(bytes)
}

#[cfg(test)]
#[path = "../test/input.rs"]
mod tests;
