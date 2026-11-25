use aster_core::app::Shared;
use aster_core::panes::PaneId;
use aster_core::profile::Stream;
use aster_core::session::WindowState;
use aster_platform::host::persistent::SessionInfo;
use aster_platform::{Platform, PlatformError, Pty, SpawnRequest, Spawned, WindowId};
use aster_vt::{Size, Terminal};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::io::{self, Read};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU8, Ordering}};
use std::time::{Duration, Instant};

type Key = (WindowId, PaneId);
type Job = Box<dyn FnOnce() + Send>;
const CANCELLED: u8 = 1;
const RESTARTING: u8 = 2;

pub struct Request {
    pub key: Key,
    pub request: SpawnRequest,
    pub terminal: Shared,
    pub delegation: Option<String>,
    pub stream: Option<Stream>,
    pub elevated: bool,
    pub clear: bool,
    pub hosted: bool,
    pub previous: Option<u64>,
    pub title: String,
}

pub struct Ready {
    pub key: Key,
    pub terminal: Shared,
    pub clear: bool,
    pub replay: Option<(Terminal, (u16, u16))>,
    pub size: (u16, u16),
    result: Option<Result<Spawned, PlatformError>>,
    cancelled: Arc<AtomicU8>,
    previous: Option<u64>,
    closing: Arc<Closing>,
}

impl Ready {
    pub fn take(&mut self) -> Result<Spawned, PlatformError> {
        self.result.take().expect("local spawn result")
    }
}

impl Drop for Ready {
    fn drop(&mut self) {
        if let Some(Ok(spawned)) = self.result.take() {
            let Spawned { pty, wait, .. } = spawned;
            let hosted = pty.host_session_id();
            if hosted.is_some() && hosted == self.previous && self.cancelled.load(Ordering::Relaxed) == RESTARTING {
                pty.detach();
            } else {
                self.closing.kill(pty);
            }
            if hosted.is_none() {
                let _ = std::thread::Builder::new().name("aster-spawn-cancel".into()).spawn(wait);
            }
        }
    }
}

#[derive(Default)]
pub struct Pending {
    pub size: Option<(u16, u16)>,
    pub input: Vec<Vec<u8>>,
    cancelled: Arc<AtomicU8>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        let _ = self.cancelled.compare_exchange(0, CANCELLED, Ordering::Relaxed, Ordering::Relaxed);
    }
}

pub struct LocalSpawns {
    queue: Sender<Job>,
    sender: Sender<Ready>,
    ready: Receiver<Ready>,
    pending: HashMap<Key, Pending>,
    claimed: HashMap<String, Receiver<Result<Spawned, PlatformError>>>,
    restore: Arc<Mutex<HashMap<u64, SessionInfo>>>,
    closing: Arc<Closing>,
}

#[derive(Default)]
struct Closing {
    used: AtomicBool,
    sessions: Mutex<Vec<Box<dyn Pty>>>,
}

impl Closing {
    fn kill(&self, pty: Box<dyn Pty>) {
        pty.kill();
        if pty.host_session_id().is_some() {
            let mut active = self.sessions.lock().unwrap();
            active.retain(|pty| !pty.wait_host_close(Duration::ZERO));
            active.push(pty);
        }
    }
}

pub struct Worker {
    done: Receiver<()>,
    closing: Arc<Closing>,
}

impl Worker {
    pub fn finish(self) {
        if !self.closing.used.load(Ordering::Relaxed) { return; }
        if self.done.recv_timeout(Duration::from_secs(8)).is_err() {
            tracing::warn!("local spawn worker did not finish before exit");
        }
        let sessions = std::mem::take(&mut *self.closing.sessions.lock().unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        for pty in sessions {
            if !pty.wait_host_close(deadline.saturating_duration_since(Instant::now())) {
                tracing::warn!("persistent terminal host did not confirm shell closure before exit");
            }
        }
    }
}

impl LocalSpawns {
    pub fn new() -> io::Result<(Self, Worker)> {
        let (queue, tasks) = unbounded::<Job>();
        let (sender, ready) = unbounded();
        let (finished, done) = bounded(1);
        let closing = Arc::new(Closing::default());
        std::thread::Builder::new().name("aster-local-spawn".into()).spawn(move || {
            for job in tasks {
                job();
            }
            let _ = finished.send(());
        })?;
        let worker = Worker { done, closing: closing.clone() };
        Ok((Self { queue, sender, ready, pending: HashMap::new(), claimed: HashMap::new(), restore: Arc::default(), closing }, worker))
    }

    pub fn claim_delegation<P: Platform>(&mut self, token: &str) -> io::Result<()> {
        if self.claimed.contains_key(token) { return Ok(()); }
        let (send, receive) = bounded(1);
        let owned_token = token.to_owned();
        std::thread::Builder::new().name("aster-console-claim".into()).spawn(move || {
            let _ = send.send(P::attach_delegated_console(&owned_token));
        })?;
        self.claimed.insert(token.to_owned(), receive);
        Ok(())
    }

    pub fn prepare_restore<P: Platform>(&self, windows: Vec<WindowState>) {
        let restore = self.restore.clone();
        let _ = self.queue.send(Box::new(move || {
            match crate::pty_host::restore::<P>(&windows) {
                Ok(list) => *restore.lock().unwrap() = list,
                Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {}
                Err(error) => tracing::warn!("could not list persistent terminal sessions: {error}"),
            }
        }));
    }

    pub fn kill_detached<P: Platform>(&self, id: u64) {
        self.closing.used.store(true, Ordering::Relaxed);
        let _ = self.queue.send(Box::new(move || crate::pty_host::kill_detached::<P>(id)));
    }

    pub fn kill(&self, pty: Box<dyn Pty>) {
        self.closing.kill(pty);
    }

    pub fn launch<P: Platform>(&mut self, request: Request, wake: impl FnOnce() + Send + 'static) -> io::Result<()> {
        let Request { key, request, terminal, delegation, stream, elevated, clear, hosted, previous, title } = request;
        let claimed = delegation.as_ref().and_then(|token| self.claimed.remove(token));
        let worker_name = if delegation.is_some() { Some("aster-console-attach") } else if elevated { Some("aster-elevate") } else { None };
        let pending = Pending::default();
        let cancelled = pending.cancelled.clone();
        let sender = self.sender.clone();
        let restore = self.restore.clone();
        let closing = self.closing.clone();
        if hosted { closing.used.store(true, Ordering::Relaxed); }
        let run = move || {
            if cancelled.load(Ordering::Relaxed) != 0 {
                return;
            }
            let size = (request.cols, request.rows);
            let mut replay = None;
            let direct = || P::spawn(&request).map(|spawned| crate::stream::wrap(spawned, stream.as_ref()));
            let result = if let Some(token) = delegation {
                let attached = match claimed {
                    Some(receiver) => receiver.recv().unwrap_or_else(|_| Err(PlatformError::new(0, "console attachment worker stopped"))),
                    None => P::attach_delegated_console(&token),
                };
                attached.inspect(|spawned| {
                    tracing::info!(pid = spawned.pty.pid(), "delegated console attached");
                    spawned.pty.resize(size.0, size.1);
                })
            } else if elevated {
                P::spawn_elevated(&request)
            } else if hosted {
                let attach = previous.and_then(|id| restore.lock().unwrap().remove(&id));
                match crate::pty_host::spawn::<P>(&request, &title, stream.as_ref(), attach) {
                    Ok(mut connection) => {
                        if let Some((cols, rows)) = connection.previous_size {
                            let mut fresh = terminal.lock().blank_for_replay(Size::new(cols.into(), rows.into()));
                            fresh.feed_replay(&connection.replay);
                            replay = Some((fresh, (cols, rows)));
                        } else if !connection.replay.is_empty() {
                            connection.spawned.reader = Box::new(io::Cursor::new(connection.replay).chain(connection.spawned.reader));
                        }
                        Ok(connection.spawned)
                    }
                    Err(crate::pty_host::SpawnError::Unavailable(error)) => {
                        tracing::warn!("persistent terminal host unavailable, using direct spawn: {error}");
                        direct()
                    }
                    Err(crate::pty_host::SpawnError::Failed(error)) => Err(PlatformError::new(-1, error.to_string())),
                }
            } else {
                direct()
            };
            let ready = Ready { key, terminal, clear, replay, size, result: Some(result), cancelled, previous, closing };
            if sender.send(ready).is_ok() {
                wake();
            }
        };
        if let Some(name) = worker_name {
            std::thread::Builder::new().name(name.into()).spawn(run)?;
        } else {
            self.queue.send(Box::new(run)).map_err(|_| std::io::Error::other("local spawn worker stopped"))?;
        }
        self.pending.insert(key, pending);
        Ok(())
    }

    pub fn pending(&mut self, key: Key) -> Option<&mut Pending> {
        self.pending.get_mut(&key)
    }

    pub fn take_pending(&mut self, key: Key) -> Option<Pending> {
        self.pending.remove(&key)
    }

    pub fn cancel_window(&mut self, window: WindowId, restarting: bool) {
        self.pending.retain(|(id, _), pending| {
            if *id != window { return true; }
            if restarting { pending.cancelled.store(RESTARTING, Ordering::Relaxed); }
            false
        });
    }

    pub fn next(&self) -> Option<Ready> {
        self.ready.try_recv().ok()
    }
}
