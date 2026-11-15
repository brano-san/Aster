use regex::Regex;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Script {
    pub expect: String,
    pub send: String,
    pub regex: bool,
    pub optional: bool,
}

pub fn unescape(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if let Some(&next) = bytes.get(index + 1) {
                let control = match next {
                    b'a' => Some(7),
                    b'b' => Some(8),
                    b'e' => Some(27),
                    b'f' => Some(12),
                    b'n' => Some(10),
                    b'r' => Some(13),
                    b't' => Some(9),
                    b'v' => Some(11),
                    b'\\' => Some(92),
                    _ => None,
                };
                if let Some(byte) = control {
                    output.push(byte);
                    index += 2;
                    continue;
                }
                let length = match next {
                    b'x' => 2,
                    b'u' => 4,
                    _ => 0,
                };
                if length > 0 {
                    if let Some(digits) = bytes.get(index + 2..index + 2 + length) {
                        if digits.iter().all(u8::is_ascii_hexdigit) {
                            let value =
                                u32::from_str_radix(std::str::from_utf8(digits).unwrap(), 16)
                                    .unwrap();
                            if next == b'x' {
                                output.push(value as u8);
                                index += 2 + length;
                                continue;
                            }
                            if let Some(character) = char::from_u32(value) {
                                let mut encoded = [0; 4];
                                output.extend_from_slice(
                                    character.encode_utf8(&mut encoded).as_bytes(),
                                );
                                index += 2 + length;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    output
}

pub struct ScriptRunner {
    scripts: Vec<Script>,
    patterns: Vec<Option<Regex>>,
    current: usize,
    buffer: String,
    pending_utf8: Vec<u8>,
    escape: Escape,
}

#[derive(Clone, Copy)]
enum Escape {
    None,
    Start,
    Csi,
    Osc,
    OscTerminator,
}

impl ScriptRunner {
    pub fn new(scripts: Vec<Script>) -> Result<Self, String> {
        let patterns = scripts
            .iter()
            .enumerate()
            .map(|(index, script)| {
                if script.regex {
                    Regex::new(&script.expect)
                        .map(Some)
                        .map_err(|error| format!("Script {index}: {error}"))
                } else {
                    Ok(None)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            scripts,
            patterns,
            current: 0,
            buffer: String::new(),
            pending_utf8: Vec::new(),
            escape: Escape::None,
        })
    }

    pub fn start(&mut self) -> Vec<u8> {
        let mut send = Vec::new();
        while let Some(script) = self.scripts.get(self.current) {
            if !script.expect.is_empty() {
                break;
            }
            send.extend(unescape(&script.send));
            self.current += 1;
        }
        send
    }

    pub fn feed(&mut self, output: &[u8]) -> Vec<u8> {
        if self.done() {
            return Vec::new();
        }
        self.append_output(output);
        let mut send = Vec::new();
        loop {
            let mut matched = None;
            for index in self.current..self.scripts.len() {
                let script = &self.scripts[index];
                let end = match &self.patterns[index] {
                    Some(regex) => regex.find(&self.buffer).map(|found| found.end()),
                    None => self
                        .buffer
                        .find(&script.expect)
                        .map(|start| start + script.expect.len()),
                };
                if let Some(end) = end {
                    matched = Some((index, end));
                    break;
                }
                if !script.optional {
                    break;
                }
            }
            let Some((index, end)) = matched else {
                break;
            };
            send.extend(unescape(&self.scripts[index].send));
            self.current = index + 1;
            self.buffer.drain(..end);
            send.extend(self.start());
        }
        send
    }

    pub fn done(&self) -> bool {
        self.current == self.scripts.len()
    }

    fn append_output(&mut self, output: &[u8]) {
        for &byte in output {
            self.escape = match self.escape {
                Escape::None if byte == 0x1b => Escape::Start,
                Escape::None => {
                    self.pending_utf8.push(byte);
                    Escape::None
                }
                Escape::Start => match byte {
                    b'[' => Escape::Csi,
                    b']' => Escape::Osc,
                    _ => Escape::None,
                },
                Escape::Csi if (0x40..=0x7e).contains(&byte) => Escape::None,
                Escape::Csi => Escape::Csi,
                Escape::Osc | Escape::OscTerminator if byte == 7 => Escape::None,
                Escape::Osc | Escape::OscTerminator if byte == 0x1b => Escape::OscTerminator,
                Escape::OscTerminator if byte == b'\\' => Escape::None,
                Escape::Osc | Escape::OscTerminator => Escape::Osc,
            };
        }
        crate::utf8::decode(&mut self.pending_utf8, &mut self.buffer);
        if let Some((start, _)) = self.buffer.char_indices().rev().nth(4095) {
            self.buffer.drain(..start);
        }
    }
}

#[cfg(test)]
#[path = "../test/scripts.rs"]
mod tests;
