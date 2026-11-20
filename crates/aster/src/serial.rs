use aster_core::app::SerialControl;
use aster_core::profile::{Profile, Stream};
use aster_platform::{Pty, Spawned};
use aster_serial::session::{self, DriverControl, Input};
use aster_serial::settings::{self, Settings};
use crossbeam_channel::{bounded, select, unbounded, Receiver, Sender};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

struct Output {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    at: usize,
    drained: Option<Sender<()>>,
}

impl Read for Output {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.at >= self.buf.len() {
            match self.rx.recv() {
                Ok(bytes) => {
                    self.buf = bytes;
                    self.at = 0;
                }
                Err(_) => {
                    self.drained = None;
                    return Ok(0);
                }
            }
        }
        let n = out.len().min(self.buf.len() - self.at);
        out[..n].copy_from_slice(&self.buf[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

struct Keys(Sender<Input>);

impl Write for Keys {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.send(Input::Data(buf.to_vec())).map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Line(Sender<Input>);

impl Pty for Line {
    fn resize(&self, _cols: u16, _rows: u16) {}

    fn kill(&self) {
        let _ = self.0.send(Input::Close);
    }

    fn pid(&self) -> u32 {
        0
    }
}

pub struct Serial {
    settings: Settings,
    stream: Option<Stream>,
    input: Option<Sender<Input>>,
    applied_baud: Arc<AtomicU32>,
    can_retry: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum KeysAction {
    Write,
    Reconnect,
    Close,
    Ignore,
}

impl Serial {
    pub fn new(spec: &str, stream: Option<Stream>) -> Result<Self, String> {
        let settings = settings::parse(spec).map_err(|e| aster_core::i18n::t(&e).to_owned())?;
        Ok(Self { settings, stream, input: None, applied_baud: Arc::default(), can_retry: false })
    }

    pub fn connect(&mut self, report: impl Fn(String) + Send + 'static) -> Spawned {
        self.connect_with(session::open, report)
    }

    fn connect_with(&mut self, open: impl FnOnce(&Settings) -> Result<session::Connection, String> + Send + 'static, report: impl Fn(String) + Send + 'static) -> Spawned {
        let (input, keys) = unbounded();
        let (output, rx) = unbounded();
        let (drained, finished) = bounded::<()>(0);
        let settings = self.settings.clone();
        self.applied_baud.store(0, Ordering::Relaxed);
        let applied_baud = self.applied_baud.clone();
        self.input = Some(input.clone());
        let spawned = Spawned {
            pty: Box::new(Line(input.clone())),
            reader: Box::new(Output { rx, buf: Vec::new(), at: 0, drained: Some(drained) }),
            writer: Box::new(Keys(input)),
            raw: None,
            wait: Box::new(move || {
                let code = match open(&settings) {
                    Ok(connection) => {
                        applied_baud.store(settings.baud, Ordering::Relaxed);
                        forward(connection, &keys, &output, |event| match event {
                            DriverControl::Baud(baud) => applied_baud.store(baud, Ordering::Relaxed),
                            DriverControl::BaudFailed(baud, error) => report(aster_core::i18n::serial_baud_failed(baud, &error)),
                            DriverControl::BreakFailed(error) => report(aster_core::i18n::serial_break_failed(&error)),
                        })
                    }
                    Err(error) => {
                        let message = match error.strip_prefix(&format!("Cannot open {}: ", settings.port)) {
                            Some(detail) => aster_core::i18n::serial_open_failed(&settings.port, detail),
                            None => aster_core::i18n::t(&error).to_owned(),
                        };
                        report(message);
                        Some(1)
                    }
                };
                drop(output);
                let _ = finished.recv();
                code
            }),
        };
        crate::stream::wrap_with_drain(spawned, self.stream.as_ref(), None)
    }

    pub fn disconnected(&mut self, code: Option<u32>) -> bool {
        self.input = None;
        let baud = self.applied_baud.load(Ordering::Relaxed);
        if baud != 0 {
            self.settings.baud = baud;
            self.can_retry = true;
        }
        code != Some(0) && self.can_retry
    }

    pub fn keys(&self, bytes: &[u8]) -> KeysAction {
        if self.input.is_some() {
            return KeysAction::Write;
        }
        match bytes {
            b"\r" | b"\n" | b"\r\n" => KeysAction::Reconnect,
            b"\x1b" => KeysAction::Close,
            _ => KeysAction::Ignore,
        }
    }

    pub fn control(&mut self, control: SerialControl) {
        let message = match control {
            SerialControl::Baud(baud) => {
                if self.input.is_none() {
                    self.settings.baud = baud;
                }
                Input::SetBaud(baud)
            }
            SerialControl::Break => Input::Break,
        };
        if let Some(input) = &self.input {
            let _ = input.send(message);
        }
    }
}

fn forward(connection: session::Connection, keys: &Receiver<Input>, output: &Sender<Vec<u8>>, control: impl Fn(DriverControl)) -> Option<u32> {
    let mut events = connection.control;
    loop {
        select! {
            recv(keys) -> message => {
                let message = message.unwrap_or(Input::Close);
                let close = message == Input::Close;
                let _ = connection.input.send(message);
                if close {
                    break;
                }
            },
            recv(connection.output) -> result => match result {
                Ok(bytes) => {
                    if output.send(bytes).is_err() {
                        let _ = connection.input.send(Input::Close);
                        break;
                    }
                }
                Err(_) => break,
            },
            recv(events) -> event => match event {
                Ok(event) => control(event),
                Err(_) => events = crossbeam_channel::never(),
            }
        }
    }
    let code = connection.exit.recv().ok();
    for bytes in connection.output.try_iter() {
        let _ = output.send(bytes);
    }
    for event in events.try_iter() {
        control(event);
    }
    code
}

pub fn profiles() -> Vec<Profile> {
    settings::ports()
        .into_iter()
        .map(|port| {
            let spec = settings::format(&Settings { port: port.name.clone(), ..Settings::default() });
            let name = if port.description.is_empty() { format!("Serial: {}", port.name) } else { format!("Serial: {} ({})", port.name, port.description) };
            let mut profile = Profile::new(&format!("serial:{}", port.name), &name, "", &[]);
            profile.serial = Some(spec);
            profile.icon = Some("serial".into());
            profile
        })
        .collect()
}

#[cfg(test)]
#[path = "../test/serial.rs"]
mod tests;
