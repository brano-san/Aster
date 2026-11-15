use std::fmt::Write;

pub struct HexDump {
    offset: usize,
}

impl HexDump {
    pub fn new() -> Self {
        Self { offset: 0 }
    }

    pub fn format(&mut self, data: &[u8]) -> String {
        let mut output = String::new();
        for chunk in data.chunks(16) {
            write!(output, "{:08x}  ", self.offset).unwrap();
            for index in 0..16 {
                if index > 0 {
                    output.push(' ');
                }
                if index == 8 {
                    output.push(' ');
                }
                if let Some(byte) = chunk.get(index) {
                    write!(output, "{byte:02x}").unwrap();
                } else {
                    output.push_str("  ");
                }
            }
            output.push_str("  |");
            for &byte in chunk {
                output.push(if (0x20..=0x7e).contains(&byte) {
                    char::from(byte)
                } else {
                    '.'
                });
            }
            output.push_str("|\r\n");
            self.offset = self.offset.wrapping_add(chunk.len());
        }
        output
    }
}

impl Default for HexDump {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../test/hex.rs"]
mod tests;
