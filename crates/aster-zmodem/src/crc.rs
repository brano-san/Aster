const fn table16() -> [u16; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut value = (i as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000 != 0 { (value << 1) ^ 0x1021 } else { value << 1 };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}

const fn table32() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut value = i as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 { (value >> 1) ^ 0xedb88320 } else { value >> 1 };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}

const TABLE16: [u16; 256] = table16();
const TABLE32: [u32; 256] = table32();

pub struct Crc16(u16);
pub struct Crc32(u32);

impl Crc16 {
    pub fn new() -> Self { Self(0) }
    pub fn update(&mut self, data: &[u8]) {
        for &byte in data { self.0 = (self.0 << 8) ^ TABLE16[((self.0 >> 8) as u8 ^ byte) as usize]; }
    }
    pub fn finish(&self) -> u16 { self.0 }
}

impl Crc32 {
    pub fn new() -> Self { Self(u32::MAX) }
    pub fn update(&mut self, data: &[u8]) {
        for &byte in data { self.0 = (self.0 >> 8) ^ TABLE32[((self.0 as u8) ^ byte) as usize]; }
    }
    pub fn finish(&self) -> u32 { self.0 ^ u32::MAX }
}

impl Default for Crc16 { fn default() -> Self { Self::new() } }
impl Default for Crc32 { fn default() -> Self { Self::new() } }

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = Crc16::new();
    crc.update(data);
    crc.finish()
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(data);
    crc.finish()
}

#[cfg(test)]
#[path = "../test/crc.rs"]
mod tests;
