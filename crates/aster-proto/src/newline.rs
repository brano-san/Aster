#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Newlines {
    #[default]
    Keep,
    Strip,
    Cr,
    Lf,
    CrLf,
    ImplicitCr,
    ImplicitLf,
}

pub struct NewlineConverter {
    mode: Newlines,
    previous_cr: bool,
}

impl NewlineConverter {
    pub fn new(mode: Newlines) -> Self {
        Self {
            mode,
            previous_cr: false,
        }
    }

    pub fn convert(&mut self, input: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(input.len());
        for &byte in input {
            match self.mode {
                Newlines::Keep => output.push(byte),
                Newlines::Strip => {
                    if !matches!(byte, b'\r' | b'\n') {
                        output.push(byte);
                    }
                }
                Newlines::Cr | Newlines::Lf | Newlines::CrLf => {
                    if byte == b'\r' || (byte == b'\n' && !self.previous_cr) {
                        output.extend_from_slice(match self.mode {
                            Newlines::Cr => b"\r",
                            Newlines::Lf => b"\n",
                            _ => b"\r\n",
                        });
                    } else if byte != b'\n' {
                        output.push(byte);
                    }
                }
                Newlines::ImplicitCr => {
                    if byte == b'\n' && !self.previous_cr {
                        output.push(b'\r');
                    }
                    output.push(byte);
                }
                Newlines::ImplicitLf => {
                    if self.previous_cr && byte != b'\n' {
                        output.push(b'\n');
                    }
                    output.push(byte);
                }
            }
            self.previous_cr = byte == b'\r';
        }
        output
    }
}

#[cfg(test)]
#[path = "../test/newline.rs"]
mod tests;
