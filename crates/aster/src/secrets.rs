use aster_core::settings::{VaultIssue, VaultView};
use aster_ssh::secrets::Secrets as SecretStore;
use aster_vault::{Vault, VaultError};
use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const PASSWORD: &str = "password";

pub struct System<P>(std::marker::PhantomData<fn() -> P>);

impl<P> Default for System<P> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<P: aster_platform::Platform> SecretStore for System<P> {
    fn get(&self, key: &str) -> Option<String> {
        P::secret_get(key)
    }

    fn set(&self, key: &str, value: &str) -> bool {
        P::secret_set(key, value)
    }

    fn delete(&self, key: &str) {
        P::secret_delete(key);
    }
}

pub enum VaultCommand {
    Create(String),
    Unlock(String),
    Lock,
    Change { old: String, new: String },
    Erase,
    Remove { kind: String, key: String },
}

pub struct VaultUpdate {
    pub view: VaultView,
    pub sealed: Option<String>,
}

type Reply<T> = Box<dyn FnOnce(T) + Send>;
type SyncConfig = Box<dyn FnOnce(&str) -> Option<(String, u32)> + Send>;

enum Request {
    Vault(VaultCommand),
    Read(String, Reply<Option<String>>),
    Write(String, Option<String>, Reply<bool>),
    SyncConfig(SyncConfig),
    Shutdown,
}

#[derive(Clone)]
pub struct Secrets {
    tx: Sender<Request>,
}

pub struct VaultWorker {
    tx: Sender<Request>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for VaultWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(Request::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Secrets {
    pub fn start(
        os: Arc<dyn SecretStore>,
        sealed: String,
        minutes: u32,
        persist: impl Fn(&str, &str) -> bool + Send + 'static,
        notify: impl Fn(VaultUpdate) + Send + 'static,
    ) -> io::Result<(Self, VaultWorker)> {
        let (tx, rx) = unbounded();
        let mut store = Store {
            os, sealed, minutes, unlocked: None, pending: BTreeMap::new(),
            persist: Box::new(persist), notify: Box::new(notify),
        };
        let thread = thread::Builder::new().name("aster-vault".into()).spawn(move || store.run(rx))?;
        Ok((Self { tx: tx.clone() }, VaultWorker { tx, thread: Some(thread) }))
    }

    pub fn command(&self, command: VaultCommand) -> bool {
        self.tx.send(Request::Vault(command)).is_ok()
    }

    pub fn sync_config(&self, load: impl FnOnce(&str) -> Option<(String, u32)> + Send + 'static) -> bool {
        self.tx.send(Request::SyncConfig(Box::new(load))).is_ok()
    }

    pub fn read(&self, key: String, done: impl FnOnce(Option<String>) + Send + 'static) {
        if let Err(crossbeam_channel::SendError(Request::Read(_, done))) = self.tx.send(Request::Read(key, Box::new(done))) {
            done(None);
        }
    }

    pub fn write(&self, key: String, value: String, done: impl FnOnce(bool) + Send + 'static) {
        self.update(key, Some(value), done);
    }

    fn update(&self, key: String, value: Option<String>, done: impl FnOnce(bool) + Send + 'static) {
        if let Err(crossbeam_channel::SendError(Request::Write(_, _, done))) = self.tx.send(Request::Write(key, value, Box::new(done))) {
            done(false);
        }
    }
}

impl SecretStore for Secrets {
    fn get(&self, key: &str) -> Option<String> {
        let (tx, rx) = bounded(1);
        self.read(key.to_owned(), move |value| { let _ = tx.send(value); });
        rx.recv().ok().flatten()
    }

    fn set(&self, key: &str, value: &str) -> bool {
        let (tx, rx) = bounded(1);
        self.write(key.to_owned(), value.to_owned(), move |saved| { let _ = tx.send(saved); });
        rx.recv().unwrap_or(false)
    }

    fn delete(&self, key: &str) {
        self.update(key.to_owned(), None, |_| {});
    }
}

type Persist = Box<dyn Fn(&str, &str) -> bool + Send>;

struct Store {
    os: Arc<dyn SecretStore>,
    sealed: String,
    minutes: u32,
    unlocked: Option<(Vault, String, Instant)>,
    pending: BTreeMap<String, Option<String>>,
    persist: Persist,
    notify: Box<dyn Fn(VaultUpdate) + Send>,
}

fn lock_after(minutes: u32, unlocked: Instant, now: Instant) -> Option<Duration> {
    (minutes != 0).then(|| Duration::from_secs(u64::from(minutes) * 60).saturating_sub(now.saturating_duration_since(unlocked)))
}

impl Store {
    fn run(&mut self, rx: Receiver<Request>) {
        loop {
            let timeout = self.check_expiry();
            let request = match timeout {
                Some(remaining) => match rx.recv_timeout(remaining) {
                    Ok(request) => request,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(request) => request,
                    Err(_) => break,
                },
            };
            let _ = self.check_expiry();
            match request {
                Request::Vault(command) => {
                    if let Err(issue) = self.command(command) {
                        self.publish(Some(issue), false);
                    }
                }
                Request::Read(key, done) => done(self.read(&key)),
                Request::Write(key, value, done) => done(self.write(key, value)),
                Request::SyncConfig(load) => {
                    if let Some((sealed, minutes)) = load(&self.sealed) {
                        self.minutes = minutes;
                        if self.sealed != sealed {
                            self.sealed = sealed;
                            self.unlocked = None;
                            self.pending.clear();
                            self.publish(None, true);
                        }
                    }
                }
                Request::Shutdown => break,
            }
        }
    }

    fn check_expiry(&mut self) -> Option<Duration> {
        let (_, _, unlocked) = self.unlocked.as_ref()?;
        let remaining = lock_after(self.minutes, *unlocked, Instant::now())?;
        if remaining.is_zero() {
            self.unlocked = None;
            self.publish(None, false);
            return None;
        }
        Some(remaining)
    }

    fn publish(&self, error: Option<VaultIssue>, changed: bool) {
        let entries = self.unlocked.as_ref().map(|(vault, _, _)| {
            vault.secrets.iter().map(|s| (s.kind.clone(), s.key.clone())).collect()
        }).unwrap_or_default();
        (self.notify)(VaultUpdate {
            view: VaultView { configured: !self.sealed.is_empty(), unlocked: self.unlocked.is_some(), entries, error },
            sealed: changed.then(|| self.sealed.clone()),
        });
    }

    fn read(&self, key: &str) -> Option<String> {
        if self.sealed.is_empty() {
            return self.os.get(key);
        }
        self.unlocked.as_ref()?.0.get(PASSWORD, key).map(str::to_owned)
    }

    fn write(&mut self, key: String, value: Option<String>) -> bool {
        if self.sealed.is_empty() {
            return match value {
                Some(value) => self.os.set(&key, &value),
                None => { self.os.delete(&key); true }
            };
        }
        if self.unlocked.is_none() {
            self.pending.insert(key, value);
            return true;
        }
        match self.edit(PASSWORD, &key, value.as_deref()) {
            Ok(()) => true,
            Err(issue) => {
                self.publish(Some(issue), false);
                false
            }
        }
    }

    fn save(&mut self, sealed: String) -> Result<(), VaultIssue> {
        if !(self.persist)(&self.sealed, &sealed) {
            return Err(VaultIssue::SaveFailed);
        }
        self.sealed = sealed;
        Ok(())
    }

    fn edit(&mut self, kind: &str, key: &str, value: Option<&str>) -> Result<(), VaultIssue> {
        let (vault, passphrase, _) = self.unlocked.as_ref().ok_or(VaultIssue::Locked)?;
        if vault.get(kind, key) == value {
            return Ok(());
        }
        let mut next = vault.clone();
        match value {
            Some(value) => next.set(kind, key, value),
            None => { next.remove(kind, key); }
        }
        self.save(next.seal(passphrase))?;
        if let Some((vault, _, _)) = &mut self.unlocked {
            *vault = next;
        }
        self.publish(None, true);
        Ok(())
    }

    fn command(&mut self, command: VaultCommand) -> Result<(), VaultIssue> {
        let changed = match command {
            VaultCommand::Create(passphrase) => {
                if !self.sealed.is_empty() {
                    return Err(VaultIssue::AlreadyConfigured);
                }
                if passphrase.is_empty() {
                    return Err(VaultIssue::EmptyPassphrase);
                }
                let vault = Vault::default();
                self.save(vault.seal(&passphrase))?;
                self.unlocked = Some((vault, passphrase, Instant::now()));
                true
            }
            VaultCommand::Unlock(passphrase) => {
                if self.sealed.is_empty() {
                    return Err(VaultIssue::NotConfigured);
                }
                let mut vault = Vault::open(&self.sealed, &passphrase).map_err(|error| match error {
                    VaultError::WrongPassphrase => VaultIssue::WrongPassphrase,
                    VaultError::Corrupted(_) => VaultIssue::Corrupted,
                    VaultError::Unsupported(_) => VaultIssue::Unsupported,
                })?;
                let changed = !self.pending.is_empty();
                if changed {
                    for (key, value) in &self.pending {
                        match value {
                            Some(value) => vault.set(PASSWORD, key, value),
                            None => { vault.remove(PASSWORD, key); }
                        }
                    }
                    self.save(vault.seal(&passphrase))?;
                    self.pending.clear();
                }
                self.unlocked = Some((vault, passphrase, Instant::now()));
                changed
            }
            VaultCommand::Lock => {
                self.unlocked = None;
                false
            }
            VaultCommand::Change { old, new } => {
                let (vault, passphrase, _) = self.unlocked.as_ref().ok_or(VaultIssue::Locked)?;
                if old != *passphrase {
                    return Err(VaultIssue::WrongPassphrase);
                }
                if new.is_empty() {
                    return Err(VaultIssue::EmptyPassphrase);
                }
                self.save(vault.seal(&new))?;
                if let Some((_, passphrase, _)) = &mut self.unlocked {
                    *passphrase = new;
                }
                true
            }
            VaultCommand::Erase => {
                if self.unlocked.is_none() {
                    return Err(VaultIssue::Locked);
                }
                self.save(String::new())?;
                self.unlocked = None;
                self.pending.clear();
                true
            }
            VaultCommand::Remove { kind, key } => return self.edit(&kind, &key, None),
        };
        self.publish(None, changed);
        Ok(())
    }
}

#[cfg(test)]
#[path = "../test/secrets.rs"]
mod tests;
