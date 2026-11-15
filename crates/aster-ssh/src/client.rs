use crate::console::{Console, Input};
use crate::destination::{self, Context, Destination};
use crossbeam_channel::Receiver;
use russh::client::{self, AuthResult, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::agent::client::AgentClient;
use russh::keys::agent::AgentIdentity;
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey};
use russh::{ChannelMsg, Disconnect, MethodKind, MethodSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;

pub const FAILED: u32 = 255;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_JUMPS: usize = 8;

#[derive(Clone, Default)]
pub struct Request {
    pub destination: String,
    pub cols: u16,
    pub rows: u16,
    pub term: String,
    pub ssh_dir: PathBuf,
    pub home: PathBuf,
    pub user: String,
    pub use_agent: bool,
    pub share: bool,
    pub secrets: Option<Arc<dyn crate::secrets::Secrets>>,
    pub remember: bool,
    pub forward_agent: bool,
}

pub struct Connection {
    pub output: Receiver<Vec<u8>>,
    pub input: UnboundedSender<Input>,
    pub exit: Receiver<u32>,
    pub files: Receiver<crate::sftp::Reply>,
}

pub fn start(req: Request) -> Connection {
    let (out_tx, output) = crossbeam_channel::unbounded();
    let (input, in_rx) = tokio::sync::mpsc::unbounded_channel();
    let (exit_tx, exit) = crossbeam_channel::bounded(1);
    let (files_tx, files) = crossbeam_channel::unbounded();
    let console = Console::new(out_tx, in_rx, (req.cols, req.rows));
    match runtime() {
        Some(rt) => {
            rt.spawn(async move {
                let code = run(req, console, files_tx).await;
                let _ = exit_tx.send(code);
            });
        }
        None => {
            console.print("\ncannot start the SSH client\n");
            let _ = exit_tx.send(FAILED);
        }
    }
    Connection { output, input, exit, files }
}

fn runtime() -> Option<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("aster-ssh")
                .enable_all()
                .build()
                .map_err(|e| tracing::error!("ssh runtime: {e}"))
                .ok()
        })
        .as_ref()
}

type Targets = Arc<std::sync::Mutex<HashMap<u32, (String, u16)>>>;

struct Conn {
    handle: Mutex<Option<Handle<Client>>>,
    jumps: std::sync::Mutex<Vec<Handle<Client>>>,
    targets: Targets,
    forwards: std::sync::Mutex<Vec<crate::forward::Running>>,
}

struct Tunnel(Weak<Conn>);

impl crate::forward::Opener for Tunnel {
    fn open(&self, host: String, port: u16) -> crate::forward::Opened {
        let conn = self.0.clone();
        Box::pin(async move {
            let conn = conn.upgrade().ok_or("the connection is closed")?;
            let guard = conn.handle.lock().await;
            let handle = guard.as_ref().ok_or("the connection is closed")?;
            let channel = handle.channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0).await.map_err(|e| e.to_string())?;
            Ok(Box::new(channel.into_stream()) as Box<dyn crate::forward::Stream>)
        })
    }
}

async fn start_forwards(conn: &Arc<Conn>, dest: &Destination) -> Vec<String> {
    use crate::forward::{Event, Forward};
    let mut notes: Vec<String> = dest.forward_errors.iter().map(|e| format!("Ignored {e}")).collect();
    for forward in &dest.forwards {
        if let Forward::Remote { bind, port, host, host_port } = forward {
            let asked = {
                let guard = conn.handle.lock().await;
                match guard.as_ref() {
                    Some(h) => h.tcpip_forward(bind.clone(), *port as u32).await.map_err(|e| e.to_string()),
                    None => Err("the connection is closed".into()),
                }
            };
            match asked {
                Ok(got) => {
                    let bound = if *port == 0 { got } else { *port as u32 };
                    if let Ok(mut t) = conn.targets.lock() {
                        t.insert(bound, (host.clone(), *host_port));
                    }
                    notes.push(format!("The server listens on {bind}:{bound} and forwards to {host}:{host_port}"));
                }
                Err(e) => notes.push(format!("The server refused to listen on {bind}:{port}: {e}")),
            }
            continue;
        }
        let (events, mut seen) = tokio::sync::mpsc::unbounded_channel();
        let running = crate::forward::start(forward.clone(), Arc::new(Tunnel(Arc::downgrade(conn))), events).await;
        while let Ok(event) = seen.try_recv() {
            match event {
                Event::Listening { forward: Forward::Local { bind, host, host_port, .. }, port } => notes.push(format!("Forwarding {bind}:{port} to {host}:{host_port}")),
                Event::Listening { forward: Forward::Dynamic { bind, .. }, port } => notes.push(format!("SOCKS proxy on {bind}:{port}")),
                Event::Failed { forward, error } => notes.push(format!("Cannot forward {}: {error}", describe(&forward))),
                _ => {}
            }
        }
        if let Ok(mut f) = conn.forwards.lock() {
            f.push(running);
        }
    }
    notes
}

fn describe(forward: &crate::forward::Forward) -> String {
    use crate::forward::Forward;
    match forward {
        Forward::Local { bind, port, host, host_port } | Forward::Remote { bind, port, host, host_port } => format!("{bind}:{port} to {host}:{host_port}"),
        Forward::Dynamic { bind, port } => format!("SOCKS on {bind}:{port}"),
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        let handle = self.handle.get_mut().take();
        let jumps = self.jumps.get_mut().map(std::mem::take).unwrap_or_default();
        if let Some(rt) = runtime() {
            rt.spawn(async move {
                if let Some(h) = handle {
                    let _ = h.disconnect(Disconnect::ByApplication, "", "").await;
                }
                for j in jumps.into_iter().rev() {
                    let _ = j.disconnect(Disconnect::ByApplication, "", "").await;
                }
            });
        }
    }
}

impl Conn {
    async fn open_session(&self) -> Result<russh::Channel<client::Msg>, String> {
        let guard = self.handle.lock().await;
        let handle = guard.as_ref().ok_or("the connection is closed")?;
        handle.channel_open_session().await.map_err(|e| format!("cannot open a session: {e}"))
    }

    async fn closed(&self) -> bool {
        self.handle.lock().await.as_ref().is_none_or(|h| h.is_closed())
    }
}

#[derive(Default)]
struct Entry {
    conn: Weak<Conn>,
    gate: Arc<Mutex<()>>,
}

fn pool() -> &'static std::sync::Mutex<HashMap<String, Entry>> {
    static POOL: OnceLock<std::sync::Mutex<HashMap<String, Entry>>> = OnceLock::new();
    POOL.get_or_init(Default::default)
}

fn pool_key(dest: &Destination) -> String {
    format!("{}@{}:{}|{}|{}", dest.user, dest.host, dest.port, dest.proxy_jump.join(","), dest.proxy_command.as_deref().unwrap_or(""))
}

async fn connection(dest: Destination, req: &Request, console: &Shared) -> Result<(Arc<Conn>, bool), String> {
    let fresh = |dest: Destination| async move {
        let (handle, jumps, targets) = open(dest, req, console, 0).await?;
        Ok::<_, String>(Arc::new(Conn { handle: Mutex::new(Some(handle)), jumps: std::sync::Mutex::new(jumps), targets, forwards: std::sync::Mutex::new(Vec::new()) }))
    };
    if !req.share {
        return Ok((fresh(dest).await?, false));
    }
    let key = pool_key(&dest);
    let gate = pool().lock().map_err(|_| "connection pool is broken")?.entry(key.clone()).or_default().gate.clone();
    let _turn = gate.lock().await;
    let existing = pool().lock().ok().and_then(|p| p.get(&key).and_then(|e| e.conn.upgrade()));
    if let Some(conn) = existing {
        if !conn.closed().await {
            return Ok((conn, true));
        }
    }
    let conn = fresh(dest).await?;
    if let Ok(mut p) = pool().lock() {
        p.entry(key).or_default().conn = Arc::downgrade(&conn);
    }
    Ok((conn, false))
}

type Shared = Arc<Mutex<Console>>;

async fn run(req: Request, console: Console, files: crossbeam_channel::Sender<crate::sftp::Reply>) -> u32 {
    let console = Arc::new(Mutex::new(console));
    match session(&req, &console, files).await {
        Ok(code) => code,
        Err(e) => {
            let c = console.lock().await;
            if !c.closed && !e.is_empty() {
                c.print(&format!("\n{e}\n"));
            }
            FAILED
        }
    }
}

fn context_destination(spec: &str, req: &Request) -> Destination {
    let config = std::fs::read_to_string(req.ssh_dir.join("config")).unwrap_or_default();
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok();
    let cx = Context { config: &config, ssh_dir: &req.ssh_dir, home: &req.home, read: &read, default_user: &req.user };
    destination::resolve(spec, &cx)
}

async fn session(req: &Request, console: &Shared, replies: crossbeam_channel::Sender<crate::sftp::Reply>) -> Result<u32, String> {
    let dest = context_destination(&req.destination, req);
    let (mut conn, shared) = connection(dest.clone(), req, console).await?;
    let dest = &dest;
    let channel = match conn.open_session().await {
        Ok(c) => c,
        Err(_) if shared => {
            if let Ok(mut p) = pool().lock() {
                p.remove(&pool_key(dest));
            }
            conn = connection(dest.clone(), req, console).await?.0;
            conn.open_session().await?
        }
        Err(e) => return Err(e),
    };
    if !shared && (!dest.forwards.is_empty() || !dest.forward_errors.is_empty()) {
        let notes = start_forwards(&conn, dest).await;
        let c = console.lock().await;
        for note in notes {
            c.print(&format!("{note}\n"));
        }
    }
    let (mut rx, tx) = channel.split();
    let (cols, rows) = console.lock().await.size;
    tx.request_pty(false, &req.term, cols as u32, rows as u32, 0, 0, &[]).await.map_err(|e| e.to_string())?;
    if forwards_agent(dest, req) {
        let _ = tx.agent_forward(false).await;
    }
    tx.request_shell(false).await.map_err(|e| e.to_string())?;
    let mut code = None;
    let mut files: Option<crate::sftp::Files> = None;
    let mut con = console.lock().await;
    loop {
        tokio::select! {
            msg = rx.wait() => match msg {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => con.write(data.to_vec()),
                Some(ChannelMsg::ExitStatus { exit_status }) => code = Some(exit_status),
                Some(ChannelMsg::ExitSignal { .. }) => code = code.or(Some(FAILED)),
                Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            },
            input = con.next() => match input {
                Some(Input::Data(bytes)) => {
                    if tx.data_bytes(bytes).await.is_err() {
                        break;
                    }
                }
                Some(Input::Resize(c, r)) => {
                    let _ = tx.window_change(c as u32, r as u32, 0, 0).await;
                }
                Some(Input::Sftp(request)) => {
                    if files.is_none() {
                        match open_files(&conn, replies.clone()).await {
                            Ok(f) => files = Some(f),
                            Err(error) => {
                                let _ = replies.send(crate::sftp::Reply::Failed { id: None, error });
                                continue;
                            }
                        }
                    }
                    if let Some(f) = &files {
                        f.handle(request);
                    }
                }
                Some(Input::Close) | None => {
                    let _ = tx.close().await;
                    return Ok(0);
                }
            },
        }
    }
    Ok(code.unwrap_or(0))
}

async fn open_files(conn: &Conn, replies: crossbeam_channel::Sender<crate::sftp::Reply>) -> Result<crate::sftp::Files, String> {
    let channel = conn.open_session().await.map_err(|e| format!("cannot open a file channel: {e}"))?;
    channel.request_subsystem(true, "sftp").await.map_err(|e| format!("the server has no SFTP: {e}"))?;
    let session = russh_sftp::client::SftpSession::new(channel.into_stream()).await.map_err(|e| format!("SFTP failed to start: {e}"))?;
    Ok(crate::sftp::Files::new(session, replies))
}

type Opened = (Handle<Client>, Vec<Handle<Client>>, Targets);

fn open<'a>(dest: Destination, req: &'a Request, console: &'a Shared, depth: usize) -> Pin<Box<dyn Future<Output = Result<Opened, String>> + Send + 'a>> {
    Box::pin(async move {
        if depth > MAX_JUMPS {
            return Err("too many jump hosts".into());
        }
        let handler = Client::new(&dest, req, console.clone());
        let targets = handler.targets.clone();
        let config = Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 4,
            nodelay: true,
            ..Default::default()
        });
        let (mut handle, jumps) = if let Some((last, rest)) = dest.proxy_jump.split_last() {
            let mut via = context_destination(last, req);
            if !rest.is_empty() {
                via.proxy_jump = rest.to_vec();
            }
            let (jump, mut jumps, _) = open(via, req, console, depth + 1).await?;
            let channel = jump
                .channel_open_direct_tcpip(dest.host.clone(), dest.port as u32, "127.0.0.1", 0)
                .await
                .map_err(|e| format!("{}:{} is not reachable through the jump host: {e}", dest.host, dest.port))?;
            let handle = connect(config, channel.into_stream(), handler, &dest).await?;
            jumps.push(jump);
            (handle, jumps)
        } else if let Some(command) = dest.proxy_command_line() {
            (connect(config, proxy_stream(&command)?, handler, &dest).await?, Vec::new())
        } else {
            let addr = (dest.host.clone(), dest.port);
            let tcp = tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(addr))
                .await
                .map_err(|_| format!("connection to {}:{} timed out", dest.host, dest.port))?
                .map_err(|e| format!("cannot connect to {}:{}: {e}", dest.host, dest.port))?;
            (connect(config, tcp, handler, &dest).await?, Vec::new())
        };
        authenticate(&mut handle, &dest, req, console).await?;
        Ok((handle, jumps, targets))
    })
}

async fn connect<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(config: Arc<client::Config>, stream: S, handler: Client, dest: &Destination) -> Result<Handle<Client>, String> {
    client::connect_stream(config, stream, handler).await.map_err(|e| match e {
        russh::Error::UnknownKey => String::new(),
        e => format!("SSH handshake with {} failed: {e}", dest.host),
    })
}

fn proxy_stream(command: &str) -> Result<impl AsyncRead + AsyncWrite + Unpin + Send + 'static, String> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = tokio::process::Command::new("cmd.exe");
        c.arg("/C").arg(command).creation_flags(0x0800_0000);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("cannot run the proxy command: {e}"))?;
    let stdin = child.stdin.take().ok_or("proxy command has no input")?;
    let stdout = child.stdout.take().ok_or("proxy command has no output")?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(tokio::io::join(stdout, stdin))
}

pub struct Client {
    targets: Targets,
    agent: Option<Option<String>>,
    console: Weak<Mutex<Console>>,
    host: String,
    port: u16,
    known_hosts: PathBuf,
    accepted: Option<PublicKey>,
}

impl Client {
    fn new(dest: &Destination, req: &Request, console: Shared) -> Self {
        Self { targets: Targets::default(), agent: forwards_agent(dest, req).then(|| dest.agent.clone()), console: Arc::downgrade(&console), host: dest.host.clone(), port: dest.port, known_hosts: req.ssh_dir.join("known_hosts"), accepted: None }
    }

    fn label(&self) -> String {
        if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        }
    }
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn server_channel_open_forwarded_tcpip(&mut self, channel: russh::Channel<client::Msg>, _address: &str, port: u32, _from: &str, _from_port: u32, reply: client::ChannelOpenHandle, _session: &mut client::Session) -> Result<(), Self::Error> {
        let target = self.targets.lock().ok().and_then(|t| t.get(&port).cloned());
        let Some((host, host_port)) = target else { return Ok(()) };
        let Ok(mut local) = tokio::net::TcpStream::connect((host, host_port)).await else { return Ok(()) };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut local).await;
        });
        Ok(())
    }

    async fn server_channel_open_agent_forward(&mut self, channel: russh::Channel<client::Msg>, reply: client::ChannelOpenHandle, _session: &mut client::Session) -> Result<(), Self::Error> {
        let Some(path) = self.agent.clone() else { return Ok(()) };
        let Ok(mut agent) = local_agent(path.as_deref()).await else { return Ok(()) };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut agent).await;
        });
        Ok(())
    }

    async fn auth_banner(&mut self, banner: &str, _session: &mut client::Session) -> Result<(), Self::Error> {
        if let Some(console) = self.console.upgrade() {
            if let Ok(c) = console.try_lock() {
                c.print(banner);
            }
        }
        Ok(())
    }

    async fn check_server_key(&mut self, key: &russh::keys::PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let console = self.console.upgrade();
        let russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            if let Some(Ok(c)) = console.as_ref().map(|c| c.try_lock()) {
                c.print(&format!("\n{} presented a host certificate; only plain host keys are supported.\n", self.label()));
            }
            return Ok(false);
        };
        if self.accepted.as_ref() == Some(key) {
            return Ok(true);
        }
        let known = russh::keys::check_known_hosts_path(&self.host, self.port, key, &self.known_hosts);
        let fingerprint = key.fingerprint(HashAlg::Sha256);
        match known {
            Ok(true) => {
                self.accepted = Some(key.clone());
                Ok(true)
            }
            Err(russh::keys::Error::KeyChanged { line }) => {
                if let Some(Ok(c)) = console.as_ref().map(|c| c.try_lock()) {
                    c.print(&format!(
                        "\nWARNING: the host key of {} has changed.\nSomeone could be intercepting the connection, or the host key was replaced.\nThe {} key fingerprint is {fingerprint}.\nThe stored key is on line {line} of {}.\nRemove that line to connect anyway.\n",
                        self.label(),
                        key.algorithm(),
                        self.known_hosts.display()
                    ));
                }
                Ok(false)
            }
            _ => {
                let Some(Ok(mut c)) = console.as_ref().map(|c| c.try_lock()) else { return Ok(false) };
                let prompt = format!(
                    "The authenticity of host '{}' can't be established.\n{} key fingerprint is {fingerprint}.\nAre you sure you want to continue connecting (yes/no)? ",
                    self.label(),
                    key.algorithm()
                );
                loop {
                    match c.ask(&prompt, true).await.map(|a| a.trim().to_ascii_lowercase()) {
                        Some(a) if a == "yes" => break,
                        Some(a) if a == "no" => return Ok(false),
                        Some(_) => continue,
                        None => return Ok(false),
                    }
                }
                if let Some(dir) = self.known_hosts.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                match russh::keys::known_hosts::learn_known_hosts_path(&self.host, self.port, key, &self.known_hosts) {
                    Ok(()) => c.print(&format!("Permanently added '{}' to the list of known hosts.\n", self.label())),
                    Err(e) => c.print(&format!("Could not save the host key: {e}\n")),
                }
                self.accepted = Some(key.clone());
                Ok(true)
            }
        }
    }
}

fn note(r: &AuthResult, methods: &mut Option<MethodSet>) -> bool {
    match r {
        AuthResult::Success => true,
        AuthResult::Failure { remaining_methods, .. } => {
            *methods = Some(remaining_methods.clone());
            false
        }
    }
}

fn allows(methods: &Option<MethodSet>, kind: MethodKind) -> bool {
    methods.as_ref().is_none_or(|m| m.contains(&kind))
}

async fn authenticate(handle: &mut Handle<Client>, dest: &Destination, req: &Request, console: &Shared) -> Result<(), String> {
    let user = dest.user.clone();
    let rsa = match handle.best_supported_rsa_hash().await {
        Ok(Some(h)) => h,
        _ => Some(HashAlg::Sha256),
    };
    let mut methods: Option<MethodSet> = None;

    if req.use_agent && allows(&methods, MethodKind::PublicKey) {
        if let Some(r) = agent_auth(handle, &user, rsa, dest.agent.as_deref()).await {
            if note(&r, &mut methods) {
                return Ok(());
            }
        }
    }

    if allows(&methods, MethodKind::PublicKey) {
        for path in &dest.identity_files {
            if !path.is_file() {
                if dest.explicit_identities {
                    console.lock().await.print(&format!("Identity file {} not accessible.\n", path.display()));
                }
                continue;
            }
            let Some(key) = load_key(path, console, req).await else { continue };
            let attempt = handle.authenticate_publickey(user.clone(), PrivateKeyWithHashAlg::new(Arc::new(key), rsa)).await.map_err(|e| e.to_string())?;
            if note(&attempt, &mut methods) {
                return Ok(());
            }
        }
    }

    if allows(&methods, MethodKind::KeyboardInteractive) {
        let mut reply = handle.authenticate_keyboard_interactive_start(user.clone(), None).await.map_err(|e| e.to_string())?;
        let mut rounds = 0;
        loop {
            match reply {
                KeyboardInteractiveAuthResponse::Success => return Ok(()),
                KeyboardInteractiveAuthResponse::Failure { remaining_methods, .. } => {
                    methods = Some(remaining_methods);
                    break;
                }
                KeyboardInteractiveAuthResponse::InfoRequest { name, instructions, prompts } => {
                    rounds += 1;
                    if rounds > 10 {
                        break;
                    }
                    let mut c = console.lock().await;
                    for text in [name, instructions] {
                        if !text.trim().is_empty() {
                            c.print(&format!("{}\n", text.trim_end()));
                        }
                    }
                    let mut answers = Vec::new();
                    for p in prompts {
                        answers.push(c.ask(&p.prompt, p.echo).await.ok_or_else(String::new)?);
                    }
                    drop(c);
                    reply = handle.authenticate_keyboard_interactive_respond(answers).await.map_err(|e| e.to_string())?;
                }
            }
        }
    }

    if allows(&methods, MethodKind::Password) {
        let key = crate::secrets::password_key(&user, &dest.host, dest.port);
        if let Some(stored) = req.secrets.as_ref().and_then(|s| s.get(&key)) {
            let r = handle.authenticate_password(user.clone(), stored).await.map_err(|e| e.to_string())?;
            if note(&r, &mut methods) {
                return Ok(());
            }
            if let Some(s) = &req.secrets {
                s.delete(&key);
            }
            console.lock().await.print("The saved password was not accepted.\n");
        }
        for attempt in 0..3 {
            let mut c = console.lock().await;
            if attempt > 0 {
                c.print("Permission denied, please try again.\n");
            }
            let password = c.ask(&format!("{user}@{}'s password: ", dest.host), false).await.ok_or_else(String::new)?;
            drop(c);
            let r = handle.authenticate_password(user.clone(), password.clone()).await.map_err(|e| e.to_string())?;
            if note(&r, &mut methods) {
                offer_to_save(req, console, &key, &password, &format!("the password for {user}@{}", dest.host)).await;
                return Ok(());
            }
        }
    }
    Err(format!("{user}@{}: permission denied", dest.host))
}

async fn load_key(path: &std::path::Path, console: &Shared, req: &Request) -> Option<russh::keys::PrivateKey> {
    match russh::keys::load_secret_key(path, None) {
        Ok(k) => return Some(k),
        Err(russh::keys::Error::KeyIsEncrypted) => {}
        Err(e) => {
            console.lock().await.print(&format!("Cannot read {}: {e}\n", path.display()));
            return None;
        }
    }
    let saved = crate::secrets::passphrase_key(path);
    if let Some(stored) = req.secrets.as_ref().and_then(|s| s.get(&saved)) {
        if let Ok(k) = russh::keys::load_secret_key(path, Some(&stored)) {
            return Some(k);
        }
        if let Some(s) = &req.secrets {
            s.delete(&saved);
        }
    }
    for _ in 0..3 {
        let mut c = console.lock().await;
        let pass = c.ask(&format!("Enter passphrase for key '{}': ", path.display()), false).await?;
        if pass.is_empty() {
            return None;
        }
        match russh::keys::load_secret_key(path, Some(&pass)) {
            Ok(k) => {
                drop(c);
                offer_to_save(req, console, &saved, &pass, &format!("the passphrase for {}", path.display())).await;
                return Some(k);
            }
            Err(_) => c.print("Bad passphrase, try again.\n"),
        }
    }
    None
}

async fn offer_to_save(req: &Request, console: &Shared, key: &str, value: &str, what: &str) {
    let Some(secrets) = req.secrets.as_ref().filter(|_| req.remember) else { return };
    let mut c = console.lock().await;
    let prompt = format!("Save {what} in the system credential store (yes/no)? ");
    loop {
        match c.ask(&prompt, true).await.map(|a| a.trim().to_ascii_lowercase()) {
            Some(a) if a == "yes" || a == "y" => {
                if !secrets.set(key, value) {
                    c.print("Could not save it.\n");
                }
                return;
            }
            Some(a) if a == "no" || a == "n" || a.is_empty() => return,
            Some(_) => continue,
            None => return,
        }
    }
}

fn forwards_agent(dest: &Destination, req: &Request) -> bool {
    dest.forward_agent.unwrap_or(req.forward_agent) && dest.agent.as_deref() != Some("none")
}

#[cfg(windows)]
const AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

trait AgentStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AgentStream for T {}

async fn local_agent(path: Option<&str>) -> std::io::Result<Box<dyn AgentStream>> {
    #[cfg(windows)]
    {
        let pipe = tokio::net::windows::named_pipe::ClientOptions::new().open(path.unwrap_or(AGENT_PIPE))?;
        Ok(Box::new(pipe))
    }
    #[cfg(not(windows))]
    {
        let socket = match path {
            Some(p) => p.to_owned(),
            None => std::env::var("SSH_AUTH_SOCK").map_err(|_| std::io::Error::from(std::io::ErrorKind::NotFound))?,
        };
        Ok(Box::new(tokio::net::UnixStream::connect(socket).await?))
    }
}

async fn agent_auth(handle: &mut Handle<Client>, user: &str, rsa: Option<HashAlg>, path: Option<&str>) -> Option<AuthResult> {
    if path == Some("none") {
        return None;
    }
    #[cfg(windows)]
    {
        if let Ok(agent) = AgentClient::connect_named_pipe(path.unwrap_or(AGENT_PIPE)).await {
            if let Some(r) = try_agent(handle, user, rsa, agent).await {
                return Some(r);
            }
        }
        if path.is_some() {
            return None;
        }
        if let Ok(agent) = AgentClient::connect_pageant().await {
            return try_agent(handle, user, rsa, agent).await;
        }
        None
    }
    #[cfg(not(windows))]
    {
        let agent = match path {
            Some(p) => AgentClient::connect_uds(p).await.ok()?,
            None => AgentClient::connect_env().await.ok()?,
        };
        try_agent(handle, user, rsa, agent).await
    }
}

async fn try_agent<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(handle: &mut Handle<Client>, user: &str, rsa: Option<HashAlg>, mut agent: AgentClient<S>) -> Option<AuthResult> {
    let identities = agent.request_identities().await.ok()?;
    let mut last = None;
    for id in identities {
        let AgentIdentity::PublicKey { key, .. } = id else { continue };
        let hash = if key.algorithm().is_rsa() { rsa } else { None };
        match handle.authenticate_publickey_with(user, key, hash, &mut agent).await {
            Ok(AuthResult::Success) => return Some(AuthResult::Success),
            Ok(r) => last = Some(r),
            Err(_) => {}
        }
    }
    last
}

#[cfg(test)]
#[path = "../test/client.rs"]
mod tests;
