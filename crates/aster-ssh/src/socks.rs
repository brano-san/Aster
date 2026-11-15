#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Reply(Vec<u8>),
    Connect { host: String, port: u16 },
    Fail(Vec<u8>),
    More,
}

pub struct Handshake {
    bytes: Vec<u8>,
    version: u8,
    greeted: bool,
    done: bool,
    consumed: usize,
}

impl Default for Handshake {
    fn default() -> Self { Self::new() }
}

impl Handshake {
    pub fn new() -> Self { Self { bytes: Vec::new(), version: 5, greeted: false, done: false, consumed: 0 } }

    fn reply(code: u8) -> Vec<u8> { vec![5, code, 0, 1, 0, 0, 0, 0, 0, 0] }

    fn fail(&mut self, reply: Vec<u8>) -> Vec<Step> {
        self.done = true;
        self.bytes.clear();
        vec![Step::Fail(reply)]
    }

    pub fn success(&self) -> Vec<u8> {
        if self.version == 4 { vec![0, 0x5a, 0, 0, 0, 0, 0, 0] } else { Self::reply(0) }
    }

    pub fn failure(&self) -> Vec<u8> {
        if self.version == 4 { vec![0, 0x5b, 0, 0, 0, 0, 0, 0] } else { Self::reply(1) }
    }

    pub fn rest(&mut self) -> Vec<u8> { if self.done { std::mem::take(&mut self.bytes) } else { Vec::new() } }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Step> {
        if self.done { self.bytes.extend_from_slice(bytes); return Vec::new(); }
        self.bytes.extend_from_slice(bytes);
        if self.bytes.is_empty() { return vec![Step::More]; }
        if !self.greeted { self.version = self.bytes[0]; }
        let mut result = Vec::new();
        if self.version == 5 && !self.greeted {
            if self.bytes.len() < 2 { return vec![Step::More]; }
            let end = 2 + self.bytes[1] as usize;
            if self.bytes.len() < end { return vec![Step::More]; }
            if !self.bytes[2..end].contains(&0) { return self.fail(vec![5, 255]); }
            self.bytes.drain(..end);
            self.consumed = end;
            self.greeted = true;
            result.push(Step::Reply(vec![5, 0]));
        }
        let parsed = match self.version {
            5 => self.parse_five(),
            4 => self.parse_four(),
            _ => return self.fail(Vec::new()),
        };
        match parsed {
            Ok(Some((host, port, end))) => {
                if self.consumed + end > 1024 { result.extend(self.fail(self.failure())); }
                else {
                    self.bytes.drain(..end);
                    self.done = true;
                    result.push(Step::Connect { host, port });
                }
            }
            Ok(None) => {
                if self.consumed + self.bytes.len() > 1024 { result.extend(self.fail(self.failure())); }
                else if result.is_empty() { result.push(Step::More); }
            }
            Err(reply) => result.extend(self.fail(reply)),
        }
        result
    }

    fn parse_five(&self) -> Result<Option<(String, u16, usize)>, Vec<u8>> {
        let b = &self.bytes;
        if b.len() < 4 { return Ok(None); }
        if b[0] != 5 || b[2] != 0 { return Err(self.failure()); }
        if b[1] != 1 { return Err(Self::reply(7)); }
        let (host, end) = match b[3] {
            1 => {
                if b.len() < 10 { return Ok(None); }
                (std::net::Ipv4Addr::new(b[4], b[5], b[6], b[7]).to_string(), 8)
            }
            3 => {
                if b.len() < 5 { return Ok(None); }
                let end = 5 + b[4] as usize;
                if b.len() < end + 2 { return Ok(None); }
                if end == 5 { return Err(self.failure()); }
                (std::str::from_utf8(&b[5..end]).map_err(|_| self.failure())?.to_owned(), end)
            }
            4 => {
                if b.len() < 22 { return Ok(None); }
                let mut ip = [0; 16]; ip.copy_from_slice(&b[4..20]);
                (std::net::Ipv6Addr::from(ip).to_string(), 20)
            }
            _ => return Err(Self::reply(8)),
        };
        Ok(Some((host, u16::from_be_bytes([b[end], b[end + 1]]), end + 2)))
    }

    fn parse_four(&self) -> Result<Option<(String, u16, usize)>, Vec<u8>> {
        let b = &self.bytes;
        if b.len() < 2 { return Ok(None); }
        if b[1] != 1 { return Err(self.failure()); }
        if b.len() < 9 { return Ok(None); }
        let Some(user_end) = b[8..].iter().position(|&v| v == 0).map(|v| v + 9) else { return Ok(None); };
        let (host, end) = if b[4..7] == [0, 0, 0] && b[7] != 0 {
            let Some(n) = b[user_end..].iter().position(|&v| v == 0) else { return Ok(None); };
            if n == 0 { return Err(self.failure()); }
            (std::str::from_utf8(&b[user_end..user_end + n]).map_err(|_| self.failure())?.to_owned(), user_end + n + 1)
        } else { (std::net::Ipv4Addr::new(b[4], b[5], b[6], b[7]).to_string(), user_end) };
        Ok(Some((host, u16::from_be_bytes([b[2], b[3]]), end)))
    }
}

#[cfg(test)]
#[path = "../test/socks.rs"]
mod tests;
