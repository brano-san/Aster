use crate::hex::HexDump;
use crate::input::{map_backspace, Backspace, InputMode, InputProcessor, Processed};
use crate::newline::{NewlineConverter, Newlines};
use crate::scripts::{Script, ScriptRunner};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamOptions {
    pub input: InputMode,
    pub hex_output: bool,
    pub newlines_in: Newlines,
    pub newlines_out: Newlines,
    pub backspace: Backspace,
    pub login: Vec<Script>,
}

impl StreamOptions {
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }
}

pub struct Outgoing {
    backspace: Backspace,
    input: InputProcessor,
    newlines: NewlineConverter,
}

impl Outgoing {
    pub fn new(options: &StreamOptions) -> Self {
        Self {
            backspace: options.backspace,
            input: InputProcessor::new(options.input),
            newlines: NewlineConverter::new(options.newlines_out),
        }
    }

    pub fn feed(&mut self, keys: &[u8]) -> Processed {
        let keys = map_backspace(keys, self.backspace);
        let processed = self.input.feed(&keys);
        Processed {
            send: self.newlines.convert(&processed.send),
            echo: processed.echo,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Received {
    pub display: Vec<u8>,
    pub reply: Vec<u8>,
}

pub struct Incoming {
    newlines: NewlineConverter,
    hex: Option<HexDump>,
    login: Option<ScriptRunner>,
}

impl Incoming {
    pub fn new(options: &StreamOptions) -> (Self, Option<String>) {
        let (login, error) = if options.login.is_empty() {
            (None, None)
        } else {
            match ScriptRunner::new(options.login.clone()) {
                Ok(runner) => (Some(runner), None),
                Err(error) => (None, Some(error)),
            }
        };
        let incoming = Self {
            newlines: NewlineConverter::new(options.newlines_in),
            hex: options.hex_output.then(HexDump::new),
            login,
        };
        (incoming, error)
    }

    pub fn start(&mut self) -> Vec<u8> {
        self.login.as_mut().map(ScriptRunner::start).unwrap_or_default()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Received {
        let reply = self
            .login
            .as_mut()
            .map(|login| login.feed(bytes))
            .unwrap_or_default();
        if self.login.as_ref().is_some_and(ScriptRunner::done) {
            self.login = None;
        }
        let converted = self.newlines.convert(bytes);
        let display = match self.hex.as_mut() {
            Some(hex) => hex.format(&converted).into_bytes(),
            None => converted,
        };
        Received { display, reply }
    }
}

#[cfg(test)]
#[path = "../test/stream.rs"]
mod tests;
