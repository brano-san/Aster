use aster_core::settings::{VaultEntry, VaultIssue, VaultView};
use aster_core::vault_settings::{VaultAction, VaultReply};
use aster_ssh::secrets::Secrets as SecretStore;
use aster_vault::{kind_for_key, Vault};
use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

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
    Erase(Option<String>),
}

pub struct VaultUpdate {
    pub view: VaultView,
    pub sealed: Option<String>,
}

pub type Reply<T> = Box<dyn FnOnce(T) + Send>;
type SyncConfig = Box<dyn FnOnce(&str) -> Option<(String, u32)> + Send>;

#[derive(Clone)]
pub enum SecretUse {
    Read(String),
    Write(String, Option<String>),
}

type UseResult = Result<Option<String>, VaultIssue>;
type Ask = Arc<dyn Fn(Option<VaultIssue>, Reply<Option<String>>) + Send + Sync>;

#[derive(Clone)]
pub struct WindowSecrets {
    store: Secrets,
    ask: Ask,
}

impl WindowSecrets {
    pub fn new(store: Secrets, ask: impl Fn(Option<VaultIssue>, Reply<Option<String>>) + Send + Sync + 'static) -> Self {
        Self { store, ask: Arc::new(ask) }
    }

    pub fn use_secret(&self, action: SecretUse, done: impl FnOnce(UseResult) + Send + 'static) {
        self.attempt(action, None, Box::new(done));
    }

    fn attempt(&self, action: SecretUse, passphrase: Option<String>, done: Reply<UseResult>) {
        let next = self.clone();
        self.store.use_secret(action.clone(), passphrase, move |result| match result {
            Err(issue @ (VaultIssue::Locked | VaultIssue::WrongPassphrase)) => {
                let ask = next.ask.clone();
                ask((issue == VaultIssue::WrongPassphrase).then_some(issue), Box::new(move |password| {
                    if password.is_some() {
                        next.attempt(action, password, done);
                    } else {
                        done(Err(VaultIssue::Locked));
                    }
                }));
            }
            result => done(result),
        });
    }

    fn wait(&self, action: SecretUse) -> UseResult {
        let (tx, rx) = bounded(1);
        self.use_secret(action, move |result| { let _ = tx.send(result); });
        rx.recv().unwrap_or(Err(VaultIssue::Unavailable))
    }
}

impl SecretStore for WindowSecrets {
    fn get(&self, key: &str) -> Option<String> {
        self.wait(SecretUse::Read(key.to_owned())).ok().flatten()
    }

    fn set(&self, key: &str, value: &str) -> bool {
        self.wait(SecretUse::Write(key.to_owned(), Some(value.to_owned()))).is_ok()
    }

    fn delete(&self, key: &str) {
        let _ = self.wait(SecretUse::Write(key.to_owned(), None));
    }
}

enum Request {
    Vault(VaultCommand, Reply<Result<VaultReply, VaultIssue>>),
    Manage(VaultAction, Option<String>, Reply<Result<VaultReply, VaultIssue>>),
    Read(String, Reply<Option<String>>),
    Use(SecretUse, Option<String>, Reply<UseResult>),
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
        persist: impl Fn(&str, &str, Option<&str>) -> bool + Send + 'static,
        notify: impl Fn(VaultUpdate) + Send + 'static,
    ) -> io::Result<(Self, VaultWorker)> {
        let (tx, rx) = unbounded();
        let mut store = Store {
            os, sealed, minutes, unlocked: None, preview: None, known: None, pending: BTreeMap::new(),
            persist: Box::new(persist), notify: Box::new(notify),
        };
        let thread = thread::Builder::new().name("aster-vault".into()).spawn(move || store.run(rx))?;
        Ok((Self { tx: tx.clone() }, VaultWorker { tx, thread: Some(thread) }))
    }

    pub fn command(&self, command: VaultCommand, done: impl FnOnce(Result<VaultReply, VaultIssue>) + Send + 'static) {
        if let Err(crossbeam_channel::SendError(Request::Vault(_, done))) = self.tx.send(Request::Vault(command, Box::new(done))) {
            done(Err(VaultIssue::Unavailable));
        }
    }

    pub fn manage(&self, action: VaultAction, passphrase: Option<String>, done: impl FnOnce(Result<VaultReply, VaultIssue>) + Send + 'static) {
        if let Err(crossbeam_channel::SendError(Request::Manage(_, _, done))) = self.tx.send(Request::Manage(action, passphrase, Box::new(done))) {
            done(Err(VaultIssue::Unavailable));
        }
    }

    fn use_secret(&self, action: SecretUse, passphrase: Option<String>, done: impl FnOnce(UseResult) + Send + 'static) {
        if let Err(crossbeam_channel::SendError(Request::Use(_, _, done))) = self.tx.send(Request::Use(action, passphrase, Box::new(done))) {
            done(Err(VaultIssue::Unavailable));
        }
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

type Persist = Box<dyn Fn(&str, &str, Option<&str>) -> bool + Send>;

struct Store {
    os: Arc<dyn SecretStore>,
    sealed: String,
    minutes: u32,
    unlocked: Option<(Vault, String, Instant)>,
    preview: Option<Vec<VaultEntry>>,
    known: Option<Vec<VaultEntry>>,
    pending: BTreeMap<String, Option<String>>,
    persist: Persist,
    notify: Box<dyn Fn(VaultUpdate) + Send>,
}

fn lock_after(minutes: u32, unlocked: Instant, now: Instant) -> Duration {
    Duration::from_secs(u64::from(minutes) * 60).saturating_sub(now.saturating_duration_since(unlocked))
}

fn metadata(vault: &Vault) -> Vec<VaultEntry> {
    vault.secrets.iter().map(|s| VaultEntry {
        kind: s.kind.clone(), key: s.key.clone(), label: s.label.clone(),
    }).collect()
}

fn open_vault(sealed: &str, passphrase: &str) -> Result<Vault, VaultIssue> {
    Vault::open(sealed, passphrase).map_err(crate::config_file::vault_issue)
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
                Request::Vault(command, done) => {
                    let result = self.command(command);
                    if let Err(issue) = result {
                        self.publish(Some(issue), false);
                    }
                    done(result.map(|()| VaultReply::Applied));
                }
                Request::Manage(action, passphrase, done) => done(self.manage(action, passphrase.as_deref())),
                Request::Read(key, done) => done(self.read(&key)),
                Request::Use(action, passphrase, done) => done(self.use_secret(action, passphrase.as_deref())),
                Request::Write(key, value, done) => done(self.write(key, value)),
                Request::SyncConfig(load) => {
                    if let Some((sealed, minutes)) = load(&self.sealed) {
                        let preview_closed = self.minutes != minutes && self.preview.take().is_some();
                        if self.minutes != minutes { self.known = None; }
                        self.minutes = minutes;
                        if self.sealed != sealed {
                            self.sealed = sealed;
                            self.unlocked = None;
                            self.preview = None;
                            self.known = None;
                            self.pending.clear();
                            self.publish(None, true);
                        } else if preview_closed {
                            self.publish(None, false);
                        }
                    }
                }
                Request::Shutdown => break,
            }
        }
    }

    fn check_expiry(&mut self) -> Option<Duration> {
        let (_, _, unlocked) = self.unlocked.as_ref()?;
        let remaining = lock_after(self.minutes, *unlocked, Instant::now());
        if remaining.is_zero() {
            self.unlocked = None;
            self.publish(None, false);
            return None;
        }
        Some(remaining)
    }

    fn remember(&mut self, vault: Vault, passphrase: String) {
        if self.minutes == 0 {
            self.unlocked = None;
            let entries = metadata(&vault);
            self.known = Some(entries.clone());
            self.preview = Some(entries);
        } else {
            self.preview = None;
            self.known = None;
            self.unlocked = Some((vault, passphrase, Instant::now()));
        }
    }

    fn authenticated<'a>(&'a self, passphrase: Option<&str>, opened: &'a mut Option<Vault>) -> Result<&'a Vault, VaultIssue> {
        if let Some((vault, _, _)) = &self.unlocked {
            return Ok(vault);
        }
        let passphrase = passphrase.filter(|_| self.preview.is_some()).ok_or(VaultIssue::Locked)?;
        Ok(opened.insert(open_vault(&self.sealed, passphrase)?))
    }

    fn publish(&self, error: Option<VaultIssue>, changed: bool) {
        let mut entries = self.unlocked.as_ref().map(|(vault, _, _)| metadata(vault)).or_else(|| self.preview.clone()).unwrap_or_default();
        entries.retain(|entry| !entry.key.starts_with("key-index:"));
        (self.notify)(VaultUpdate {
            view: VaultView { configured: !self.sealed.is_empty(), unlocked: self.unlocked.is_some() || self.preview.is_some(), entries, error },
            sealed: changed.then(|| self.sealed.clone()),
        });
    }

    fn read(&self, key: &str) -> Option<String> {
        if self.sealed.is_empty() {
            return self.os.get(key);
        }
        Self::lookup(&self.unlocked.as_ref()?.0, key).map(str::to_owned)
    }

    fn lookup<'a>(vault: &'a Vault, key: &str) -> Option<&'a str> {
        match key.strip_prefix("key-file:") {
            Some(fingerprint_or_label) => vault.key_file(fingerprint_or_label).or_else(|| {
                if !fingerprint_or_label.starts_with("SHA256:") {
                    return None;
                }
                vault.secrets.iter().filter(|entry| entry.kind == "key-file").find_map(|entry| {
                    (aster_ssh::secrets::private_key_fingerprint(&entry.value).as_deref() == Some(fingerprint_or_label)).then_some(entry.value.as_str())
                })
            }),
            None => vault.get(kind_for_key(key), key),
        }
    }

    fn use_secret(&mut self, action: SecretUse, passphrase: Option<&str>) -> UseResult {
        if self.minutes != 0 || self.sealed.is_empty() {
            return match action {
                SecretUse::Read(key) => Ok(self.read(&key)),
                SecretUse::Write(key, value) => self.write(key, value).then_some(None).ok_or(VaultIssue::SaveFailed),
            };
        }
        if let SecretUse::Read(key) = &action {
            if self.known.as_ref().is_some_and(|entries| {
                !key.starts_with("key-file:") && !self.pending.contains_key(key) && !entries.iter().any(|entry| &entry.key == key)
            }) {
                return Ok(None);
            }
        }
        let passphrase = passphrase.ok_or(VaultIssue::Locked)?;
        let mut vault = open_vault(&self.sealed, passphrase)?;
        self.apply_pending(&mut vault);
        let (result, edited) = match action {
            SecretUse::Read(key) => (Self::lookup(&vault, &key).map(str::to_owned), false),
            SecretUse::Write(key, value) => {
                let kind = kind_for_key(&key);
                let edited = vault.get(kind, &key) != value.as_deref();
                match value {
                    Some(value) => vault.set(kind, &key, &value),
                    None => { vault.remove(kind, &key); }
                }
                (None, edited)
            }
        };
        let changed = edited || !self.pending.is_empty();
        if changed {
            self.save(vault.seal(passphrase), None)?;
            self.pending.clear();
        }
        let entries = metadata(&vault);
        self.known = Some(entries.clone());
        if self.preview.is_some() {
            self.preview = Some(entries);
        }
        if changed {
            self.publish(None, true);
        }
        Ok(result)
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
        match self.edit(kind_for_key(&key), &key, value.as_deref()) {
            Ok(()) => true,
            Err(issue) => {
                self.publish(Some(issue), false);
                false
            }
        }
    }

    fn save(&mut self, sealed: String, rekey: Option<&str>) -> Result<(), VaultIssue> {
        if !(self.persist)(&self.sealed, &sealed, rekey) {
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
        self.save(next.seal(passphrase), None)?;
        if let Some((vault, _, _)) = &mut self.unlocked {
            *vault = next;
        }
        self.publish(None, true);
        Ok(())
    }

    fn apply_pending(&self, vault: &mut Vault) {
        for (key, value) in &self.pending {
            match value {
                Some(value) => vault.set(kind_for_key(key), key, value),
                None => { vault.remove(kind_for_key(key), key); }
            }
        }
    }

    fn flush_pending(&mut self, vault: &mut Vault, passphrase: &str) -> Result<bool, VaultIssue> {
        if self.pending.is_empty() {
            return Ok(false);
        }
        self.apply_pending(vault);
        self.save(vault.seal(passphrase), None)?;
        self.pending.clear();
        Ok(true)
    }

    fn manage(&mut self, action: VaultAction, passphrase: Option<&str>) -> Result<VaultReply, VaultIssue> {
        let mut opened = None;
        self.authenticated(passphrase, &mut opened)?;
        if let (Some(vault), Some(password)) = (&mut opened, passphrase) {
            if self.flush_pending(vault, password)? {
                let entries = metadata(vault);
                self.known = Some(entries.clone());
                self.preview = Some(entries);
                self.publish(None, true);
            }
        }
        let vault = self.unlocked.as_ref().map(|(vault, _, _)| vault).or(opened.as_ref()).ok_or(VaultIssue::Locked)?;
        let mut next = vault.clone();
        let entry = match &action {
            VaultAction::Show(entry) | VaultAction::Delete(entry)
            | VaultAction::Rename { entry, .. } | VaultAction::Replace { entry, .. }
            | VaultAction::Export { entry, .. } => Some(entry),
            VaultAction::Import { .. } => None,
        };
        let existing = entry.map(|entry| {
            next.secrets.iter_mut().find(|secret| secret.kind == entry.kind && secret.key == entry.key)
                .ok_or(VaultIssue::EntryMissing)
        }).transpose()?;
        match (action, existing) {
            (VaultAction::Show(_), Some(secret)) => return Ok(VaultReply::Value(secret.value.clone())),
            (VaultAction::Rename { label, .. }, Some(secret)) => {
                let label = label.trim();
                if label.is_empty() {
                    return Err(VaultIssue::EmptyLabel);
                }
                if secret.kind == "key-file" && vault.secrets.iter().any(|other| {
                    other.kind == "key-file" && other.key != secret.key
                        && (other.label == label || other.key == label || other.key.strip_prefix("key-file:") == Some(label))
                }) {
                    return Err(VaultIssue::DuplicateLabel);
                }
                secret.label = label.to_owned();
            }
            (VaultAction::Replace { value, .. }, Some(secret)) => secret.value = value,
            (VaultAction::Export { path, .. }, Some(secret)) => {
                export_secret(&path, &secret.value).map_err(|_| VaultIssue::ExportFailed)?;
                return Ok(VaultReply::Applied);
            }
            (VaultAction::Import { label, path }, _) => {
                let label = label.trim();
                if label.is_empty() {
                    return Err(VaultIssue::EmptyLabel);
                }
                if vault.key_file(label).is_some() {
                    return Err(VaultIssue::DuplicateLabel);
                }
                let value = std::fs::read_to_string(path).map_err(|_| VaultIssue::ImportFailed)?;
                if value.trim().is_empty() {
                    return Err(VaultIssue::ImportFailed);
                }
                next.set("key-file", &aster_ssh::secrets::key_file_key(label), &value);
            }
            (VaultAction::Delete(entry), _) => { next.remove(&entry.kind, &entry.key); }
            _ => return Err(VaultIssue::EntryMissing),
        }
        if &next != vault {
            let password = self.unlocked.as_ref().map(|(_, password, _)| password.as_str()).or(passphrase).ok_or(VaultIssue::Locked)?;
            self.save(next.seal(password), None)?;
            if let Some((vault, _, _)) = &mut self.unlocked {
                *vault = next;
            } else {
                let entries = metadata(&next);
                self.known = Some(entries.clone());
                self.preview = Some(entries);
            }
            self.publish(None, true);
        }
        Ok(VaultReply::Applied)
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
                self.save(vault.seal(&passphrase), None)?;
                self.remember(vault, passphrase);
                true
            }
            VaultCommand::Unlock(passphrase) => {
                if self.sealed.is_empty() {
                    return Err(VaultIssue::NotConfigured);
                }
                let mut vault = open_vault(&self.sealed, &passphrase)?;
                let changed = self.flush_pending(&mut vault, &passphrase)?;
                self.remember(vault, passphrase);
                changed
            }
            VaultCommand::Lock => {
                self.unlocked = None;
                self.preview = None;
                self.known = None;
                false
            }
            VaultCommand::Change { old, new } => {
                if self.unlocked.as_ref().is_some_and(|(_, passphrase, _)| old != *passphrase) {
                    return Err(VaultIssue::WrongPassphrase);
                }
                if new.is_empty() {
                    return Err(VaultIssue::EmptyPassphrase);
                }
                let mut opened = None;
                let mut vault = self.authenticated(Some(&old), &mut opened)?.clone();
                self.apply_pending(&mut vault);
                self.save(vault.seal(&new), Some(&new))?;
                self.pending.clear();
                if let Some((saved, passphrase, _)) = &mut self.unlocked {
                    *saved = vault;
                    *passphrase = new;
                } else {
                    let entries = metadata(&vault);
                    self.known = Some(entries.clone());
                    self.preview = Some(entries);
                }
                true
            }
            VaultCommand::Erase(passphrase) => {
                self.authenticated(passphrase.as_deref(), &mut None)?;
                self.save(String::new(), None)?;
                self.unlocked = None;
                self.preview = None;
                self.known = None;
                self.pending.clear();
                true
            }
        };
        self.publish(None, changed);
        Ok(())
    }
}

fn export_secret(path: &str, value: &str) -> io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    let result = file.write_all(value.as_bytes()).and_then(|()| file.sync_all());
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[cfg(test)]
#[path = "../test/secrets.rs"]
mod tests;
