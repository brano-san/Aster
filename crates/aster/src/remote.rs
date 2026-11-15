use aster_platform::{Pty, Spawned};
use aster_ssh::client::{self, Request};
use aster_core::files::{FileEntry, FileReply, FileRequest};
use aster_ssh::console::Input;
use aster_ssh::sftp;
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

struct Channel(UnboundedSender<Input>);

impl Pty for Channel {
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

pub struct Remote {
    pub spawned: Spawned,
    pub input: UnboundedSender<Input>,
    pub files: Receiver<sftp::Reply>,
}

pub fn file_reply(reply: sftp::Reply) -> FileReply {
    match reply {
        sftp::Reply::Listing { path, entries } => FileReply::Listing {
            path,
            entries: entries.into_iter().map(|e| FileEntry { name: e.name, dir: e.dir, link: e.link, size: e.size, modified: e.modified }).collect(),
        },
        sftp::Reply::Progress { id, done, total } => FileReply::Progress { id, done, total },
        sftp::Reply::Downloaded { id, local } => FileReply::Downloaded { id, local },
        sftp::Reply::Uploaded { id, remote } => FileReply::Uploaded { id, remote },
        sftp::Reply::Changed { dir } => FileReply::Changed { dir },
        sftp::Reply::Failed { id, error } => FileReply::Failed { id, error },
    }
}

pub fn file_request(request: FileRequest, downloads: PathBuf) -> sftp::Request {
    match request {
        FileRequest::List(path) => sftp::Request::List { path },
        FileRequest::Download { id, remote } => sftp::Request::Download { id, remote, dir: downloads },
        FileRequest::Upload { id, local, dir } => sftp::Request::Upload { id, local, dir },
        FileRequest::Mkdir(path) => sftp::Request::Mkdir { path },
        FileRequest::Remove { path, dir } => sftp::Request::Remove { path, dir },
        FileRequest::Rename { from, to } => sftp::Request::Rename { from, to },
        FileRequest::Cancel(id) => sftp::Request::Cancel { id },
    }
}

pub fn connect(destination: &str, cols: u16, rows: u16, env_term: Option<&str>, home: Option<PathBuf>) -> Remote {
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
    let spawned = Spawned {
        pty: Box::new(Channel(conn.input.clone())),
        reader: Box::new(Output { rx: conn.output, buf: Vec::new(), at: 0 }),
        writer: Box::new(Keys(conn.input.clone())),
        wait: Box::new(move || exit.recv().ok()),
    };
    Remote { spawned, input: conn.input, files: conn.files }
}
