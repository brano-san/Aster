const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const ECHO: u8 = 1;
const SGA: u8 = 3;
const TTYPE: u8 = 24;
const NAWS: u8 = 31;
const IS: u8 = 0;
const SEND: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelnetEvent {
    Data(Vec<u8>),
    Send(Vec<u8>),
    RemoteEcho(bool),
}

pub struct Telnet {
    term: Vec<u8>,
    cols: u16,
    rows: u16,
    remote: [Option<bool>; 256],
    local: [Option<bool>; 256],
    state: ParseState,
    subnegotiation: Vec<u8>,
}

#[derive(Clone, Copy)]
enum ParseState {
    Data,
    Command,
    Option(u8),
    Subnegotiation,
    Subcommand,
}

impl Telnet {
    pub fn new(term: &str, cols: u16, rows: u16) -> Self {
        Self {
            term: term
                .chars()
                .map(|character| {
                    if character.is_ascii() {
                        character as u8
                    } else {
                        b'?'
                    }
                })
                .collect(),
            cols,
            rows,
            remote: [None; 256],
            local: [None; 256],
            state: ParseState::Data,
            subnegotiation: Vec::new(),
        }
    }

    pub fn receive(&mut self, bytes: &[u8]) -> Vec<TelnetEvent> {
        let mut events = Vec::new();
        let mut data = Vec::new();
        for &byte in bytes {
            match self.state {
                ParseState::Data => {
                    if byte == IAC {
                        self.state = ParseState::Command;
                    } else {
                        data.push(byte);
                    }
                }
                ParseState::Command => {
                    if byte == IAC {
                        data.push(IAC);
                        self.state = ParseState::Data;
                        continue;
                    }
                    flush_data(&mut data, &mut events);
                    self.state = match byte {
                        WILL | WONT | DO | DONT => ParseState::Option(byte),
                        SB => {
                            self.subnegotiation.clear();
                            ParseState::Subnegotiation
                        }
                        _ => ParseState::Data,
                    };
                }
                ParseState::Option(command) => {
                    self.negotiate(command, byte, &mut events);
                    self.state = ParseState::Data;
                }
                ParseState::Subnegotiation => {
                    if byte == IAC {
                        self.state = ParseState::Subcommand;
                    } else {
                        self.append_subnegotiation(byte);
                    }
                }
                ParseState::Subcommand => {
                    if byte == SE {
                        if self.subnegotiation == [TTYPE, SEND] {
                            let mut response = vec![IAC, SB, TTYPE, IS];
                            response.extend_from_slice(&self.term);
                            response.extend_from_slice(&[IAC, SE]);
                            events.push(TelnetEvent::Send(response));
                        }
                        self.subnegotiation.clear();
                        self.state = ParseState::Data;
                    } else {
                        if byte == IAC {
                            self.append_subnegotiation(IAC);
                        } else {
                            self.append_subnegotiation(IAC);
                            self.append_subnegotiation(byte);
                        }
                        self.state = ParseState::Subnegotiation;
                    }
                }
            }
        }
        flush_data(&mut data, &mut events);
        events
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Option<Vec<u8>> {
        self.cols = cols;
        self.rows = rows;
        (self.local[usize::from(NAWS)] == Some(true)).then(|| self.window_size())
    }

    pub fn escape_outgoing(data: &[u8]) -> Vec<u8> {
        let mut escaped = Vec::with_capacity(data.len());
        for &byte in data {
            escaped.push(byte);
            if byte == IAC {
                escaped.push(IAC);
            }
        }
        escaped
    }

    fn append_subnegotiation(&mut self, byte: u8) {
        if self.subnegotiation.len() < 3 {
            self.subnegotiation.push(byte);
        }
    }

    fn negotiate(&mut self, command: u8, option: u8, events: &mut Vec<TelnetEvent>) {
        let index = usize::from(option);
        match command {
            WILL => {
                let enabled = matches!(option, ECHO | SGA);
                if self.remote[index] != Some(enabled) {
                    self.remote[index] = Some(enabled);
                    events.push(TelnetEvent::Send(vec![
                        IAC,
                        if enabled { DO } else { DONT },
                        option,
                    ]));
                    if option == ECHO {
                        events.push(TelnetEvent::RemoteEcho(true));
                    }
                }
            }
            WONT => {
                if self.remote[index].take() == Some(true) {
                    events.push(TelnetEvent::Send(vec![IAC, DONT, option]));
                    if option == ECHO {
                        events.push(TelnetEvent::RemoteEcho(false));
                    }
                }
            }
            DO => {
                let enabled = matches!(option, SGA | TTYPE | NAWS);
                if self.local[index] != Some(enabled) {
                    self.local[index] = Some(enabled);
                    events.push(TelnetEvent::Send(vec![
                        IAC,
                        if enabled { WILL } else { WONT },
                        option,
                    ]));
                    if option == NAWS {
                        events.push(TelnetEvent::Send(self.window_size()));
                    }
                }
            }
            DONT if self.local[index].take() == Some(true) => {
                events.push(TelnetEvent::Send(vec![IAC, WONT, option]));
            }
            _ => {}
        }
    }

    fn window_size(&self) -> Vec<u8> {
        let mut response = vec![IAC, SB, NAWS];
        response.extend(Self::escape_outgoing(&self.cols.to_be_bytes()));
        response.extend(Self::escape_outgoing(&self.rows.to_be_bytes()));
        response.extend_from_slice(&[IAC, SE]);
        response
    }
}

fn flush_data(data: &mut Vec<u8>, events: &mut Vec<TelnetEvent>) {
    if !data.is_empty() {
        events.push(TelnetEvent::Data(std::mem::take(data)));
    }
}

#[cfg(test)]
#[path = "../test/telnet.rs"]
mod tests;
