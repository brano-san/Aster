use aster_core::profile::Profile;
use aster_platform::{Pty, Spawned};
use aster_serial::session::{self, Input};
use aster_serial::settings::{self, Settings};
use crossbeam_channel::{Receiver, Sender};
use std::io::{self, Read, Write};

struct Output {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    at: usize,
}

impl Read for Output {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.at >= self.buf.len() {
            match self.rx.recv() {
                Ok(bytes) => {
                    self.buf = bytes;
                    self.at = 0;
                }
                Err(_) => return Ok(0),
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

struct Line(Option<Sender<Input>>);

impl Pty for Line {
    fn resize(&self, _cols: u16, _rows: u16) {}

    fn kill(&self) {
        if let Some(tx) = &self.0 {
            let _ = tx.send(Input::Close);
        }
    }

    fn pid(&self) -> u32 {
        0
    }
}

struct Message {
    text: io::Cursor<Vec<u8>>,
    done: Option<Sender<()>>,
}

impl Read for Message {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.text.read(out)?;
        if n == 0 {
            self.done = None;
        }
        Ok(n)
    }
}

fn failed(message: String) -> Spawned {
    let (done, finished) = crossbeam_channel::bounded::<()>(0);
    Spawned {
        pty: Box::new(Line(None)),
        reader: Box::new(Message { text: io::Cursor::new(format!("{message}\r\n").into_bytes()), done: Some(done) }),
        writer: Box::new(io::sink()),
        raw: None,
        wait: Box::new(move || {
            let _ = finished.recv();
            Some(1)
        }),
    }
}

pub fn connect(spec: &str) -> Spawned {
    let settings = match settings::parse(spec) {
        Ok(s) => s,
        Err(e) => return failed(e),
    };
    match session::open(&settings) {
        Ok(conn) => {
            let exit = conn.exit;
            Spawned {
                pty: Box::new(Line(Some(conn.input.clone()))),
                reader: Box::new(Output { rx: conn.output, buf: Vec::new(), at: 0 }),
                writer: Box::new(Keys(conn.input)),
                raw: None,
                wait: Box::new(move || exit.recv().ok()),
            }
        }
        Err(e) => failed(e),
    }
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
