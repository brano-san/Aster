use super::*;
use aster_ssh::secrets::Memory;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Default)]
struct OsStore {
    calls: AtomicUsize,
    memory: Memory,
}

impl SecretStore for OsStore {
    fn get(&self, key: &str) -> Option<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.memory.get(key)
    }

    fn set(&self, key: &str, value: &str) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.memory.set(key, value)
    }

    fn delete(&self, key: &str) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.memory.delete(key);
    }
}

struct Harness {
    secrets: Secrets,
    updates: Receiver<VaultUpdate>,
    os: Arc<OsStore>,
    sealed: Arc<Mutex<String>>,
    writable: Arc<AtomicBool>,
    _worker: VaultWorker,
}

impl Harness {
    fn start(initial: String) -> Self {
        let os = Arc::new(OsStore::default());
        let writable = Arc::new(AtomicBool::new(true));
        let sealed = Arc::new(Mutex::new(initial.clone()));
        let (can_write, disk) = (writable.clone(), sealed.clone());
        let (updates_tx, updates) = unbounded();
        let (secrets, worker) = Secrets::start(os.clone(), initial, 15, move |expected, next| {
            let mut saved = disk.lock().unwrap();
            if !can_write.load(Ordering::SeqCst) || *saved != expected {
                return false;
            }
            *saved = next.to_owned();
            true
        }, move |update| { let _ = updates_tx.send(update); }).unwrap();
        Self { secrets, updates, os, sealed, writable, _worker: worker }
    }

    fn update(&self) -> VaultUpdate {
        self.updates.recv_timeout(Duration::from_secs(30)).expect("vault update")
    }

    fn command(&self, command: VaultCommand) -> VaultUpdate {
        assert!(self.secrets.command(command));
        self.update()
    }
}

#[test]
fn an_unconfigured_store_delegates_every_operation_to_the_os() {
    let h = Harness::start(String::new());
    assert!(h.secrets.set("sudo:local", "system"));
    assert_eq!(h.secrets.get("sudo:local").as_deref(), Some("system"));
    h.secrets.delete("sudo:local");
    assert_eq!(h.secrets.get("sudo:local"), None);
    assert_eq!(h.os.calls.load(Ordering::SeqCst), 4);
    assert!(h.sealed.lock().unwrap().is_empty());
}

#[test]
fn locked_writes_and_deletes_are_applied_only_after_successful_unlock() {
    let mut initial = Vault::default();
    initial.set(PASSWORD, "key:a", "old");
    let sealed = initial.seal("master");
    let h = Harness::start(sealed.clone());
    h.os.memory.set("key:a", "system");
    assert_eq!(h.secrets.get("key:a"), None);
    h.secrets.delete("key:a");
    assert!(h.secrets.set("sudo:new", "first"));
    assert!(h.secrets.set("sudo:new", "latest"));
    assert_eq!(h.secrets.get("sudo:new"), None);
    assert_eq!(h.command(VaultCommand::Unlock("wrong".into())).view.error, Some(VaultIssue::WrongPassphrase));
    h.writable.store(false, Ordering::SeqCst);
    let failed = h.command(VaultCommand::Unlock("master".into()));
    assert_eq!(failed.view.error, Some(VaultIssue::SaveFailed));
    assert!(!failed.view.unlocked);
    assert_eq!(*h.sealed.lock().unwrap(), sealed);
    h.writable.store(true, Ordering::SeqCst);
    let saved = h.command(VaultCommand::Unlock("master".into()));
    assert_eq!(saved.view.entries, vec![(PASSWORD.to_owned(), "sudo:new".to_owned())]);
    let vault = Vault::open(&saved.sealed.unwrap(), "master").unwrap();
    assert_eq!(vault.get(PASSWORD, "key:a"), None);
    assert_eq!(vault.get(PASSWORD, "sudo:new"), Some("latest"));
    assert_eq!(h.secrets.get("sudo:new").as_deref(), Some("latest"));
    assert_eq!(h.os.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn unlocked_writes_reseal_and_failed_writes_leave_the_previous_value() {
    let h = Harness::start(String::new());
    assert!(h.command(VaultCommand::Create("master".into())).view.unlocked);
    assert!(h.secrets.set("ssh:host", "first"));
    let saved = h.update().sealed.unwrap();
    assert_eq!(Vault::open(&saved, "master").unwrap().get(PASSWORD, "ssh:host"), Some("first"));
    h.writable.store(false, Ordering::SeqCst);
    assert!(!h.secrets.set("ssh:host", "second"));
    assert_eq!(h.update().view.error, Some(VaultIssue::SaveFailed));
    assert_eq!(h.secrets.get("ssh:host").as_deref(), Some("first"));
    assert_eq!(*h.sealed.lock().unwrap(), saved);
    assert_eq!(h.os.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn passphrase_change_entry_removal_and_erase_follow_the_vault_state() {
    let mut initial = Vault::default();
    initial.set("token", "host", "token");
    initial.set(PASSWORD, "host", "password");
    let h = Harness::start(initial.seal("old"));
    h.os.memory.set("host", "system");
    assert_eq!(h.command(VaultCommand::Erase).view.error, Some(VaultIssue::Locked));
    assert!(h.command(VaultCommand::Unlock("old".into())).view.unlocked);
    assert_eq!(h.command(VaultCommand::Change { old: "bad".into(), new: "new".into() }).view.error, Some(VaultIssue::WrongPassphrase));
    let changed = h.command(VaultCommand::Change { old: "old".into(), new: "new".into() }).sealed.unwrap();
    assert!(Vault::open(&changed, "old").is_err());
    assert!(h.command(VaultCommand::Lock).view.entries.is_empty());
    assert!(h.command(VaultCommand::Unlock("new".into())).view.unlocked);
    let removed = h.command(VaultCommand::Remove { kind: "token".into(), key: "host".into() });
    assert_eq!(removed.view.entries, vec![(PASSWORD.to_owned(), "host".to_owned())]);
    assert_eq!(h.secrets.get("host").as_deref(), Some("password"));
    let erased = h.command(VaultCommand::Erase);
    assert!(!erased.view.configured);
    assert!(erased.sealed.unwrap().is_empty());
    assert_eq!(h.secrets.get("host").as_deref(), Some("system"));
    assert_eq!(h.os.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn replacing_the_configured_vault_discards_writes_for_the_previous_vault() {
    let h = Harness::start(Vault::default().seal("first"));
    assert!(h.secrets.set("ssh:host", "pending"));
    let replacement = Vault::default().seal("second");
    let expected = replacement.clone();
    h.secrets.sync_config(move |_| Some((replacement, 0)));
    let replaced = h.update();
    assert!(!replaced.view.unlocked);
    assert_eq!(replaced.sealed.as_deref(), Some(expected.as_str()));
    let reopened = h.command(VaultCommand::Unlock("second".into()));
    assert!(reopened.view.unlocked);
    assert!(reopened.view.entries.is_empty());
    assert!(reopened.sealed.is_none());
}

#[test]
fn auto_lock_uses_the_unlock_instant_and_zero_disables_it() {
    let unlocked = Instant::now();
    let before = unlocked + Duration::from_secs(59);
    let at = unlocked + Duration::from_secs(60);
    let after = unlocked + Duration::from_secs(90);
    assert_eq!(lock_after(1, unlocked, before), Some(Duration::from_secs(1)));
    assert_eq!(lock_after(1, unlocked, at), Some(Duration::ZERO));
    assert_eq!(lock_after(1, unlocked, after), Some(Duration::ZERO));
    assert_eq!(lock_after(0, unlocked, after), None);
}

fn unlocked_store(age: Duration) -> (Store, Arc<OsStore>, Receiver<VaultUpdate>) {
    let os = Arc::new(OsStore::default());
    os.memory.set("ssh:host", "system");
    let mut vault = Vault::default();
    vault.set(PASSWORD, "ssh:host", "saved");
    let (events, updates) = unbounded();
    let store = Store {
        os: os.clone(),
        sealed: vault.seal("master"),
        minutes: 1,
        unlocked: Some((vault, "master".into(), Instant::now() - age)),
        pending: BTreeMap::new(),
        persist: Box::new(|_, _| false),
        notify: Box::new(move |update| { let _ = events.send(update); }),
    };
    (store, os, updates)
}

#[test]
fn an_idle_worker_locks_and_wakes_the_windows_at_the_deadline() {
    let (mut store, os, updates) = unlocked_store(Duration::from_millis(59_950));
    let (tx, rx) = unbounded();
    let worker = thread::spawn(move || store.run(rx));
    let expired = updates.recv_timeout(Duration::from_secs(5)).expect("auto-lock notification");
    assert!(expired.view.configured);
    assert!(!expired.view.unlocked);
    assert!(expired.view.entries.is_empty());
    assert!(expired.sealed.is_none());
    let secrets = Secrets { tx };
    assert_eq!(secrets.get("ssh:host"), None);
    assert_eq!(os.calls.load(Ordering::SeqCst), 0);
    drop(secrets);
    worker.join().unwrap();
}

#[test]
fn a_queued_read_at_expiry_cannot_access_the_vault_or_os_store() {
    let (mut store, os, updates) = unlocked_store(Duration::from_secs(60));
    let (tx, rx) = unbounded();
    let (done, read) = bounded(1);
    let secrets = Secrets { tx };
    secrets.read("ssh:host".into(), move |value| { done.send(value).unwrap(); });
    drop(secrets);
    store.run(rx);
    assert_eq!(read.try_recv().unwrap(), None);
    let expired = updates.try_recv().unwrap();
    assert!(!expired.view.unlocked);
    assert!(expired.view.entries.is_empty());
    assert_eq!(os.calls.load(Ordering::SeqCst), 0);
}
