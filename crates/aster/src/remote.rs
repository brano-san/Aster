use aster_platform::{Pty, Spawned};
use aster_ssh::client::{self, Request};
use aster_ssh::console::Input;
use crossbeam_channel::Receiver;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;

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

struct Keys(UnboundedSender<Input>);

impl Write for Keys {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.send(Input::Data(buf.to_vec())).map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Remote(UnboundedSender<Input>);

impl Pty for Remote {
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

pub fn local_user() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "root".into())
}

pub fn connect(destination: &str, cols: u16, rows: u16, env_term: Option<&str>, home: Option<PathBuf>) -> Spawned {
    let home = home.unwrap_or_default();
    let req = Request {
        destination: destination.to_owned(),
        cols,
        rows,
        term: env_term.unwrap_or("xterm-256color").to_owned(),
        ssh_dir: home.join(".ssh"),
        home,
        user: local_user(),
        use_agent: true,
    };
    let conn = client::start(req);
    let exit = conn.exit;
    Spawned {
        pty: Box::new(Remote(conn.input.clone())),
        reader: Box::new(Output { rx: conn.output, buf: Vec::new(), at: 0 }),
        writer: Box::new(Keys(conn.input)),
        wait: Box::new(move || exit.recv().ok()),
    }
}
