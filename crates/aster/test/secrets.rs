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
        Self::with_minutes(initial, 15)
    }

    fn with_minutes(initial: String, minutes: u32) -> Self {
        let os = Arc::new(OsStore::default());
        let writable = Arc::new(AtomicBool::new(true));
        let sealed = Arc::new(Mutex::new(initial.clone()));
        let (can_write, disk) = (writable.clone(), sealed.clone());
        let (updates_tx, updates) = unbounded();
        let (secrets, worker) = Secrets::start(os.clone(), initial, minutes, move |expected, next, _| {
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
        self.secrets.command(command, |_| {});
        self.update()
    }
}

#[test]
fn an_unconfigured_store_delegates_every_operation_to_the_os() {
    let h = Harness::with_minutes(String::new(), 0);
    let route = WindowSecrets::new(h.secrets.clone(), |_, _| panic!("system route requested a master passphrase"));
    let password = aster_ssh::secrets::password_key("me", "host", 22);
    let passphrase = aster_ssh::secrets::passphrase_key(std::path::Path::new("id_ed25519"));
    let key_file = aster_ssh::secrets::key_file_key("SHA256:id");
    for key in ["sudo:local", &password, &passphrase, &key_file] {
        assert!(route.set(key, "system"));
        assert_eq!(route.get(key).as_deref(), Some("system"));
        if key == key_file {
            assert_eq!(route.key_file("SHA256:id").as_deref(), Some("system"));
        }
        route.delete(key);
        assert_eq!(route.get(key), None);
    }
    assert_eq!(h.os.calls.load(Ordering::SeqCst), 17);
    assert!(h.sealed.lock().unwrap().is_empty());
}

#[test]
fn locked_writes_and_deletes_are_applied_only_after_successful_unlock() {
    let mut initial = Vault::default();
    initial.set("password", "key:a", "old");
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
    assert_eq!(saved.view.entries, vec![VaultEntry { kind: "other".into(), key: "sudo:new".into(), label: "sudo:new".into() }]);
    let vault = Vault::open(&saved.sealed.unwrap(), "master").unwrap();
    assert_eq!(vault.get("password", "key:a"), None);
    assert_eq!(vault.get("password", "sudo:new"), Some("latest"));
    assert_eq!(h.secrets.get("sudo:new").as_deref(), Some("latest"));
    assert_eq!(h.os.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn unlocked_writes_reseal_and_failed_writes_leave_the_previous_value() {
    let h = Harness::start(String::new());
    assert!(h.command(VaultCommand::Create("master".into())).view.unlocked);
    assert!(h.secrets.set("ssh:host", "first"));
    let saved = h.update().sealed.unwrap();
    assert_eq!(Vault::open(&saved, "master").unwrap().get("password", "ssh:host"), Some("first"));
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
    initial.set("key-file", "host", "token");
    initial.set("password", "host", "password");
    let h = Harness::start(initial.seal("old"));
    h.os.memory.set("host", "system");
    assert_eq!(h.command(VaultCommand::Erase(None)).view.error, Some(VaultIssue::Locked));
    assert!(h.command(VaultCommand::Unlock("old".into())).view.unlocked);
    assert_eq!(h.command(VaultCommand::Change { old: "bad".into(), new: "new".into() }).view.error, Some(VaultIssue::WrongPassphrase));
    let changed = h.command(VaultCommand::Change { old: "old".into(), new: "new".into() }).sealed.unwrap();
    assert!(Vault::open(&changed, "old").is_err());
    assert_eq!(Vault::open(&changed, "new").unwrap(), initial);
    assert!(h.command(VaultCommand::Lock).view.entries.is_empty());
    assert!(h.command(VaultCommand::Unlock("new".into())).view.unlocked);
    let entry = VaultEntry { kind: "key-file".into(), key: "host".into(), label: "host".into() };
    h.secrets.manage(VaultAction::Delete(entry), None, |_| {});
    let removed = h.update();
    assert_eq!(removed.view.entries, vec![VaultEntry { kind: "other".into(), key: "host".into(), label: "host".into() }]);
    assert_eq!(h.secrets.get("host").as_deref(), Some("password"));
    let erased = h.command(VaultCommand::Erase(None));
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
fn auto_lock_uses_the_unlock_instant_and_zero_expires_immediately() {
    let unlocked = Instant::now();
    let before = unlocked + Duration::from_secs(59);
    let at = unlocked + Duration::from_secs(60);
    let after = unlocked + Duration::from_secs(90);
    assert_eq!(lock_after(1, unlocked, before), Duration::from_secs(1));
    assert_eq!(lock_after(1, unlocked, at), Duration::ZERO);
    assert_eq!(lock_after(1, unlocked, after), Duration::ZERO);
    assert_eq!(lock_after(0, unlocked, after), Duration::ZERO);
}

fn unlocked_store(age: Duration) -> (Store, Arc<OsStore>, Receiver<VaultUpdate>) {
    let os = Arc::new(OsStore::default());
    os.memory.set("ssh:host", "system");
    let mut vault = Vault::default();
    vault.set("password", "ssh:host", "saved");
    let (events, updates) = unbounded();
    let store = Store {
        os: os.clone(),
        sealed: vault.seal("master"),
        minutes: 1,
        unlocked: Some((vault, "master".into(), Instant::now() - age)),
        preview: None,
        known: None,
        pending: BTreeMap::new(),
        persist: Box::new(|_, _, _| false),
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

#[test]
fn management_renames_replaces_exports_and_deletes_a_key_file() {
    let mut initial = Vault::default();
    initial.set("key-file", "key-file:SHA256:id", "old\nprivate key\n");
    let h = Harness::start(initial.seal("master"));
    let entry = h.command(VaultCommand::Unlock("master".into())).view.entries.remove(0);
    let manage = |action| {
        let (tx, rx) = bounded(1);
        h.secrets.manage(action, None, move |result| { tx.send(result).unwrap(); });
        rx.recv_timeout(Duration::from_secs(15)).unwrap()
    };
    assert!(matches!(manage(VaultAction::Rename { entry: entry.clone(), label: "work key".into() }), Ok(VaultReply::Applied)));
    assert_eq!(h.update().view.entries[0].label, "work key");
    assert!(matches!(manage(VaultAction::Replace { entry: entry.clone(), value: "new\nkey\n".into() }), Ok(VaultReply::Applied)));
    let changed = h.update().sealed.unwrap();
    assert_eq!(Vault::open(&changed, "master").unwrap().key_file("work key"), Some("new\nkey\n"));
    assert_eq!(h.secrets.key_file("SHA256:id").as_deref(), Some("new\nkey\n"));
    assert!(matches!(manage(VaultAction::Show(entry.clone())), Ok(VaultReply::Value(value)) if value == "new\nkey\n"));
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("aster-vault-export-{}-{stamp}", std::process::id()));
    let name = path.to_str().unwrap().to_owned();
    assert!(matches!(manage(VaultAction::Export { entry: entry.clone(), path: name.clone() }), Ok(VaultReply::Applied)));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\nkey\n");
    assert!(matches!(manage(VaultAction::Export { entry: entry.clone(), path: name }), Err(VaultIssue::ExportFailed)));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    std::fs::remove_file(path).unwrap();
    assert!(matches!(manage(VaultAction::Delete(entry)), Ok(VaultReply::Applied)));
    assert!(h.update().view.entries.is_empty());
    assert_eq!(h.secrets.key_file("work key"), None);
}

#[test]
fn zero_minutes_requires_authentication_for_every_secret_access() {
    let mut vault = Vault::default();
    vault.set("other", "sudo:local", "saved");
    let h = Harness::with_minutes(vault.seal("master"), 0);
    let entry = h.command(VaultCommand::Unlock("master".into())).view.entries.remove(0);
    assert_eq!(h.secrets.get("sudo:local"), None);
    let show = |passphrase: Option<&str>| {
        let (tx, rx) = bounded(1);
        h.secrets.manage(VaultAction::Show(entry.clone()), passphrase.map(str::to_owned), move |result| { tx.send(result).unwrap(); });
        rx.recv_timeout(Duration::from_secs(15)).unwrap()
    };
    assert!(matches!(show(None), Err(VaultIssue::Locked)));
    assert!(matches!(show(Some("wrong")), Err(VaultIssue::WrongPassphrase)));
    assert!(matches!(show(Some("master")), Ok(VaultReply::Value(v)) if v == "saved"));
    assert!(matches!(show(None), Err(VaultIssue::Locked)));
    assert_eq!(h.secrets.get("sudo:local"), None);
}

#[test]
fn one_time_authentication_persists_pending_writes_before_deleting() {
    let mut vault = Vault::default();
    vault.set("other", "sudo:local", "saved");
    let h = Harness::with_minutes(vault.seal("master"), 0);
    let entry = h.command(VaultCommand::Unlock("master".into())).view.entries.remove(0);
    assert!(h.secrets.set("sudo:local", "queued"));
    let (tx, rx) = bounded(1);
    h.secrets.manage(VaultAction::Delete(entry), Some("master".into()), move |result| { tx.send(result).unwrap(); });
    assert!(matches!(rx.recv_timeout(Duration::from_secs(15)).unwrap(), Ok(VaultReply::Applied)));
    h.update();
    let deleted = h.update().sealed.unwrap();
    assert!(Vault::open(&deleted, "master").unwrap().secrets.is_empty());
    assert!(h.command(VaultCommand::Unlock("master".into())).view.entries.is_empty());
}

#[test]
fn interactive_secret_use_retries_and_requires_fresh_authorization() {
    let mut vault = Vault::default();
    vault.set("ssh-password", "ssh:me@box:22", "saved");
    let h = Harness::with_minutes(vault.seal("master"), 0);
    let (asks, requests) = bounded(1);
    let route = WindowSecrets::new(h.secrets.clone(), move |issue, reply| { asks.send((issue, reply)).unwrap(); });
    let (finished, result) = bounded(1);
    route.use_secret(SecretUse::Read("ssh:me@box:22".into()), move |value| { finished.send(value).unwrap(); });
    let (issue, reply) = requests.recv_timeout(Duration::from_secs(15)).unwrap();
    assert_eq!(issue, None);
    reply(Some("wrong".into()));
    let (issue, reply) = requests.recv_timeout(Duration::from_secs(15)).unwrap();
    assert_eq!(issue, Some(VaultIssue::WrongPassphrase));
    reply(Some("master".into()));
    assert_eq!(result.recv_timeout(Duration::from_secs(15)).unwrap(), Ok(Some("saved".into())));
    let (finished, result) = bounded(1);
    route.use_secret(SecretUse::Read("ssh:me@box:22".into()), move |value| { finished.send(value).unwrap(); });
    let (issue, reply) = requests.recv_timeout(Duration::from_secs(15)).unwrap();
    assert_eq!(issue, None);
    reply(None);
    assert_eq!(result.recv_timeout(Duration::from_secs(15)).unwrap(), Err(VaultIssue::Locked));
    assert_eq!(h.secrets.get("ssh:me@box:22"), None);
}

#[test]
fn key_file_import_resolves_the_actual_public_key_fingerprint() {
    let initial = Vault::default().seal("master");
    let h = Harness::start(initial);
    h.command(VaultCommand::Unlock("master".into()));
    let value = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\nQyNTUxOQAAACAZf2sj4WyFMsaryDj6zV6nib4MdrKSAzQDm/qLPTaNYQAAAJhBQkNEQUJD\nRAAAAAtzc2gtZWQyNTUxOQAAACAZf2sj4WyFMsaryDj6zV6nib4MdrKSAzQDm/qLPTaNYQ\nAAAEAqKioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKhl/ayPhbIUyxqvIOPrNXqeJ\nvgx2spIDNAOb+os9No1hAAAAD2FzdGVyLTEwMDQtdGVzdAECAwQFBg==\n-----END OPENSSH PRIVATE KEY-----\n";
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("aster-vault-import-{}-{stamp}", std::process::id()));
    std::fs::write(&path, value).unwrap();
    let (reply, result) = bounded(1);
    h.secrets.manage(VaultAction::Import { label: "work key".into(), path: path.to_str().unwrap().to_owned() }, None, move |r| { reply.send(r).unwrap(); });
    assert!(matches!(result.recv_timeout(Duration::from_secs(15)).unwrap(), Ok(VaultReply::Applied)));
    std::fs::remove_file(path).unwrap();
    assert_eq!(h.secrets.key_file("work key").as_deref(), Some(value));
    assert_eq!(h.secrets.key_file("SHA256:1Lt0B6m58n9i5yp9hABit7qhCJ/7aR7kRxt8QRziO8k").as_deref(), Some(value));
}
