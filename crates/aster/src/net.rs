use aster_net::session::{self, Input, Options};
use aster_platform::{Pty, Spawned};
use crossbeam_channel::{Receiver, Sender};
use std::io::{self, Read, Write};

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

struct Keys(Sender<Input>, fn(Vec<u8>) -> Input);

impl Write for Keys {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.send((self.1)(buf.to_vec())).map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Link(Sender<Input>);

impl Pty for Link {
    fn resize(&self, cols: u16, rows: u16) {
        let _ = self.0.send(Input::Resize(cols, rows));
    }

    fn kill(&self) {
        let _ = self.0.send(Input::Close);
    }

    fn pid(&self) -> u32 {
        0
    }
}

pub fn connect(spec: &str, cols: u16, rows: u16, term: Option<&str>, echo: bool) -> Spawned {
    let address = match aster_proto::address::parse(spec) {
        Ok(address) => address,
        Err(message) => {
            let (output_tx, output) = crossbeam_channel::unbounded();
            let (input, _) = crossbeam_channel::unbounded();
            let (exit_tx, exit) = crossbeam_channel::bounded(1);
            let _ = output_tx.send(format!("{message}\r\n").into_bytes());
            let _ = exit_tx.send(1);
            return spawned(session::Connection { output, input, exit });
        }
    };
    let options = Options { term: term.unwrap_or("xterm-256color").to_owned(), cols, rows, echo };
    spawned(session::open(address, options))
}

fn spawned(connection: session::Connection) -> Spawned {
    let (drained, finished) = crossbeam_channel::bounded::<()>(0);
    let exit = connection.exit;
    Spawned {
        pty: Box::new(Link(connection.input.clone())),
        reader: Box::new(Output { rx: connection.output, buf: Vec::new(), at: 0, drained: Some(drained) }),
        writer: Box::new(Keys(connection.input.clone(), Input::Data)),
        raw: Some(Box::new(Keys(connection.input, Input::Raw))),
        wait: Box::new(move || {
            let code = exit.recv().ok();
            let _ = finished.recv();
            code
        }),
    }
}
