use crate::client::*;
use crate::console::Input;
use russh::keys::PublicKey;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use russh::server::{self, Auth, Msg, Session};
use russh::{Channel, ChannelId};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use std::borrow::Cow;

pub const HOST_KEY: &str = include_str!("keys/host");
pub const CLIENT_KEY: &str = include_str!("keys/client");
pub const CLIENT_PUB: &str = include_str!("keys/client.pub");

#[derive(Clone, Default)]
pub struct Seen {
    pub sizes: Arc<std::sync::Mutex<Vec<(u32, u32)>>>,
    pub terms: Arc<std::sync::Mutex<Vec<String>>>,
    pub tunnels: Arc<std::sync::Mutex<Vec<String>>>,
}

struct Server {
    seen: Seen,
    client_key: PublicKey,
    sessions: Vec<ChannelId>,
    channels: HashMap<ChannelId, Channel<Msg>>,
    forwards: HashMap<(String, u32), tokio::task::JoinHandle<()>>,
    files: Option<PathBuf>,
    interactive: Option<u8>,
}

impl Drop for Server {
    fn drop(&mut self) {
        for task in self.forwards.values() { task.abort(); }
    }
}

impl server::Handler for Server {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == "ann" && password == "secret" { Auth::Accept } else { Auth::reject() })
    }

    async fn auth_keyboard_interactive<'a>(&'a mut self, user: &str, _: &str, response: Option<server::Response<'a>>) -> Result<Auth, Self::Error> {
        let Some(round) = self.interactive.as_mut().filter(|_| user == "ann") else { return Ok(Auth::reject()) };
        let responses = response.map(|answers| answers.collect::<Vec<_>>());
        Ok(match (*round, responses.as_deref()) {
            (0, None) => Auth::Partial {
                name: "Sign in to the SSH server".into(), instructions: "See https://example.test/device for instructions.".into(),
                prompts: vec![(Cow::Borrowed("Account label"), true), (Cow::Borrowed("Password"), false)].into(),
            },
            (0, Some([account, password])) if account.as_ref() == b"office" && password.as_ref() == b"secret" => {
                *round = 1;
                Auth::Partial { name: "Verification".into(), instructions: "Enter the verification code.".into(),
                    prompts: vec![(Cow::Borrowed("Code"), false)].into() }
            }
            (1, Some([code])) if code.as_ref() == b"123456" => Auth::Accept,
            _ => Auth::reject(),
        })
    }

    async fn auth_publickey(&mut self, user: &str, key: server::VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
        Ok(if user == "ann" && key.public_key.key_data() == self.client_key.key_data() { Auth::Accept } else { Auth::reject() })
    }

    async fn channel_open_session(&mut self, channel: Channel<Msg>, reply: server::ChannelOpenHandle, _session: &mut Session) -> Result<(), Self::Error> {
        self.sessions.push(channel.id());
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn pty_request(&mut self, _c: ChannelId, term: &str, cols: u32, rows: u32, _pw: u32, _ph: u32, _modes: &[(russh::Pty, u32)], _s: &mut Session) -> Result<(), Self::Error> {
        self.seen.terms.lock().unwrap().push(term.to_owned());
        self.seen.sizes.lock().unwrap().push((cols, rows));
        Ok(())
    }

    async fn window_change_request(&mut self, _c: ChannelId, cols: u32, rows: u32, _pw: u32, _ph: u32, _s: &mut Session) -> Result<(), Self::Error> {
        self.seen.sizes.lock().unwrap().push((cols, rows));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn channel_open_direct_tcpip(&mut self, channel: Channel<Msg>, host: &str, port: u32, _oa: &str, _op: u32, reply: server::ChannelOpenHandle, _s: &mut Session) -> Result<(), Self::Error> {
        self.seen.tunnels.lock().unwrap().push(format!("{host}:{port}"));
        let target = tokio::net::TcpStream::connect((host.to_owned(), port as u16)).await;
        let Ok(mut target) = target else { return Ok(()) };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut target).await;
        });
        Ok(())
    }

    async fn subsystem_request(&mut self, channel: ChannelId, name: &str, session: &mut Session) -> Result<(), Self::Error> {
        let (Some(root), Some(ch)) = (self.files.clone(), self.channels.remove(&channel)) else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        if name != "sftp" {
            session.channel_failure(channel)?;
            return Ok(());
        }
        self.sessions.retain(|c| *c != channel);
        session.channel_success(channel)?;
        tokio::spawn(russh_sftp::server::run(ch.into_stream(), Fs::new(root)));
        Ok(())
    }

    async fn tcpip_forward(&mut self, address: &str, port: &mut u32, session: &mut Session) -> Result<bool, Self::Error> {
        let Ok(listener) = tokio::net::TcpListener::bind((address.to_owned(), *port as u16)).await else { return Ok(false) };
        *port = listener.local_addr().map(|a| a.port() as u32).unwrap_or(*port);
        let (handle, address, bound) = (session.handle(), address.to_owned(), *port);
        let key = (address.clone(), bound);
        let task = tokio::spawn(async move {
            while let Ok((mut socket, peer)) = listener.accept().await {
                let (handle, address) = (handle.clone(), address.clone());
                tokio::spawn(async move {
                    if let Ok(ch) = handle.channel_open_forwarded_tcpip(address, bound, peer.ip().to_string(), peer.port() as u32).await {
                        let mut stream = ch.into_stream();
                        let _ = tokio::io::copy_bidirectional(&mut stream, &mut socket).await;
                    }
                });
            }
        });
        self.forwards.insert(key, task);
        Ok(true)
    }

    async fn cancel_tcpip_forward(&mut self, address: &str, port: u32, _: &mut Session) -> Result<bool, Self::Error> {
        let Some(task) = self.forwards.remove(&(address.into(), port)) else { return Ok(false) };
        task.abort();
        let _ = task.await;
        Ok(true)
    }

    async fn agent_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<bool, Self::Error> {
        let handle = session.handle();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let reply = match handle.channel_open_agent().await {
                Ok(agent) => {
                    let mut stream = agent.into_stream();
                    let mut buf = [0u8; 4];
                    match stream.write_all(b"ping").await {
                        Ok(()) => match stream.read_exact(&mut buf).await {
                            Ok(_) => String::from_utf8_lossy(&buf).into_owned(),
                            Err(e) => format!("read failed: {e}"),
                        },
                        Err(e) => format!("write failed: {e}"),
                    }
                }
                Err(e) => format!("refused: {e}"),
            };
            let _ = handle.data(channel, format!("agent said {reply}
").into_bytes()).await;
        });
        Ok(true)
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        session.data(channel, &b"welcome\r\n"[..])?;
        Ok(())
    }

    async fn exec_request(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        session.data(channel, data.to_vec())?;
        Ok(())
    }

    async fn x11_request(&mut self, channel: ChannelId, single: bool, protocol: &str, cookie: &str, screen: u32, session: &mut Session) -> Result<(), Self::Error> {
        if !single && protocol == "MIT-MAGIC-COOKIE-1" && cookie.len() == 32 && cookie.bytes().all(|c| c.is_ascii_hexdigit()) && screen == 3 {
            session.channel_success(channel)?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }

    async fn data(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        if !self.sessions.contains(&channel) {
            return Ok(());
        }
        if data == b"exit\r" {
            session.exit_status_request(channel, 7)?;
            session.close(channel)?;
        } else {
            session.data(channel, format!("got:{}\r\n", String::from_utf8_lossy(data).trim_end()).into_bytes())?;
        }
        Ok(())
    }
}

pub fn serve() -> (u16, Seen) {
    serve_files(None)
}

pub fn serve_files(files: Option<PathBuf>) -> (u16, Seen) {
    serve_with(files, false)
}

pub fn serve_interactive() -> (u16, Seen) {
    serve_with(None, true)
}

fn serve_with(files: Option<PathBuf>, interactive: bool) -> (u16, Seen) {
    let seen = Seen::default();
    let shared = seen.clone();
    let (port_tx, port_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            port_tx.send(listener.local_addr().unwrap().port()).unwrap();
            let config = Arc::new(server::Config {
                keys: vec![russh::keys::PrivateKey::from_openssh(HOST_KEY).unwrap()],
                auth_rejection_time: Duration::from_millis(1),
                auth_rejection_time_initial: Some(Duration::ZERO),
                ..Default::default()
            });
            let client_key = PublicKey::from_openssh(CLIENT_PUB.trim()).unwrap();
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let handler = Server { seen: shared.clone(), client_key: client_key.clone(), sessions: Vec::new(), channels: HashMap::new(), forwards: HashMap::new(), files: files.clone(), interactive: interactive.then_some(0) };
                let config = config.clone();
                tokio::spawn(async move {
                    if let Ok(running) = server::run_stream(config, socket, handler).await {
                        let _ = running.await;
                    }
                });
            }
        });
    });
    (port_rx.recv().unwrap(), seen)
}

pub fn temp_ssh_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("aster-ssh-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn request(destination: String, ssh_dir: PathBuf) -> Request {
    Request { destination, cols: 90, rows: 25, home: ssh_dir.clone(), ssh_dir, ..Request::default() }
}

pub struct Pane {
    pub conn: Connection,
    pub seen: String,
}

impl Pane {
    pub fn wait_for(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.seen.contains(text) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.conn.output.recv_timeout(left) {
                Ok(bytes) => self.seen.push_str(&String::from_utf8_lossy(&bytes)),
                Err(_) => panic!("{text:?} never appeared in {:?}", self.seen),
            }
        }
    }

    pub fn send(&self, text: &str) {
        self.conn.input.send(Input::Data(text.as_bytes().to_vec())).unwrap();
    }

    pub fn exit(&self) -> u32 {
        self.conn.exit.recv_timeout(Duration::from_secs(10)).unwrap()
    }
}

pub fn connect_to(port: u16, ssh_dir: PathBuf) -> Pane {
    Pane { conn: start(request(format!("ann@127.0.0.1:{port}"), ssh_dir)), seen: String::new() }
}


pub struct Fs {
    root: PathBuf,
    dirs: HashMap<String, bool>,
    files: HashMap<String, std::fs::File>,
    next: u64,
}

impl Fs {
    pub fn new(root: PathBuf) -> Self {
        Self { root, dirs: HashMap::new(), files: HashMap::new(), next: 0 }
    }

    fn local(&self, path: &str) -> PathBuf {
        let clean = path.trim_start_matches('/');
        if clean.is_empty() || clean == "." {
            self.root.clone()
        } else {
            self.root.join(clean)
        }
    }

    fn handle(&mut self) -> String {
        self.next += 1;
        self.next.to_string()
    }
}

fn attrs(meta: &std::fs::Metadata) -> russh_sftp::protocol::FileAttributes {
    let mut a = russh_sftp::protocol::FileAttributes::empty();
    a.size = Some(meta.len());
    a.permissions = Some(if meta.is_dir() { 0o040755 } else { 0o100644 });
    a.mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as u32);
    a
}

fn status(id: u32) -> russh_sftp::protocol::Status {
    russh_sftp::protocol::Status { id, status_code: russh_sftp::protocol::StatusCode::Ok, error_message: "Ok".into(), language_tag: "en-US".into() }
}

fn code(e: std::io::Error) -> russh_sftp::protocol::StatusCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => russh_sftp::protocol::StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => russh_sftp::protocol::StatusCode::PermissionDenied,
        _ => russh_sftp::protocol::StatusCode::Failure,
    }
}

impl russh_sftp::server::Handler for Fs {
    type Error = russh_sftp::protocol::StatusCode;

    async fn setstat(&mut self, id: u32, path: String, attrs: russh_sftp::protocol::FileAttributes) -> Result<russh_sftp::protocol::Status, Self::Error> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = attrs.permissions { std::fs::set_permissions(self.local(&path), std::fs::Permissions::from_mode(mode)).map_err(code)?; }
        }
        #[cfg(not(unix))]
        let _ = (path, attrs);
        Ok(status(id))
    }

    fn unimplemented(&self) -> Self::Error {
        russh_sftp::protocol::StatusCode::OpUnsupported
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<russh_sftp::protocol::Name, Self::Error> {
        let path = if path == "." || path.is_empty() { "/".to_owned() } else { path };
        Ok(russh_sftp::protocol::Name { id, files: vec![russh_sftp::protocol::File::dummy(path)] })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<russh_sftp::protocol::Handle, Self::Error> {
        let local = self.local(&path);
        if !local.is_dir() {
            return Err(russh_sftp::protocol::StatusCode::NoSuchFile);
        }
        let handle = format!("d{}", self.handle());
        self.dirs.insert(format!("{handle}\n{path}"), false);
        Ok(russh_sftp::protocol::Handle { id, handle: format!("{handle}\n{path}") })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<russh_sftp::protocol::Name, Self::Error> {
        let done = self.dirs.get_mut(&handle).ok_or(russh_sftp::protocol::StatusCode::Failure)?;
        if *done {
            return Err(russh_sftp::protocol::StatusCode::Eof);
        }
        *done = true;
        let path = handle.split_once('\n').map(|(_, p)| p.to_owned()).unwrap_or_default();
        let mut files = Vec::new();
        for e in std::fs::read_dir(self.local(&path)).map_err(code)?.flatten() {
            let meta = e.metadata().map_err(code)?;
            files.push(russh_sftp::protocol::File::new(e.file_name().to_string_lossy().into_owned(), attrs(&meta)));
        }
        Ok(russh_sftp::protocol::Name { id, files })
    }

    async fn open(&mut self, id: u32, filename: String, pflags: russh_sftp::protocol::OpenFlags, _a: russh_sftp::protocol::FileAttributes) -> Result<russh_sftp::protocol::Handle, Self::Error> {
        let options: std::fs::OpenOptions = pflags.into();
        let file = options.open(self.local(&filename)).map_err(code)?;
        let handle = format!("f{}", self.handle());
        self.files.insert(handle.clone(), file);
        Ok(russh_sftp::protocol::Handle { id, handle })
    }

    async fn read(&mut self, id: u32, handle: String, offset: u64, len: u32) -> Result<russh_sftp::protocol::Data, Self::Error> {
        use std::io::{Read, Seek, SeekFrom};
        let file = self.files.get_mut(&handle).ok_or(russh_sftp::protocol::StatusCode::Failure)?;
        file.seek(SeekFrom::Start(offset)).map_err(code)?;
        let mut data = vec![0u8; len as usize];
        let n = file.read(&mut data).map_err(code)?;
        if n == 0 {
            return Err(russh_sftp::protocol::StatusCode::Eof);
        }
        data.truncate(n);
        Ok(russh_sftp::protocol::Data { id, data })
    }

    async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<russh_sftp::protocol::Status, Self::Error> {
        use std::io::{Seek, SeekFrom, Write};
        let file = self.files.get_mut(&handle).ok_or(russh_sftp::protocol::StatusCode::Failure)?;
        file.seek(SeekFrom::Start(offset)).map_err(code)?;
        file.write_all(&data).map_err(code)?;
        Ok(status(id))
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<russh_sftp::protocol::Status, Self::Error> {
        self.files.remove(&handle);
        self.dirs.remove(&handle);
        Ok(status(id))
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<russh_sftp::protocol::Attrs, Self::Error> {
        let meta = std::fs::metadata(self.local(&path)).map_err(code)?;
        Ok(russh_sftp::protocol::Attrs { id, attrs: attrs(&meta) })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<russh_sftp::protocol::Attrs, Self::Error> {
        self.stat(id, path).await
    }

    async fn mkdir(&mut self, id: u32, path: String, _a: russh_sftp::protocol::FileAttributes) -> Result<russh_sftp::protocol::Status, Self::Error> {
        std::fs::create_dir(self.local(&path)).map_err(code)?;
        Ok(status(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<russh_sftp::protocol::Status, Self::Error> {
        std::fs::remove_dir(self.local(&path)).map_err(code)?;
        Ok(status(id))
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<russh_sftp::protocol::Status, Self::Error> {
        std::fs::remove_file(self.local(&filename)).map_err(code)?;
        Ok(status(id))
    }

    async fn rename(&mut self, id: u32, oldpath: String, newpath: String) -> Result<russh_sftp::protocol::Status, Self::Error> {
        std::fs::rename(self.local(&oldpath), self.local(&newpath)).map_err(code)?;
        Ok(status(id))
    }
}

pub async fn sftp_session(port: u16) -> russh_sftp::client::SftpSession {
    struct Accept;
    impl russh::client::Handler for Accept {
        type Error = russh::Error;
        async fn check_server_key(&mut self, _key: &russh::keys::PublicKeyOrCertificate) -> Result<bool, Self::Error> { Ok(true) }
    }
    let mut connection = russh::client::connect(Arc::new(russh::client::Config::default()), ("127.0.0.1", port), Accept).await.unwrap();
    assert!(connection.authenticate_password("ann", "secret").await.unwrap().success());
    let channel = connection.channel_open_session().await.unwrap();
    channel.request_subsystem(true, "sftp").await.unwrap();
    russh_sftp::client::SftpSession::new(channel.into_stream()).await.unwrap()
}
