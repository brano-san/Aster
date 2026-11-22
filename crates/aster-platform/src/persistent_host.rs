use super::{decode_request, encode_request, put, read_frame, take, write_frame, Reader, Writer, DATA, ERROR, EXIT, HELLO, KILL, READY, RESIZE, SPAWN};
use crate::{PlatformError, Pty, SpawnRequest, Spawned};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, sync_channel, Sender, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const ATTACH: u8 = 8;
const DETACH: u8 = 9;
const LIST: u8 = 10;
const SESSIONS: u8 = 11;
const REPLAY: u8 = 12;
const RAW_DATA: u8 = 13;
const FORCE_RESIZE: u8 = 14;
const KILL_DETACHED: u8 = 15;
const REPLAY_LIMIT: usize = 2 * 1024 * 1024;
const OUTPUT_QUEUE: usize = 64;

pub struct Connection {
    reader: Reader,
    writer: Writer,
    streaming: Box<dyn FnOnce() -> io::Result<()> + Send>,
    abort: Box<dyn FnOnce() + Send>,
}

impl Connection {
    pub fn new(reader: Reader, writer: Writer, streaming: impl FnOnce() -> io::Result<()> + Send + 'static) -> Self {
        Self { reader, writer, streaming: Box::new(streaming), abort: Box::new(|| {}) }
    }

    pub fn with_abort(mut self, abort: impl FnOnce() + Send + 'static) -> Self {
        self.abort = Box::new(abort);
        self
    }
}

pub trait Listener {
    fn accept(&mut self, timeout: Duration) -> io::Result<Option<Connection>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: u64,
    pub pid: u32,
    pub title: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
}

fn number<const N: usize>(input: &mut &[u8]) -> Option<[u8; N]> {
    let (head, tail) = input.split_at_checked(N)?;
    *input = tail;
    head.try_into().ok()
}

fn encode_sessions(sessions: &[SessionInfo]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for session in sessions {
        bytes.extend(session.id.to_le_bytes());
        bytes.extend(session.pid.to_le_bytes());
        put(&mut bytes, &session.title);
        put(&mut bytes, &session.cwd);
        bytes.extend(session.cols.to_le_bytes());
        bytes.extend(session.rows.to_le_bytes());
    }
    bytes
}

fn decode_sessions(mut bytes: &[u8]) -> Option<Vec<SessionInfo>> {
    let mut sessions = Vec::new();
    while !bytes.is_empty() {
        sessions.push(SessionInfo {
            id: u64::from_le_bytes(number(&mut bytes)?),
            pid: u32::from_le_bytes(number(&mut bytes)?),
            title: take(&mut bytes)?,
            cwd: take(&mut bytes)?,
            cols: u16::from_le_bytes(number(&mut bytes)?),
            rows: u16::from_le_bytes(number(&mut bytes)?),
        });
    }
    Some(sessions)
}

#[derive(Default)]
struct Replay(VecDeque<u8>);

impl Replay {
    fn push(&mut self, bytes: &[u8]) {
        let bytes = &bytes[bytes.len().saturating_sub(REPLAY_LIMIT)..];
        self.0.drain(..(self.0.len() + bytes.len()).saturating_sub(REPLAY_LIMIT));
        self.0.extend(bytes);
    }

    fn snapshot(&self) -> Vec<u8> {
        let (head, tail) = self.0.as_slices();
        [head, tail].concat()
    }
}

struct State {
    info: SessionInfo,
    replay: Replay,
    attached: HashMap<u64, SyncSender<Arc<[u8]>>>,
    ended: bool,
    exit: Option<u32>,
}

struct Entry {
    pty: Box<dyn Pty>,
    input: Sender<(u8, Vec<u8>)>,
    state: Mutex<State>,
}

type Spawn<'a> = dyn Fn(&SpawnRequest, &str) -> Result<Spawned, PlatformError> + 'a;

pub struct Host {
    next: AtomicU64,
    sessions: Mutex<HashMap<u64, Arc<Entry>>>,
}

impl Host {
    pub fn new(first_id: u64) -> Self {
        Self { next: AtomicU64::new(first_id), sessions: Mutex::new(HashMap::new()) }
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.lock().unwrap().is_empty()
    }

    fn get(&self, id: u64) -> Option<Arc<Entry>> {
        self.sessions.lock().unwrap().get(&id).cloned()
    }

    fn list(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.lock().unwrap();
        sessions.values().filter_map(|entry| {
            let state = entry.state.lock().unwrap();
            (!state.ended).then(|| state.info.clone())
        }).collect()
    }

    fn spawn(self: &Arc<Self>, req: SpawnRequest, title: String, options: String, spawn: &Spawn<'_>) -> Result<(u64, Arc<Entry>), PlatformError> {
        let Spawned { pty, mut reader, mut writer, mut raw, wait } = spawn(&req, &options)?;
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let info = SessionInfo { id, pid: pty.pid(), title, cwd: req.cwd.map(|cwd| cwd.to_string_lossy().into_owned()).unwrap_or_default(), cols: req.cols, rows: req.rows };
        let (input, incoming) = channel::<(u8, Vec<u8>)>();
        let entry = Arc::new(Entry { pty, input, state: Mutex::new(State { info, replay: Replay::default(), attached: HashMap::new(), ended: false, exit: None }) });
        self.sessions.lock().unwrap().insert(id, entry.clone());
        let (drained, drained_rx) = channel::<()>();
        let output = entry.clone();
        let read = std::thread::Builder::new().name("aster-persistent-output".into()).spawn(move || {
            let _drained = drained;
            let mut buf = vec![0; 64 * 1024];
            while let Ok(n @ 1..) = reader.read(&mut buf) {
                let mut state = output.state.lock().unwrap();
                state.replay.push(&buf[..n]);
                if !state.attached.is_empty() {
                    let bytes: Arc<[u8]> = Arc::from(&buf[..n]);
                    state.attached.retain(|_, tx| tx.try_send(bytes.clone()).is_ok());
                }
            }
        });
        let host = self.clone();
        let ending = entry.clone();
        let waiter = std::thread::Builder::new().name("aster-persistent-wait".into()).spawn(move || {
            let code = wait();
            let _ = drained_rx.recv_timeout(Duration::from_secs(2));
            let attached = {
                let mut state = ending.state.lock().unwrap();
                state.ended = true;
                state.exit = code;
                std::mem::take(&mut state.attached)
            };
            host.sessions.lock().unwrap().remove(&id);
            drop(attached);
        });
        let writing = std::thread::Builder::new().name("aster-persistent-input".into()).spawn(move || {
            for (kind, bytes) in incoming {
                let target = if kind == RAW_DATA { raw.as_mut().unwrap_or(&mut writer) } else { &mut writer };
                if target.write_all(&bytes).and_then(|()| target.flush()).is_err() { break; }
            }
        });
        if let Err(e) = read.and(waiter).and(writing) {
            self.sessions.lock().unwrap().remove(&id);
            entry.pty.kill();
            return Err(PlatformError::new(-1, e.to_string()));
        }
        Ok((id, entry))
    }

    pub fn serve(self: &Arc<Self>, connection: Connection, token: &str, spawn: &Spawn<'_>) -> io::Result<()> {
        let Connection { mut reader, mut writer, streaming, abort } = connection;
        match read_frame(&mut *reader)? {
            Some((HELLO, given)) if given == token.as_bytes() => write_frame(&mut *writer, READY, &[])?,
            None => return Ok(()),
            _ => return Err(io::Error::new(io::ErrorKind::PermissionDenied, "wrong host token")),
        }
        let mut spawned_here = HashMap::new();
        while let Some((kind, payload)) = read_frame(&mut *reader)? {
            match (kind, payload.as_slice()) {
                (LIST, []) => write_frame(&mut *writer, SESSIONS, &encode_sessions(&self.list()))?,
                (SPAWN, mut bytes) => {
                    let result = take(&mut bytes).zip(take(&mut bytes)).zip(decode_request(bytes))
                        .ok_or_else(|| PlatformError::new(-1, "bad spawn request"))
                        .and_then(|((title, options), req)| self.spawn(req, title, options, spawn));
                    match result {
                        Ok((id, entry)) => {
                            spawned_here.insert(id, entry);
                            write_frame(&mut *writer, READY, &id.to_le_bytes())?;
                        }
                        Err(e) => write_frame(&mut *writer, ERROR, e.message.as_bytes())?,
                    }
                }
                (ATTACH | KILL | KILL_DETACHED, bytes) if bytes.len() == 8 => {
                    let id = u64::from_le_bytes(bytes.try_into().unwrap());
                    if kind != ATTACH {
                        if let Some(entry) = self.get(id) {
                            let mut state = entry.state.lock().unwrap();
                            if !state.ended && (kind == KILL || state.attached.is_empty()) {
                                state.ended = true;
                                drop(state);
                                entry.pty.kill();
                            }
                        }
                        write_frame(&mut *writer, READY, &[])?;
                    } else {
                        let fresh = spawned_here.remove(&id);
                        let first_attach = fresh.is_some();
                        if let Some(entry) = fresh.or_else(|| self.get(id)) {
                            streaming()?;
                            return self.attach(id, entry, reader, writer, abort, first_attach);
                        }
                        write_frame(&mut *writer, ERROR, b"terminal session has ended")?;
                    }
                }
                _ => write_frame(&mut *writer, ERROR, b"unexpected terminal host request")?,
            }
        }
        Ok(())
    }

    fn attach(&self, id: u64, entry: Arc<Entry>, mut reader: Reader, mut writer: Writer, abort: Box<dyn FnOnce() + Send>, first_attach: bool) -> io::Result<()> {
        let connection = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = sync_channel(OUTPUT_QUEUE);
        let snapshot = {
            let mut state = entry.state.lock().unwrap();
            if state.ended && !first_attach {
                None
            } else {
                let ended = state.ended.then_some(state.exit);
                if ended.is_none() { state.attached.insert(connection, tx); }
                Some((state.info.pid, state.replay.snapshot(), ended))
            }
        };
        let Some((pid, replay, ended)) = snapshot else { return write_frame(&mut *writer, ERROR, b"terminal session has ended") };
        if let Some(code) = ended {
            write_frame(&mut *writer, READY, &pid.to_le_bytes())?;
            write_frame(&mut *writer, REPLAY, &replay)?;
            return write_frame(&mut *writer, EXIT, &code.unwrap_or(u32::MAX).to_le_bytes());
        }
        let output_entry = entry.clone();
        let output = std::thread::Builder::new().name("aster-persistent-attach".into()).spawn(move || {
            let write = || {
                write_frame(&mut *writer, READY, &pid.to_le_bytes())?;
                write_frame(&mut *writer, REPLAY, &replay)?;
                for bytes in rx {
                    write_frame(&mut *writer, DATA, &bytes)?;
                }
                let ended = {
                    let state = output_entry.state.lock().unwrap();
                    state.ended.then_some(state.exit)
                };
                match ended {
                    Some(code) => write_frame(&mut *writer, EXIT, &code.unwrap_or(u32::MAX).to_le_bytes()),
                    None => write_frame(&mut *writer, ERROR, b"terminal attachment closed"),
                }
            };
            if write().is_err() { abort(); }
        });
        let result = output.and_then(|_| Self::attached(id, &entry, &mut *reader));
        entry.state.lock().unwrap().attached.remove(&connection);
        result
    }

    fn attached(id: u64, entry: &Entry, reader: &mut dyn Read) -> io::Result<()> {
        while let Some((kind, payload)) = read_frame(reader)? {
            match (kind, payload.as_slice()) {
                (DATA | RAW_DATA, _) => entry.input.send((kind, payload)).map_err(|_| io::ErrorKind::BrokenPipe)?,
                (RESIZE | FORCE_RESIZE, &[a, b, c, d]) => {
                    let (cols, rows) = (u16::from_le_bytes([a, b]), u16::from_le_bytes([c, d]));
                    {
                        let mut state = entry.state.lock().unwrap();
                        (state.info.cols, state.info.rows) = (cols, rows);
                    }
                    if kind == FORCE_RESIZE { entry.pty.force_resize(cols, rows); }
                    else { entry.pty.resize(cols, rows); }
                }
                (KILL, bytes) if bytes == id.to_le_bytes() => entry.pty.kill(),
                (DETACH, bytes) if bytes == id.to_le_bytes() => break,
                _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected terminal attachment request")),
            }
        }
        Ok(())
    }
}

pub struct Client {
    connection: Connection,
}

impl Client {
    pub fn connect(mut connection: Connection, token: &str) -> io::Result<Self> {
        write_frame(&mut *connection.writer, HELLO, token.as_bytes())?;
        let mut client = Self { connection };
        client.expect(READY)?;
        Ok(client)
    }

    fn expect(&mut self, expected: u8) -> io::Result<Vec<u8>> {
        match read_frame(&mut *self.connection.reader)? {
            Some((kind, payload)) if kind == expected => Ok(payload),
            Some((ERROR, payload)) => Err(io::Error::other(String::from_utf8_lossy(&payload).into_owned())),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected terminal host response")),
        }
    }

    pub fn spawn(&mut self, req: &SpawnRequest, title: &str, options: &str) -> io::Result<u64> {
        let mut payload = Vec::new();
        put(&mut payload, title);
        put(&mut payload, options);
        payload.extend(encode_request(req));
        write_frame(&mut *self.connection.writer, SPAWN, &payload)?;
        let id = self.expect(READY)?;
        id.try_into().map(u64::from_le_bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad terminal session id"))
    }

    pub fn list(&mut self) -> io::Result<Vec<SessionInfo>> {
        write_frame(&mut *self.connection.writer, LIST, &[])?;
        decode_sessions(&self.expect(SESSIONS)?).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad terminal session list"))
    }

    pub fn kill(&mut self, id: u64) -> io::Result<()> {
        self.kill_session(KILL, id)
    }

    pub fn kill_detached(&mut self, id: u64) -> io::Result<()> {
        self.kill_session(KILL_DETACHED, id)
    }

    fn kill_session(&mut self, kind: u8, id: u64) -> io::Result<()> {
        write_frame(&mut *self.connection.writer, kind, &id.to_le_bytes())?;
        self.expect(READY).map(|_| ())
    }

    pub fn attach(mut self, id: u64) -> io::Result<(Spawned, Vec<u8>)> {
        write_frame(&mut *self.connection.writer, ATTACH, &id.to_le_bytes())?;
        let pid = self.expect(READY)?.try_into().map(u32::from_le_bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad terminal process id"))?;
        let replay = self.expect(REPLAY)?;
        let Connection { mut reader, mut writer, streaming, abort } = self.connection;
        streaming()?;
        let (commands, incoming) = channel::<(u8, Vec<u8>)>();
        let (out_tx, out_rx) = sync_channel(OUTPUT_QUEUE);
        let (exit_tx, exit_rx) = channel();
        std::thread::Builder::new().name("aster-persistent-client-write".into()).spawn(move || {
            for (kind, bytes) in incoming {
                if kind == EXIT { break; }
                if write_frame(&mut *writer, kind, &bytes).is_err() {
                    abort();
                    return;
                }
                if matches!(kind, DETACH | KILL) { break; }
            }
        })?;
        let closed: Arc<(Mutex<bool>, Condvar)> = Arc::default();
        let remote = Remote { commands, id, pid, closed: closed.clone() };
        let stop = remote.commands.clone();
        std::thread::Builder::new().name("aster-persistent-client-read".into()).spawn(move || {
            let mut code = None;
            while let Ok(Some((kind, payload))) = read_frame(&mut *reader) {
                match kind {
                    DATA => if out_tx.send(payload).is_err() { break },
                    EXIT => {
                        code = payload.try_into().ok().map(u32::from_le_bytes).filter(|&code| code != u32::MAX);
                        break;
                    }
                    _ => break,
                }
            }
            let _ = stop.send((EXIT, Vec::new()));
            drop(out_tx);
            let _ = exit_tx.send(code);
            *closed.0.lock().unwrap() = true;
            closed.1.notify_all();
        })?;
        Ok((Spawned {
            writer: Box::new(Keys(remote.commands.clone(), DATA)),
            raw: Some(Box::new(Keys(remote.commands.clone(), RAW_DATA))),
            pty: Box::new(remote),
            reader: Box::new(super::Output { rx: out_rx, buf: Vec::new(), at: 0 }),
            wait: Box::new(move || exit_rx.recv().ok().flatten()),
        }, replay))
    }
}

type Commands = Sender<(u8, Vec<u8>)>;

struct Keys(Commands, u8);

impl Write for Keys {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.send((self.1, bytes.to_vec())).map_err(|_| io::ErrorKind::BrokenPipe)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

struct Remote {
    commands: Commands,
    id: u64,
    pid: u32,
    closed: Arc<(Mutex<bool>, Condvar)>,
}

impl Pty for Remote {
    fn resize(&self, cols: u16, rows: u16) {
        let _ = self.commands.send((RESIZE, [cols.to_le_bytes(), rows.to_le_bytes()].concat()));
    }

    fn force_resize(&self, cols: u16, rows: u16) {
        let _ = self.commands.send((FORCE_RESIZE, [cols.to_le_bytes(), rows.to_le_bytes()].concat()));
    }

    fn kill(&self) { let _ = self.commands.send((KILL, self.id.to_le_bytes().to_vec())); }
    fn pid(&self) -> u32 { self.pid }
    fn host_session_id(&self) -> Option<u64> { Some(self.id) }
    fn detach(&self) { let _ = self.commands.send((DETACH, self.id.to_le_bytes().to_vec())); }
    fn wait_host_close(&self, timeout: Duration) -> bool {
        let (closed, wake) = &*self.closed;
        *wake.wait_timeout_while(closed.lock().unwrap(), timeout, |closed| !*closed).unwrap().0
    }
}

impl Drop for Remote {
    fn drop(&mut self) { self.detach(); }
}

#[cfg(test)]
#[path = "../test/persistent_host.rs"]
mod tests;
