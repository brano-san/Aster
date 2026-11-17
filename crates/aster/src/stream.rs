use aster_core::profile::Stream;
use aster_platform::Spawned;
use aster_proto::stream::{Incoming, Outgoing};
use crossbeam_channel::{select, unbounded, Receiver, Sender};
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

const DRAIN: std::time::Duration = std::time::Duration::from_secs(1);

type Line =Arc<Mutex<Box<dyn Write + Send>>>;

struct Keys {
    line: Line,
    outgoing: Outgoing,
    echo: Sender<Vec<u8>>,
}

impl Write for Keys {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let processed = self.outgoing.feed(buf);
        if !processed.echo.is_empty() {
            let _ = self.echo.send(processed.echo);
        }
        if !processed.send.is_empty() {
            let mut line = self.line.lock().map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
            line.write_all(&processed.send)?;
            line.flush()?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Shown {
    data: Receiver<Vec<u8>>,
    echo: Option<Receiver<Vec<u8>>>,
    buf: Vec<u8>,
    at: usize,
    drained: Option<Sender<()>>,
}

impl Shown {
    fn next(&mut self) -> Option<Vec<u8>> {
        let Some(echo) = &self.echo else { return self.data.recv().ok() };
        select! {
            recv(self.data) -> bytes => bytes.ok(),
            recv(echo) -> bytes => match bytes {
                Ok(bytes) => Some(bytes),
                Err(_) => {
                    self.echo = None;
                    self.data.recv().ok()
                }
            },
        }
    }
}

impl Read for Shown {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.at >= self.buf.len() {
            match self.next() {
                Some(bytes) => {
                    self.buf = bytes;
                    self.at = 0;
                }
                None => {
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

fn send_reply(line: &Line, reply: &[u8]) -> bool {
    if reply.is_empty() {
        return true;
    }
    let Ok(mut line) = line.lock() else { return false };
    line.write_all(reply).and_then(|()| line.flush()).is_ok()
}

pub fn wrap(spawned: Spawned, stream: Option<&Stream>) -> Spawned {
    let options = match stream.map(Stream::options) {
        Some(options) if !options.is_plain() => options,
        _ => return spawned,
    };
    let Spawned { pty, mut reader, writer, wait } = spawned;
    let line: Line = Arc::new(Mutex::new(writer));
    let (data_tx, data) = unbounded();
    let (echo_tx, echo) = unbounded();
    let (mut incoming, error) = Incoming::new(&options);
    if let Some(error) = error {
        let _ = data_tx.send(format!("\x1b[31mLogin script: {error}\x1b[0m\r\n").into_bytes());
    }
    let replies = line.clone();
    let started = std::thread::Builder::new().name("aster-stream".into()).spawn(move || {
        if !send_reply(&replies, &incoming.start()) {
            return;
        }
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            let received = incoming.feed(&buf[..n]);
            if !received.display.is_empty() && data_tx.send(received.display).is_err() {
                return;
            }
            if !send_reply(&replies, &received.reply) {
                return;
            }
        }
    });
    if let Err(error) = started {
        tracing::error!("stream thread: {error}");
    }
    let (drained, finished) = crossbeam_channel::bounded::<()>(0);
    Spawned {
        pty,
        reader: Box::new(Shown { data, echo: Some(echo), buf: Vec::new(), at: 0, drained: Some(drained) }),
        writer: Box::new(Keys { line, outgoing: Outgoing::new(&options), echo: echo_tx }),
        wait: Box::new(move || {
            let code = wait();
            let _ = finished.recv_timeout(DRAIN);
            code
        }),
    }
}
