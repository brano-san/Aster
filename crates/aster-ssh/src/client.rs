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
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;

pub const FAILED: u32 = 255;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_JUMPS: usize = 8;

#[derive(Clone, Debug)]
pub struct Request {
    pub destination: String,
    pub cols: u16,
    pub rows: u16,
    pub term: String,
    pub ssh_dir: PathBuf,
    pub home: PathBuf,
    pub user: String,
    pub use_agent: bool,
}

pub struct Connection {
    pub output: Receiver<Vec<u8>>,
    pub input: UnboundedSender<Input>,
    pub exit: Receiver<u32>,
}

pub fn start(req: Request) -> Connection {
    let (out_tx, output) = crossbeam_channel::unbounded();
    let (input, in_rx) = tokio::sync::mpsc::unbounded_channel();
    let (exit_tx, exit) = crossbeam_channel::bounded(1);
    let spawned = std::thread::Builder::new().name("aster-ssh".into()).spawn(move || {
        let console = Console::new(out_tx, in_rx, (req.cols, req.rows));
        let code = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => {
                let code = rt.block_on(run(req, console));
                drop(rt);
                code
            }
            Err(e) => {
                console.print(&format!("\ncannot start the SSH client: {e}\n"));
                FAILED
            }
        };
        let _ = exit_tx.send(code);
    });
    if let Err(e) = spawned {
        tracing::error!("ssh thread: {e}");
    }
    Connection { output, input, exit }
}

type Shared = Arc<Mutex<Console>>;

async fn run(req: Request, console: Console) -> u32 {
    let console = Arc::new(Mutex::new(console));
    match session(&req, &console).await {
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

async fn session(req: &Request, console: &Shared) -> Result<u32, String> {
    let dest = context_destination(&req.destination, req);
    let (handle, _jumps) = open(dest, req, console, 0).await?;
    let channel = handle.channel_open_session().await.map_err(|e| format!("cannot open a session: {e}"))?;
    let (mut rx, tx) = channel.split();
    let (cols, rows) = console.lock().await.size;
    tx.request_pty(false, &req.term, cols as u32, rows as u32, 0, 0, &[]).await.map_err(|e| e.to_string())?;
    tx.request_shell(false).await.map_err(|e| e.to_string())?;
    let mut code = None;
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
                Some(Input::Close) | None => {
                    let _ = tx.close().await;
                    let _ = handle.disconnect(Disconnect::ByApplication, "", "").await;
                    return Ok(0);
                }
            },
        }
    }
    let _ = handle.disconnect(Disconnect::ByApplication, "", "").await;
    Ok(code.unwrap_or(0))
}

type Opened = (Handle<Client>, Vec<Handle<Client>>);

fn open<'a>(dest: Destination, req: &'a Request, console: &'a Shared, depth: usize) -> Pin<Box<dyn Future<Output = Result<Opened, String>> + Send + 'a>> {
    Box::pin(async move {
        if depth > MAX_JUMPS {
            return Err("too many jump hosts".into());
        }
        let handler = Client::new(&dest, req, console.clone());
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
            let (jump, mut jumps) = open(via, req, console, depth + 1).await?;
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
        Ok((handle, jumps))
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
    console: Shared,
    host: String,
    port: u16,
    known_hosts: PathBuf,
    accepted: Option<PublicKey>,
}

impl Client {
    fn new(dest: &Destination, req: &Request, console: Shared) -> Self {
        Self { console, host: dest.host.clone(), port: dest.port, known_hosts: req.ssh_dir.join("known_hosts"), accepted: None }
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

    async fn auth_banner(&mut self, banner: &str, _session: &mut client::Session) -> Result<(), Self::Error> {
        if let Ok(c) = self.console.try_lock() {
            c.print(banner);
        }
        Ok(())
    }

    async fn check_server_key(&mut self, key: &russh::keys::PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            if let Ok(c) = self.console.try_lock() {
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
                if let Ok(c) = self.console.try_lock() {
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
                let Ok(mut c) = self.console.try_lock() else { return Ok(false) };
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
        if let Some(r) = agent_auth(handle, &user, rsa).await {
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
            let Some(key) = load_key(path, console).await else { continue };
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
        for attempt in 0..3 {
            let mut c = console.lock().await;
            if attempt > 0 {
                c.print("Permission denied, please try again.\n");
            }
            let password = c.ask(&format!("{user}@{}'s password: ", dest.host), false).await.ok_or_else(String::new)?;
            drop(c);
            let r = handle.authenticate_password(user.clone(), password).await.map_err(|e| e.to_string())?;
            if note(&r, &mut methods) {
                return Ok(());
            }
        }
    }
    Err(format!("{user}@{}: permission denied", dest.host))
}

async fn load_key(path: &std::path::Path, console: &Shared) -> Option<russh::keys::PrivateKey> {
    match russh::keys::load_secret_key(path, None) {
        Ok(k) => return Some(k),
        Err(russh::keys::Error::KeyIsEncrypted) => {}
        Err(e) => {
            console.lock().await.print(&format!("Cannot read {}: {e}\n", path.display()));
            return None;
        }
    }
    let mut c = console.lock().await;
    for _ in 0..3 {
        let pass = c.ask(&format!("Enter passphrase for key '{}': ", path.display()), false).await?;
        if pass.is_empty() {
            return None;
        }
        match russh::keys::load_secret_key(path, Some(&pass)) {
            Ok(k) => return Some(k),
            Err(_) => c.print("Bad passphrase, try again.\n"),
        }
    }
    None
}

async fn agent_auth(handle: &mut Handle<Client>, user: &str, rsa: Option<HashAlg>) -> Option<AuthResult> {
    #[cfg(windows)]
    {
        if let Ok(agent) = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
            if let Some(r) = try_agent(handle, user, rsa, agent).await {
                return Some(r);
            }
        }
        if let Ok(agent) = AgentClient::connect_pageant().await {
            return try_agent(handle, user, rsa, agent).await;
        }
        None
    }
    #[cfg(not(windows))]
    {
        let agent = AgentClient::connect_env().await.ok()?;
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
