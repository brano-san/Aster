use aster_core::profile::Stream;
use aster_core::session::{NodeState, WindowState};
use aster_platform::host::persistent::{Client, Host, SessionInfo};
use aster_platform::{Platform, PlatformError, SpawnRequest, Spawned};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(100);
const START_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Idle(Option<Instant>);

impl Idle {
    fn expired(&mut self, now: Instant, empty: bool) -> bool {
        if !empty {
            self.0 = None;
            return false;
        }
        now.duration_since(*self.0.get_or_insert(now)) >= IDLE_TIMEOUT
    }
}

struct ClientCount(Arc<AtomicUsize>);

impl ClientCount {
    fn enter(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::Relaxed);
        Self(count.clone())
    }
}

impl Drop for ClientCount {
    fn drop(&mut self) { self.0.fetch_sub(1, Ordering::Release); }
}

fn token_path<P: Platform>() -> io::Result<PathBuf> {
    crate::config_dir::<P>().map(|dir| dir.join("pty-host.token")).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "configuration directory unavailable"))
}

fn publish_token<P: Platform>(token: &str) -> io::Result<()> {
    let path = token_path::<P>()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let temp = path.with_extension(format!("{}.{nonce}.tmp", std::process::id()));
    let mut file = P::create_pty_host_token(&temp)?;
    let result = (|| {
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, &path)
    })();
    if result.is_err() { let _ = std::fs::remove_file(temp); }
    result
}

fn connect<P: Platform>(start: bool) -> io::Result<Client> {
    let endpoint = P::pty_host_endpoint()?;
    let path = token_path::<P>()?;
    let attempt = || {
        let connection = P::connect_pty_host(&endpoint)?;
        let mut token = String::with_capacity(64);
        std::fs::File::open(&path)?.take(65).read_to_string(&mut token)?;
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid terminal host token"));
        }
        Client::connect(connection, &token)
    };
    match attempt() {
        Err(error) if start && error.kind() != io::ErrorKind::PermissionDenied => {}
        result => return result,
    }
    P::start_pty_host(&endpoint)?;
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match attempt() {
            Err(error) if error.kind() != io::ErrorKind::PermissionDenied && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(30)),
            result => return result,
        }
    }
}

pub fn kill_detached<P: Platform>(id: u64) {
    if let Err(error) = connect::<P>(false).and_then(|mut client| client.kill_detached(id)) {
        if !matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) {
            tracing::warn!("could not close detached terminal session: {error}");
        }
    }
}

#[cfg(windows)]
pub fn close_for_uninstall<P: Platform>() -> io::Result<()> {
    let mut client = match connect::<P>(false) {
        Ok(client) => client,
        Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => return Ok(()),
        Err(error) => return Err(error),
    };
    for session in client.list()? {
        client.kill(session.id)?;
    }
    Ok(())
}

fn match_sessions(saved: &[Option<u64>], live: Vec<SessionInfo>) -> (Vec<Option<SessionInfo>>, Vec<u64>) {
    let mut live: HashMap<_, _> = live.into_iter().map(|info| (info.id, info)).collect();
    let mut panes = Vec::with_capacity(saved.len());
    for id in saved { panes.push(id.and_then(|id| live.remove(&id))); }
    let mut kill: Vec<_> = live.into_keys().collect();
    kill.sort_unstable();
    (panes, kill)
}

fn saved_ids(node: &NodeState, ids: &mut Vec<Option<u64>>) {
    match node {
        NodeState::Pane(pane) => ids.push(pane.host_id.filter(|_| pane.profile.is_local() && !pane.profile.elevated)),
        NodeState::Split { a, b, .. } => {
            saved_ids(a, ids);
            saved_ids(b, ids);
        }
    }
}

pub fn restore<P: Platform>(windows: &[WindowState]) -> io::Result<HashMap<u64, SessionInfo>> {
    let mut client = connect::<P>(false)?;
    let mut ids = Vec::new();
    for tab in windows.iter().flat_map(|window| &window.tabs) { saved_ids(&tab.root, &mut ids); }
    let (panes, kill) = match_sessions(&ids, client.list()?);
    for id in kill {
        if let Err(error) = client.kill_detached(id) {
            tracing::warn!("could not close orphaned terminal session: {error}");
            break;
        }
    }
    Ok(panes.into_iter().flatten().map(|info| (info.id, info)).collect())
}

pub struct Hosted {
    pub spawned: Spawned,
    pub replay: Vec<u8>,
    pub previous_size: Option<(u16, u16)>,
}

pub enum SpawnError {
    Unavailable(io::Error),
    Failed(io::Error),
}

pub fn spawn<P: Platform>(request: &SpawnRequest, title: &str, stream: Option<&Stream>, previous: Option<SessionInfo>) -> Result<Hosted, SpawnError> {
    if let Some(info) = previous {
        match connect::<P>(false).and_then(|client| client.attach(info.id)) {
            Ok((spawned, replay)) => return Ok(Hosted { spawned, replay, previous_size: Some((info.cols, info.rows)) }),
            Err(error) => tracing::warn!("could not reattach local shell: {error}"),
        }
    }
    let mut client = connect::<P>(true).map_err(SpawnError::Unavailable)?;
    let options = stream.map(serde_json::to_string).transpose().map_err(|error| SpawnError::Failed(error.into()))?.unwrap_or_default();
    let id = client.spawn(request, title, &options).map_err(SpawnError::Failed)?;
    match client.attach(id) {
        Ok((spawned, replay)) => Ok(Hosted { spawned, replay, previous_size: None }),
        Err(error) => {
            if let Err(kill_error) = connect::<P>(false).and_then(|mut client| client.kill(id)) {
                tracing::warn!("could not close an unclaimed terminal session: {kill_error}");
            }
            Err(SpawnError::Failed(error))
        }
    }
}

pub fn run<P: Platform>(endpoint: &str) -> i32 {
    let path = crate::config_dir::<P>().map(|dir| dir.join("aster-pty-host.log"));
    let log = crate::log::open(path.as_deref());
    tracing_subscriber::fmt().with_ansi(false).with_writer(move || log.clone()).with_max_level(tracing::Level::INFO).init();
    match serve::<P>(endpoint) {
        Ok(()) => 0,
        Err(error) => {
            tracing::error!("persistent terminal host: {error}");
            1
        }
    }
}

fn serve<P: Platform>(endpoint: &str) -> io::Result<()> {
    let mut listener = match P::listen_pty_host(endpoint) {
        Ok(listener) => listener,
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut random = [0; 40];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let mut token = String::with_capacity(64);
    for byte in &random[..32] { write!(&mut token, "{byte:02x}").unwrap(); }
    publish_token::<P>(&token)?;
    let token: Arc<str> = Arc::from(token);
    let host = Arc::new(Host::new(u64::from_le_bytes(random[32..].try_into().unwrap())));
    let clients = Arc::new(AtomicUsize::new(0));
    let mut idle = Idle::default();
    tracing::info!("persistent terminal host started");
    loop {
        if let Some(connection) = listener.accept(POLL)? {
            let client = ClientCount::enter(&clients);
            let host = host.clone();
            let token = token.clone();
            std::thread::Builder::new().name("aster-pty-host-client".into()).spawn(move || {
                let _client = client;
                let spawn = |request: &SpawnRequest, options: &str| {
                    let stream: Option<Stream> = if options.is_empty() { None } else {
                        Some(serde_json::from_str(options).map_err(|error| PlatformError::new(-1, format!("invalid terminal stream options: {error}")))?)
                    };
                    P::spawn(request).map(|spawned| crate::stream::wrap(spawned, stream.as_ref()))
                };
                if let Err(error) = host.serve(connection, &token, &spawn) {
                    if !matches!(error.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof | io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) {
                        tracing::warn!("persistent terminal host connection: {error}");
                    }
                }
            })?;
        }
        let empty = clients.load(Ordering::Acquire) == 0 && host.is_empty();
        if idle.expired(Instant::now(), empty) {
            tracing::info!("persistent terminal host stopped after idle timeout");
            return Ok(());
        }
    }
}

#[cfg(test)]
#[path = "../test/pty_host.rs"]
mod tests;
