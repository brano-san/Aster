use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.tmp");
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!("sync-test-{}-{}", std::process::id(), unix_nanos()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn folder_lists_reads_replaces_and_deletes_snapshots() {
    let dir = TempDir::new();
    let store = Store::folder(&dir.0).unwrap();
    let first = sync::extract_at("", &[], "desk", 10).unwrap();
    let second = sync::extract_at("", &[], "travel", 11).unwrap();
    assert!(store.list().unwrap().is_empty());
    store.write("Shared settings", &first, true, "desk").unwrap();
    assert!(store.write("Shared settings", &second, true, "travel").is_err());
    assert_eq!(store.read("Shared settings").unwrap(), Some(first));
    store.write("Shared settings", &second, false, "travel").unwrap();
    let entries = store.list().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "Shared settings");
    assert_eq!(entries[0].machine, "travel");
    assert!(entries[0].modified > 0);
    assert_eq!(store.read("Shared settings").unwrap(), Some(second));
    assert_eq!(fs::read_dir(dir.0.join("aster-sync")).unwrap().count(), 1);
    store.delete("Shared settings", "desk").unwrap();
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn folder_rejects_names_that_escape_or_alias_the_store() {
    let dir = TempDir::new();
    let store = Store::folder(&dir.0).unwrap();
    for name in ["", ".", "../outside", "nested/file", r"..\outside", "NUL", "COM1.port", "tail.", "a:b"] {
        assert!(store.read(name).is_err(), "{name}");
        assert!(store.write(name, "", false, "desk").is_err(), "{name}");
        assert!(store.delete(name, "desk").is_err(), "{name}");
    }
    assert!(!dir.0.join("outside.toml").exists());
}

#[test]
fn folder_rejects_an_automatic_write_based_on_an_old_snapshot() {
    let dir = TempDir::new();
    let store = Store::folder(&dir.0).unwrap();
    let original = sync::extract_at("", &[], "first", 10).unwrap();
    store.write("Shared", &original, true, "first").unwrap();
    let updated = sync::extract_at("", &[], "second", 12).unwrap();
    store.write("Shared", &updated, false, "second").unwrap();
    assert!(matches!(store.write_snapshot("Shared", &original, false, "first", Some(Some(&original))), Err(Issue::StoreChanged)));
    assert_eq!(store.read("Shared").unwrap(), Some(updated));
}

#[test]
fn git_round_trip_keeps_the_store_layout() {
    let dir = TempDir::new();
    let remote = dir.0.join("remote");
    fs::create_dir(&remote).unwrap();
    let Ok(probe) = Command::new("git").arg("--version").output() else { return };
    assert!(probe.status.success());
    let init = git(&remote, "init", ["init", "--bare", "--initial-branch=main"]).unwrap();
    assert!(init.status.success());
    let repo = dir.0.join("config");
    fs::create_dir(&repo).unwrap();
    let store = Store::git(&repo, remote.to_str().unwrap()).unwrap();
    let checkout = store.root.parent().unwrap();
    git_ok(checkout, "config", ["config", "user.name", "Aster Test"]).unwrap();
    git_ok(checkout, "config", ["config", "user.email", "aster-test@example.invalid"]).unwrap();
    let text = sync::extract_at("", &[], "desk", 10).unwrap();
    store.write("Shared", &text, true, "desk").unwrap();
    let second = dir.0.join("second");
    fs::create_dir(&second).unwrap();
    let store = Store::git(&second, remote.to_str().unwrap()).unwrap();
    assert_eq!(store.read("Shared").unwrap(), Some(text));
}

#[test]
fn git_recovers_a_rejected_push_and_keeps_both_snapshots() {
    let dir = TempDir::new();
    let remote = dir.0.join("remote");
    let first = dir.0.join("first");
    let second = dir.0.join("second");
    for path in [&remote, &first, &second] { fs::create_dir(path).unwrap(); }
    if Command::new("git").arg("--version").output().is_err() { return; }
    git_ok(&remote, "init", ["init", "--bare", "--initial-branch=main"]).unwrap();
    let repository = remote.to_str().unwrap();
    let identity = |store: &Store| {
        let checkout = store.root.parent().unwrap();
        git_ok(checkout, "config", ["config", "user.name", "Aster Test"]).unwrap();
        git_ok(checkout, "config", ["config", "user.email", "aster-test@example.invalid"]).unwrap();
    };
    let a = Store::git(&first, repository).unwrap();
    identity(&a);
    let original = sync::extract_at("", &[], "first", 10).unwrap();
    a.write("Shared", &original, true, "first").unwrap();
    let b = Store::git(&second, repository).unwrap();
    identity(&b);
    let winner = sync::extract_at("", &[], "first", 11).unwrap();
    a.write("Shared", &winner, false, "first").unwrap();
    let loser = sync::extract_at("", &[], "second", 12).unwrap();
    assert!(matches!(b.write("Shared", &loser, false, "second"), Err(Issue::Git("push"))));
    let b = Store::git(&second, repository).unwrap();
    assert_eq!(b.read("Shared").unwrap(), Some(winner));
    let conflict = b.list().unwrap().into_iter().find(|entry| entry.name.starts_with("Shared (conflict ")).unwrap();
    assert_eq!(b.read(&conflict.name).unwrap(), Some(loser));
}

#[test]
fn automatic_folder_conflict_preserves_local_and_applies_remote() {
    use aster_core::config::Config;
    use aster_core::sync::Part;
    let dir = TempDir::new();
    let store = Store::folder(&dir.0).unwrap();
    let path = dir.0.join("config.toml");
    let access = Mutex::new(crate::config_file::ConfigAccess::default());
    let preferences = Preferences {
        backend: Backend::Folder, folder: dir.0.to_string_lossy().into_owned(), name: "Shared".into(),
        parts: vec![Part::Appearance], automatic: true, ..Default::default()
    };
    let mut config = Config { sync: preferences, ..Default::default() };
    config.appearance.opacity = 0.5;
    fs::write(&path, config.to_text()).unwrap();
    perform(&path, &access, "Desk", &config.sync, Operation::Automatic).unwrap();

    let text = fs::read_to_string(&path).unwrap();
    let mut changed = Config::parse(&text).unwrap();
    changed.appearance.opacity = 0.6;
    fs::write(&path, changed.to_text()).unwrap();
    let remote = sync::extract_at("[appearance]\nopacity = 0.8\n", &[Part::Appearance], "Laptop", 10).unwrap();
    store.write("Shared", &remote, false, "Laptop").unwrap();
    let report = perform(&path, &access, "Desk", &changed.sync, Operation::Automatic).unwrap();
    let conflict = report.conflict.unwrap();
    assert!(conflict.starts_with("Shared (conflict "));
    let saved = store.read(&conflict).unwrap().unwrap();
    assert_eq!(Config::parse(&sync::merge("", &saved, &[Part::Appearance]).unwrap()).unwrap().appearance.opacity, 0.6);
    assert_eq!(store.read("Shared").unwrap(), Some(remote));
    assert_eq!(Config::parse(&fs::read_to_string(&path).unwrap()).unwrap().appearance.opacity, 0.8);
    let next = perform(&path, &access, "Desk", &changed.sync, Operation::Automatic).unwrap();
    assert!(next.conflict.is_none());
    assert!(next.saved.is_none());
}

#[test]
fn automatic_upload_includes_local_changes_outside_the_remote_parts() {
    use aster_core::config::Config;
    use aster_core::sync::Part;
    let dir = TempDir::new();
    let path = dir.0.join("config.toml");
    let store = Store::folder(&dir.0).unwrap();
    let access = Mutex::new(crate::config_file::ConfigAccess::default());
    let mut config = Config { sync: Preferences {
        backend: Backend::Folder, folder: dir.0.to_string_lossy().into_owned(), name: "Shared".into(),
        parts: vec![Part::Appearance, Part::Terminal], automatic: true, ..Default::default()
    }, ..Default::default() };
    fs::write(&path, config.to_text()).unwrap();
    let remote = sync::extract_at("", &[Part::Appearance], "Laptop", 10).unwrap();
    store.write("Shared", &remote, true, "Laptop").unwrap();
    perform(&path, &access, "Desk", &config.sync, Operation::Automatic).unwrap();
    config = Config::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    config.terminal.scrollback_lines = 535;
    fs::write(&path, config.to_text()).unwrap();
    perform(&path, &access, "Desk", &config.sync, Operation::Automatic).unwrap();
    let snapshot = store.read("Shared").unwrap().unwrap();
    let merged = sync::merge("", &snapshot, &[Part::Terminal]).unwrap();
    assert_eq!(Config::parse(&merged).unwrap().terminal.scrollback_lines, 535);
}

#[test]
fn saving_an_edit_keeps_settings_downloaded_since_the_edit_started() {
    use aster_core::config::Config;
    use aster_core::sync::Part;
    let dir = TempDir::new();
    let store = Store::folder(&dir.0).unwrap();
    let path = dir.0.join("config.toml");
    let access = Mutex::new(crate::config_file::ConfigAccess::default());
    let base = Config { sync: Preferences {
        backend: Backend::Folder, folder: dir.0.to_string_lossy().into_owned(),
        name: "Shared".into(), parts: vec![Part::Appearance], ..Default::default()
    }, ..Default::default() };
    fs::write(&path, base.to_text()).unwrap();
    let remote = sync::extract_at("[appearance]\nopacity = 0.8\n", &[Part::Appearance], "Laptop", 10).unwrap();
    store.write("Shared", &remote, false, "Laptop").unwrap();
    let mut editing = base.clone();
    editing.terminal.scrollback_lines = 1200;
    perform(&path, &access, "Desk", &base.sync, Operation::Download("Shared".into())).unwrap();
    access.lock().unwrap().save_settings(&path, &mut editing, "", Some(&base)).unwrap();
    let saved = Config::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved.appearance.opacity, 0.8);
    assert_eq!(saved.terminal.scrollback_lines, 1200);
    assert!(saved.sync.last_synced > 0);
}
