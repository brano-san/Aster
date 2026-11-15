use crate::frame::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RxEvent {
    Send(Vec<u8>),
    FileStart { name: String, size: Option<u64> },
    Data(Vec<u8>),
    FileEnd,
    Done,
    Aborted,
}

pub struct Receiver {
    decoder: Decoder,
    pending: Option<u8>,
    position: u32,
    file: bool,
    accepting: bool,
    finished: bool,
}

fn ready() -> Vec<u8> { hex_header(Header::new(ZRINIT, [0, 0, 0, CANFDX | CANOVIO | CANFC32])) }
fn reply(kind: u8, position: u32) -> RxEvent { RxEvent::Send(hex_header(Header::at(kind, position))) }

impl Receiver {
    pub fn new() -> Self {
        Self { decoder: Decoder::new(), pending: None, position: 0, file: false, accepting: false, finished: false }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<RxEvent> {
        if self.finished { return Vec::new(); }
        let mut out = Vec::new();
        for item in self.decoder.feed(bytes) {
            match item {
                Item::Cancel => { self.finished = true; out.push(RxEvent::Aborted); }
                Item::BadCrc if self.file => { self.accepting = false; out.push(reply(ZRPOS, self.position)); }
                Item::Header(h) => {
                    self.pending = Some(h.kind);
                    self.accepting = false;
                    match h.kind {
                        ZRQINIT => out.push(RxEvent::Send(ready())),
                        ZDATA if self.file => {
                            self.accepting = h.position() == self.position;
                            if !self.accepting { out.push(reply(ZRPOS, self.position)); }
                        }
                        ZEOF if self.file && h.position() == self.position => {
                            self.file = false;
                            out.push(RxEvent::FileEnd);
                            out.push(RxEvent::Send(ready()));
                        }
                        ZFIN => { out.push(reply(ZFIN, 0)); out.push(RxEvent::Done); self.finished = true; }
                        _ => {}
                    }
                }
                Item::Subpacket { data, end } => {
                    match self.pending {
                        Some(ZSINIT) => { out.push(reply(ZACK, 0)); self.pending = None; }
                        Some(ZFILE) => {
                            let mut parts = data.split(|b| *b == 0);
                            let raw = String::from_utf8_lossy(parts.next().unwrap_or_default());
                            let base = raw.rsplit(['/', '\\']).next().unwrap_or_default();
                            let name = if matches!(base, "" | "." | "..") { "file" } else { base }.to_owned();
                            let size = parts.next().and_then(|part| std::str::from_utf8(part).ok()).and_then(|s| s.split_whitespace().next()).and_then(|s| s.parse().ok());
                            self.position = 0;
                            self.file = true;
                            self.pending = None;
                            out.push(RxEvent::FileStart { name, size });
                            out.push(reply(ZRPOS, 0));
                        }
                        Some(ZDATA) if self.file && self.accepting => {
                            if let Some(position) = self.position.checked_add(data.len() as u32) {
                                self.position = position;
                                if !data.is_empty() { out.push(RxEvent::Data(data)); }
                                if matches!(end, ZCRCW | ZCRCQ) { out.push(reply(ZACK, self.position)); }
                            } else { out.push(RxEvent::Send(self.abort())); out.push(RxEvent::Aborted); }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            if self.finished { break; }
        }
        out
    }

    pub fn skip(&mut self) -> Vec<u8> {
        if self.finished || !self.file { return Vec::new(); }
        self.file = false;
        self.accepting = false;
        self.pending = None;
        hex_header(Header::new(ZSKIP, [0; 4]))
    }

    pub fn abort(&mut self) -> Vec<u8> {
        self.finished = true;
        let mut bytes = vec![CAN; 8];
        bytes.extend([0x08; 8]);
        bytes
    }

    pub fn finished(&self) -> bool { self.finished }
}

impl Default for Receiver { fn default() -> Self { Self::new() } }

#[cfg(test)]
#[path = "../test/receive.rs"]
mod tests;
