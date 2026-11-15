use crate::frame::*;

pub trait Source {
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool { self.len() == 0 }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> usize;
}

impl Source for Vec<u8> {
    fn len(&self) -> u64 { Vec::len(self) as u64 }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> usize {
        let Ok(offset) = usize::try_from(offset) else { return 0; };
        let Some(data) = self.get(offset..) else { return 0; };
        let len = data.len().min(buf.len());
        buf[..len].copy_from_slice(&data[..len]);
        len
    }
}

pub struct SendFile {
    pub name: String,
    pub modified: u64,
    pub source: Box<dyn Source + Send>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TxEvent {
    Progress { file: usize, sent: u64 },
    FileDone(usize),
    Skipped(usize),
    Done,
    Aborted,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State { Init, Offer, AwaitPosition, Header, Data, Eof, AwaitEof, Finish, AwaitFinish, Ended }

pub struct Sender {
    files: Vec<SendFile>,
    decoder: Decoder,
    state: State,
    index: usize,
    position: u32,
    wide: bool,
    goodbye: bool,
    failed: bool,
}

impl Sender {
    pub fn new(files: Vec<SendFile>) -> Self {
        Self { files, decoder: Decoder::new(), state: State::Init, index: 0, position: 0, wide: false, goodbye: false, failed: false }
    }

    fn next(&mut self) {
        self.state = if self.index == self.files.len() { State::Finish } else { State::Offer };
        self.position = 0;
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<TxEvent> {
        if self.failed { self.failed = false; return vec![TxEvent::Aborted]; }
        if self.finished() { return Vec::new(); }
        let mut out = Vec::new();
        for item in self.decoder.feed(bytes) {
            match item {
                Item::Cancel => { self.state = State::Ended; out.push(TxEvent::Aborted); }
                Item::Header(h) => match h.kind {
                    ZRINIT if self.state == State::Init => { self.wide = h.flags() & CANFC32 != 0; self.next(); }
                    ZRINIT if self.state == State::AwaitEof => {
                        out.push(TxEvent::Progress { file: self.index, sent: self.files[self.index].source.len() });
                        out.push(TxEvent::FileDone(self.index));
                        self.index += 1;
                        self.next();
                    }
                    ZRINIT if self.state == State::AwaitPosition => self.state = State::Offer,
                    ZRPOS if matches!(self.state, State::AwaitPosition | State::Header | State::Data | State::Eof | State::AwaitEof) => {
                        if h.position() as u64 <= self.files[self.index].source.len() {
                            self.position = h.position();
                            self.state = State::Header;
                        }
                    }
                    ZACK if self.index < self.files.len() => out.push(TxEvent::Progress { file: self.index, sent: h.position() as u64 }),
                    ZSKIP if matches!(self.state, State::AwaitPosition | State::Header | State::Data | State::Eof | State::AwaitEof) => {
                        out.push(TxEvent::Skipped(self.index));
                        self.index += 1;
                        self.next();
                    }
                    ZFIN if self.state == State::AwaitFinish => {
                        self.goodbye = true;
                        self.state = State::Ended;
                        out.push(TxEvent::Done);
                    }
                    _ => {}
                },
                _ => {}
            }
            if self.finished() { break; }
        }
        out
    }

    pub fn poll(&mut self) -> Option<Vec<u8>> {
        if self.goodbye { self.goodbye = false; return Some(b"OO".to_vec()); }
        match self.state {
            State::Offer => {
                let file = &self.files[self.index];
                if file.source.len() > u32::MAX as u64 {
                    self.failed = true;
                    return Some(self.abort());
                }
                let remaining: u64 = self.files[self.index..].iter().map(|f| f.source.len()).fold(0, u64::saturating_add);
                let info = format!("{}\0{} {:o} 0 0 {} {}\0", file.name, file.source.len(), file.modified, self.files.len() - self.index, remaining);
                let mut bytes = bin_header(Header::new(ZFILE, [0; 4]), self.wide);
                bytes.extend(subpacket(info.as_bytes(), ZCRCW, self.wide));
                self.state = State::AwaitPosition;
                Some(bytes)
            }
            State::Header => {
                self.state = State::Data;
                Some(bin_header(Header::at(ZDATA, self.position), self.wide))
            }
            State::Data => {
                let file = &mut self.files[self.index];
                let remaining = file.source.len().saturating_sub(self.position as u64);
                let mut buf = [0; 1024];
                let limit = remaining.min(1024) as usize;
                let read = if limit == 0 { 0 } else { file.source.read_at(self.position as u64, &mut buf[..limit]) };
                if read > limit || (read == 0 && limit != 0) {
                    self.failed = true;
                    return Some(self.abort());
                }
                self.position += read as u32;
                let end = if self.position as u64 == file.source.len() { self.state = State::Eof; ZCRCE } else { ZCRCG };
                Some(subpacket(&buf[..read], end, self.wide))
            }
            State::Eof => {
                self.state = State::AwaitEof;
                Some(bin_header(Header::at(ZEOF, self.position), self.wide))
            }
            State::Finish => {
                self.state = State::AwaitFinish;
                Some(hex_header(Header::new(ZFIN, [0; 4])))
            }
            _ => None,
        }
    }

    pub fn abort(&mut self) -> Vec<u8> {
        self.state = State::Ended;
        self.goodbye = false;
        let mut bytes = vec![CAN; 8];
        bytes.extend([0x08; 8]);
        bytes
    }

    pub fn finished(&self) -> bool { self.state == State::Ended }
}

#[cfg(test)]
#[path = "../test/send.rs"]
mod tests;
