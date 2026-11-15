use crate::crc::{crc16, crc32};

pub const ZPAD: u8 = b'*';
pub const ZDLE: u8 = 0x18;
pub const ZDLEE: u8 = 0x58;
pub const ZBIN: u8 = b'A';
pub const ZHEX: u8 = b'B';
pub const ZBIN32: u8 = b'C';
pub const ZCRCE: u8 = b'h';
pub const ZCRCG: u8 = b'i';
pub const ZCRCQ: u8 = b'j';
pub const ZCRCW: u8 = b'k';
pub const ZRUB0: u8 = b'l';
pub const ZRUB1: u8 = b'm';
pub const XON: u8 = 0x11;
pub const XOFF: u8 = 0x13;
pub const CAN: u8 = 0x18;
pub const ZRQINIT: u8 = 0;
pub const ZRINIT: u8 = 1;
pub const ZSINIT: u8 = 2;
pub const ZACK: u8 = 3;
pub const ZFILE: u8 = 4;
pub const ZSKIP: u8 = 5;
pub const ZNAK: u8 = 6;
pub const ZABORT: u8 = 7;
pub const ZFIN: u8 = 8;
pub const ZRPOS: u8 = 9;
pub const ZDATA: u8 = 10;
pub const ZEOF: u8 = 11;
pub const ZFERR: u8 = 12;
pub const ZCRC: u8 = 13;
pub const ZCHALLENGE: u8 = 14;
pub const ZCOMPL: u8 = 15;
pub const ZCAN: u8 = 16;
pub const ZFREECNT: u8 = 17;
pub const ZCOMMAND: u8 = 18;
pub const CANFDX: u8 = 0x01;
pub const CANOVIO: u8 = 0x02;
pub const CANFC32: u8 = 0x20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: u8,
    pub data: [u8; 4],
}

impl Header {
    pub fn new(kind: u8, data: [u8; 4]) -> Self { Self { kind, data } }
    pub fn at(kind: u8, position: u32) -> Self { Self::new(kind, position.to_le_bytes()) }
    pub fn position(&self) -> u32 { u32::from_le_bytes(self.data) }
    pub fn flags(&self) -> u8 { self.data[3] }
    fn bytes(self) -> Vec<u8> { let mut bytes = vec![self.kind]; bytes.extend(self.data); bytes }
}

pub fn escape(data: &[u8], out: &mut Vec<u8>) {
    for &byte in data {
        if matches!(byte, 0x0d | 0x10 | 0x11 | 0x13 | 0x18 | 0x8d | 0x90 | 0x91 | 0x93) {
            out.extend([ZDLE, byte ^ 0x40]);
        } else { out.push(byte); }
    }
}

fn checksum(data: &[u8], wide: bool) -> Vec<u8> {
    if wide { crc32(data).to_le_bytes().to_vec() } else { crc16(data).to_be_bytes().to_vec() }
}

pub fn hex_header(header: Header) -> Vec<u8> {
    let mut out = vec![ZPAD, ZPAD, ZDLE, ZHEX];
    let mut bytes = header.bytes();
    bytes.extend(checksum(&bytes, false));
    let hex = b"0123456789abcdef";
    for byte in bytes { out.extend([hex[(byte >> 4) as usize], hex[(byte & 15) as usize]]); }
    out.extend([0x0d, 0x8a]);
    if !matches!(header.kind, ZACK | ZFIN) { out.push(XON); }
    out
}

pub fn bin_header(header: Header, crc32: bool) -> Vec<u8> {
    let mut out = vec![ZPAD, ZDLE, if crc32 { ZBIN32 } else { ZBIN }];
    let mut bytes = header.bytes();
    bytes.extend(checksum(&bytes, crc32));
    escape(&bytes, &mut out);
    out
}

pub fn subpacket(data: &[u8], end: u8, crc32: bool) -> Vec<u8> {
    let mut out = Vec::new();
    escape(data, &mut out);
    out.extend([ZDLE, end]);
    let mut checked = data.to_vec();
    checked.push(end);
    escape(&checksum(&checked, crc32), &mut out);
    if end == ZCRCW { out.push(XON); }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Header(Header),
    Subpacket { data: Vec<u8>, end: u8 },
    BadCrc,
    Cancel,
}

enum State {
    Seek(u8),
    Hex(Vec<u8>),
    Binary(Vec<u8>),
    Tail(u8),
    Data(Vec<u8>),
    Check { data: Vec<u8>, end: u8, crc: Vec<u8> },
}

pub struct Decoder {
    state: State,
    wide: bool,
    escaped: bool,
    cancels: u8,
}

impl Decoder {
    pub fn new() -> Self { Self { state: State::Seek(0), wide: false, escaped: false, cancels: 0 } }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Item> {
        let mut out = Vec::new();
        for &byte in bytes {
            self.cancels = if byte == CAN { self.cancels + 1 } else { 0 };
            if self.cancels == 5 {
                self.cancels = 0;
                self.state = State::Seek(0);
                self.escaped = false;
                out.push(Item::Cancel);
                continue;
            }
            self.byte(byte, &mut out);
        }
        out
    }

    fn bad(&mut self, out: &mut Vec<Item>) {
        self.state = State::Seek(0);
        self.escaped = false;
        out.push(Item::BadCrc);
    }

    fn header(&mut self, bytes: &[u8], hex: bool, out: &mut Vec<Item>) {
        if checksum(&bytes[..5], self.wide) != bytes[5..] { self.bad(out); return; }
        let header = Header::new(bytes[0], bytes[1..5].try_into().unwrap());
        self.state = if matches!(header.kind, ZFILE | ZDATA | ZSINIT | ZCOMMAND) {
            if hex { State::Tail(0) } else { State::Data(Vec::new()) }
        } else { State::Seek(0) };
        out.push(Item::Header(header));
    }

    fn byte(&mut self, mut byte: u8, out: &mut Vec<Item>) {
        if matches!(self.state, State::Binary(_) | State::Data(_) | State::Check { .. }) {
            if matches!(byte, XON | XOFF | 0x91 | 0x93) { return; }
            if self.escaped {
                self.escaped = false;
                if matches!(byte, ZCRCE | ZCRCG | ZCRCQ | ZCRCW) && matches!(self.state, State::Data(_)) {
                    let State::Data(data) = std::mem::replace(&mut self.state, State::Seek(0)) else { unreachable!() };
                    self.state = State::Check { data, end: byte, crc: Vec::new() };
                    return;
                }
                byte = match byte {
                    ZRUB0 => 0x7f,
                    ZRUB1 => 0xff,
                    c if c & 0x60 == 0x40 => c ^ 0x40,
                    _ => { self.bad(out); return; }
                };
            } else if byte == ZDLE { self.escaped = true; return; }
        }
        let state = std::mem::replace(&mut self.state, State::Seek(0));
        match state {
            State::Seek(stage) => {
                self.state = match (stage, byte) {
                    (_, ZPAD) => State::Seek(1),
                    (1, ZDLE) => State::Seek(2),
                    (2, ZHEX) => { self.wide = false; State::Hex(Vec::new()) }
                    (2, ZBIN | ZBIN32) => { self.wide = byte == ZBIN32; State::Binary(Vec::new()) }
                    _ => State::Seek(0),
                };
            }
            State::Hex(mut digits) => {
                if let Some(nibble) = (byte as char).to_digit(16) {
                    digits.push(nibble as u8);
                    if digits.len() == 14 {
                        let decoded: Vec<u8> = digits.chunks_exact(2).map(|p| p[0] * 16 + p[1]).collect();
                        self.header(&decoded, true, out);
                    } else { self.state = State::Hex(digits); }
                } else { self.bad(out); }
            }
            State::Binary(mut bytes) => {
                bytes.push(byte);
                if bytes.len() == if self.wide { 9 } else { 7 } { self.header(&bytes, false, out); }
                else { self.state = State::Binary(bytes); }
            }
            State::Tail(stage) => {
                match (stage, byte) {
                    (0, b'\r') => self.state = State::Tail(1),
                    (0 | 1, b'\n' | 0x8a) => self.state = State::Tail(2),
                    (_, XON) => self.state = State::Data(Vec::new()),
                    _ => { self.state = State::Data(Vec::new()); self.byte(byte, out); }
                }
            }
            State::Data(mut data) => {
                data.push(byte);
                if data.len() > 8192 { self.bad(out); } else { self.state = State::Data(data); }
            }
            State::Check { mut data, end, mut crc } => {
                crc.push(byte);
                if crc.len() == if self.wide { 4 } else { 2 } {
                    data.push(end);
                    let valid = checksum(&data, self.wide) == crc;
                    data.pop();
                    if valid {
                        self.state = if matches!(end, ZCRCG | ZCRCQ) { State::Data(Vec::new()) } else { State::Seek(0) };
                        out.push(Item::Subpacket { data, end });
                    } else { self.bad(out); }
                } else { self.state = State::Check { data, end, crc }; }
            }
        }
    }
}

impl Default for Decoder { fn default() -> Self { Self::new() } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detected { Receive, Send }

pub fn detect(bytes: &[u8]) -> Option<(usize, Detected)> {
    bytes.windows(6).enumerate().find_map(|(i, bytes)| match bytes {
        b"**\x18B00" => Some((i, Detected::Receive)),
        b"**\x18B01" => Some((i, Detected::Send)),
        _ => None,
    })
}

#[cfg(test)]
#[path = "../test/frame.rs"]
mod tests;
