use aster_core::app::Shared;
use aster_core::panes::PaneId;
use aster_core::profile::Stream;
use aster_platform::{Platform, PlatformError, SpawnRequest, Spawned, WindowId};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

type Key = (WindowId, PaneId);
type Job = Box<dyn FnOnce() + Send>;

pub struct Request {
    pub key: Key,
    pub request: SpawnRequest,
    pub terminal: Shared,
    pub stream: Option<Stream>,
    pub elevated: bool,
    pub clear: bool,
}

pub struct Ready {
    pub key: Key,
    pub terminal: Shared,
    pub clear: bool,
    result: Option<Result<Spawned, PlatformError>>,
}

impl Ready {
    pub fn take(&mut self) -> Result<Spawned, PlatformError> {
        self.result.take().expect("local spawn result")
    }
}

impl Drop for Ready {
    fn drop(&mut self) {
        if let Some(Ok(spawned)) = self.result.take() {
            spawned.pty.kill();
            let Spawned { wait, .. } = spawned;
            let _ = std::thread::Builder::new().name("aster-spawn-cancel".into()).spawn(wait);
        }
    }
}

#[derive(Default)]
pub struct Pending {
    pub size: Option<(u16, u16)>,
    pub input: Vec<Vec<u8>>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub struct LocalSpawns {
    queue: Sender<Job>,
    sender: Sender<Ready>,
    ready: Receiver<Ready>,
    pending: HashMap<Key, Pending>,
}

impl LocalSpawns {
    pub fn new() -> std::io::Result<Self> {
        let (queue, tasks) = unbounded::<Job>();
        let (sender, ready) = unbounded();
        std::thread::Builder::new().name("aster-local-spawn".into()).spawn(move || {
            for job in tasks {
                job();
            }
        })?;
        Ok(Self { queue, sender, ready, pending: HashMap::new() })
    }

    pub fn launch<P: Platform>(&mut self, request: Request, wake: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
        let Request { key, request, terminal, stream, elevated, clear } = request;
        let pending = Pending::default();
        let cancelled = pending.cancelled.clone();
        let sender = self.sender.clone();
        let run = move || {
            if cancelled.load(Ordering::Relaxed) {
                return;
            }
            let result = if elevated {
                P::spawn_elevated(&request)
            } else {
                P::spawn(&request).map(|spawned| crate::stream::wrap(spawned, stream.as_ref()))
            };
            let ready = Ready { key, terminal, clear, result: Some(result) };
            if sender.send(ready).is_ok() {
                wake();
            }
        };
        if elevated {
            std::thread::Builder::new().name("aster-elevate".into()).spawn(run)?;
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

    pub fn cancel_window(&mut self, window: WindowId) {
        self.pending.retain(|(id, _), _| *id != window);
    }

    pub fn next(&self) -> Option<Ready> {
        self.ready.try_recv().ok()
    }
}
