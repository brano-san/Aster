use aster_platform::{Pty, Spawned};
use aster_ssh::client::{self, Request};
use aster_core::files::{FileEntry, FileReply, FileRequest};
use aster_ssh::console::Input;
use aster_ssh::secrets::Secrets;
use aster_ssh::sftp;
use std::sync::Arc;
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

pub struct Remote {
    pub spawned: Spawned,
    pub input: UnboundedSender<Input>,
    pub files: Receiver<sftp::Reply>,
    pub events: Receiver<aster_ssh::console::SshEvent>,
}

pub fn file_reply(reply: sftp::Reply) -> FileReply {
    match reply {
        sftp::Reply::Listing { path, entries } => FileReply::Listing {
            path,
            entries: entries.into_iter().map(|e| FileEntry { name: e.name, dir: e.dir, link: e.link, size: e.size, modified: e.modified, mode: e.mode }).collect(),
        },
        sftp::Reply::Progress { id, done, total } => FileReply::Progress { id, done, total },
        sftp::Reply::Downloaded { id, local } => FileReply::Downloaded { id, local },
        sftp::Reply::Uploaded { id, remote } => FileReply::Uploaded { id, remote },
        sftp::Reply::Changed { dir } => FileReply::Changed { dir },
        sftp::Reply::Editing { id, local } => FileReply::Editing { id, local },
        sftp::Reply::Saved { id, remote } => FileReply::Saved { id, remote },
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
        FileRequest::Edit { id, remote } => sftp::Request::Edit { id, remote, dir: std::env::temp_dir().join("aster-edit") },
        FileRequest::Chmod { path, mode, recursive } => sftp::Request::Chmod { path, mode, recursive },
        FileRequest::Cancel(id) => sftp::Request::Cancel { id },
    }
}

pub struct Options {
    pub profile: aster_core::profile::SshOptions,
    pub jump_profiles: aster_ssh::client::JumpProfiles,
    pub cwd: Option<String>,
    pub clear_after_connect: bool,
    pub x11_display: String,
    pub share: bool,
    pub forward_agent: bool,
    pub verify_host_keys: bool,
    pub windows_agent: aster_core::config::WindowsAgent,
    pub agent_pipe: String,
    pub remember: bool,
    pub secrets: Arc<dyn Secrets>,
}

pub fn password_key(destination: &str, profile: &aster_core::profile::SshOptions, home: &std::path::Path) -> Option<String> {
    let dest = aster_ssh::destination::resolve_file(destination, &home.join(".ssh"), home, "", profile);
    (!dest.user.trim().is_empty()).then(|| aster_ssh::secrets::password_key(&dest.user, &dest.host, dest.port))
}

pub type KeyFiles = (PathBuf, Vec<(String, bool)>);

pub fn key_files(directory: Option<PathBuf>, home: Option<PathBuf>) -> std::io::Result<KeyFiles> {
    let directory = match directory {
        Some(path) => path,
        None => {
            let home = home.ok_or(std::io::ErrorKind::NotFound)?;
            let ssh = home.join(".ssh");
            if ssh.is_dir() { ssh } else { home }
        }
    };
    let mut entries: Vec<_> = std::fs::read_dir(&directory)?.filter_map(Result::ok).filter_map(|entry| {
        let kind = entry.path().metadata().ok()?.file_type();
        if !kind.is_file() && !kind.is_dir() { return None; }
        Some((entry.file_name().into_string().ok()?, kind.is_dir()))
    }).collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())).then(a.0.cmp(&b.0)));
    Ok((directory, entries))
}

pub fn connect(destination: &str, cols: u16, rows: u16, env_term: Option<&str>, home: Option<PathBuf>, options: Options) -> Remote {
    let home = home.unwrap_or_default();
    let req = Request {
        destination: destination.to_owned(),
        cols,
        rows,
        profile: options.profile,
        jump_profiles: options.jump_profiles,
        env_term: env_term.map(str::to_owned),
        cwd: options.cwd,
        clear_after_connect: options.clear_after_connect,
        x11_display: options.x11_display,
        ssh_dir: home.join(".ssh"),
        home,
        use_agent: true,
        share: options.share,
        secrets: Some(options.secrets),
        remember: options.remember,
        forward_agent: options.forward_agent,
        verify_host_keys: options.verify_host_keys,
        windows_agent: options.windows_agent,
        agent_pipe: options.agent_pipe,
        interactive_panel: true,
    };
    let conn = client::start(req);
    let exit = conn.exit;
    let spawned = Spawned {
        pty: Box::new(Channel(conn.input.clone())),
        reader: Box::new(Output { rx: conn.output, buf: Vec::new(), at: 0 }),
        writer: Box::new(Keys(conn.input.clone())),
        raw: None,
        wait: Box::new(move || exit.recv().ok()),
    };
    Remote { spawned, input: conn.input, files: conn.files, events: conn.events }
}
