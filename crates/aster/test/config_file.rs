use super::*;
use std::path::PathBuf;

struct ConfigDir(PathBuf);

impl ConfigDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("aster-vault-{}-{name}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    fn file(&self) -> PathBuf {
        self.0.join("config.toml")
    }
}

impl Drop for ConfigDir {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.file());
        let _ = fs::remove_file(self.file().with_extension("toml.lock"));
        let _ = fs::remove_dir(&self.0);
    }
}

#[test]
fn stale_settings_and_vault_updates_preserve_each_others_changes() {
    let dir = ConfigDir::new("merge");
    let path = dir.file();
    let mut settings = Config::default();
    let mut access = ConfigAccess::default();
    access.save_settings(&path, &mut settings, "", None).unwrap();
    access.save_vault(&path, "", "first", None).unwrap();
    settings.vault.remember_minutes = 35;
    access.save_settings(&path, &mut settings, "", None).unwrap();
    access.save_vault(&path, "first", "second", None).unwrap();
    assert!(access.save_vault(&path, "first", "", None).is_err());
    let saved = access.load(&path).unwrap();
    assert_eq!(saved.vault.remember_minutes, 35);
    assert_eq!(saved.vault.sealed, "second");
}

#[test]
fn unreadable_config_is_preserved_after_failed_writes() {
    let dir = ConfigDir::new("invalid");
    let path = dir.file();
    fs::write(&path, "[invalid").unwrap();
    let mut access = ConfigAccess::default();
    assert!(access.save_vault(&path, "", "new", None).is_err());
    assert!(access.save_settings(&path, &mut Config::default(), "", None).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"[invalid");
}

#[test]
fn future_config_is_preserved_by_settings_and_vault_writes() {
    let dir = ConfigDir::new("future-version");
    let path = dir.file();
    let original = "version = 50\nfuture = true\n[vault]\nsealed = 'current'\n";
    fs::write(&path, original).unwrap();
    let mut access = ConfigAccess::default();
    assert!(access.load(&path).unwrap().read_only());
    assert_eq!(access.save_vault(&path, "current", "new", None).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(access.save_settings(&path, &mut Config::default(), "", None).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn raw_save_repairs_invalid_toml_and_retains_user_formatting() {
    let dir = ConfigDir::new("raw-repair");
    let path = dir.file();
    let broken = "[terminal]\nfont-size = ]";
    fs::write(&path, broken).unwrap();
    let repaired = "# user layout\nversion = 1\n\n[terminal]\nfont-size = 19\n";
    let access = ConfigAccess::default();
    let config = access.save_raw(&path, Some(broken), repaired).unwrap();
    assert_eq!(config.terminal.font_size, 19.0);
    assert_eq!(fs::read_to_string(&path).unwrap(), repaired);
    assert!(access.save_raw(&path, Some(repaired), broken).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), repaired);
}

#[test]
fn raw_save_cannot_overwrite_a_changed_or_newer_file() {
    let dir = ConfigDir::new("raw-conflict");
    let path = dir.file();
    let old = "version = 1\n";
    let new = "version = 1\n[appearance]\nopacity = 0.6";
    fs::write(&path, new).unwrap();
    let access = ConfigAccess::default();
    assert_eq!(access.save_raw(&path, Some(old), old).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&path).unwrap(), new);
    let future = "version = 50\n";
    fs::write(&path, future).unwrap();
    assert_eq!(access.save_raw(&path, Some(future), old).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read_to_string(&path).unwrap(), future);
}

#[test]
fn atomic_replacement_rechecks_the_staged_source_before_installing_it() {
    let dir = ConfigDir::new("staged-source");
    let path = dir.file();
    fs::write(&path, "version = 42\n").unwrap();
    let rejected = write_text(&path, "version = 1\n", Some(Some("version = 1\n"))).unwrap_err();
    assert_eq!(rejected.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&path).unwrap(), "version = 42\n");
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
}

#[test]
fn vault_writes_preserve_the_raw_document_around_the_seal() {
    let dir = ConfigDir::new("raw-vault");
    let path = dir.file();
    let mut access = ConfigAccess::default();
    for original in [
        "version = 1\n# retained\ncustom = 17\n[vault]\nremember-minutes = 30\n",
        "version = 1\n# retained\nvault = { remember-minutes = 30 }\ncustom = 17\n",
        "version = 1\n# retained\nvault.remember-minutes = 30\ncustom = 17\n",
    ] {
        fs::write(&path, original).unwrap();
        access.save_vault(&path, "", "first", None).unwrap();
        access.save_vault(&path, "first", "second", None).unwrap();
        let source = fs::read_to_string(&path).unwrap();
        assert!(source.contains("# retained"));
        assert!(source.contains("custom = 17"));
        assert!(source.starts_with("version = 1\n"));
        let cfg = access.load(&path).unwrap();
        assert_eq!(cfg.vault.sealed, "second");
        assert_eq!(cfg.vault.remember_minutes, 30);
    }
}

#[test]
fn disk_diagnostics_include_location_without_echoing_the_config_line() {
    let dir = ConfigDir::new("diagnostic");
    let path = dir.file();
    fs::write(&path, "[terminal]\nfont-size = ] # private value\n").unwrap();
    let message = ConfigAccess::default().load(&path).unwrap_err().to_string();
    assert!(message.contains("Line 2, column 13"), "{message}");
    assert!(!message.contains("private value"));
}

#[test]
fn showing_defaults_refreshes_a_separate_file_and_preserves_the_active_config() {
    let dir = ConfigDir::new("defaults");
    let path = dir.file();
    let original = "[terminal]\nfont-size = 23";
    fs::write(&path, original).unwrap();
    let defaults = dir.0.join("defaults.toml");
    fs::write(&defaults, "old").unwrap();
    assert_eq!(write_defaults(&dir.0).unwrap(), defaults);
    assert_eq!(fs::read_to_string(&defaults).unwrap(), Config::defaults_text());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    fs::remove_file(defaults).unwrap();
}

#[test]
fn encrypted_config_round_trips_rekeys_and_returns_to_toml() {
    let dir = ConfigDir::new("encrypted");
    let path = dir.file();
    let sealed = aster_vault::Vault::default().seal("old");
    let mut config = Config::default();
    config.terminal.open_on_start = false;
    let mut access = ConfigAccess::default();
    access.save_settings(&path, &mut config, &sealed, None).unwrap();
    access.set_encryption(&path, &sealed, "old", true).unwrap();
    let encrypted = fs::read_to_string(&path).unwrap();
    assert!(aster_vault::config::is_encrypted(&encrypted));
    assert!(!encrypted.contains("open-on-start"));
    let envelope: serde_json::Value = serde_json::from_str(&encrypted).unwrap();
    let encrypted = format!(" \r\n{}", serde_json::to_string_pretty(&envelope).unwrap().replace('\n', "\r\n"));
    fs::write(&path, &encrypted).unwrap();
    let mut reopened = ConfigAccess::default();
    assert!(reopened.unlock(&path, "wrong").is_err());
    assert!(reopened.save_settings(&path, &mut Config::default(), "", None).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), encrypted);
    let mut loaded = reopened.unlock(&path, "old").unwrap();
    assert!(!loaded.terminal.open_on_start);
    loaded.vault.remember_minutes = 9;
    reopened.save_settings(&path, &mut loaded, &sealed, None).unwrap();
    assert!(aster_vault::config::is_encrypted(&fs::read_to_string(&path).unwrap()));
    let next = aster_vault::Vault::default().seal("new");
    reopened.save_vault(&path, &sealed, &next, Some("new")).unwrap();
    let mut restarted = ConfigAccess::default();
    assert!(restarted.unlock(&path, "old").is_err());
    let config = restarted.unlock(&path, "new").unwrap();
    assert_eq!(config.vault.remember_minutes, 9);
    assert_eq!(config.vault.sealed, next);
    restarted.set_encryption(&path, &next, "new", false).unwrap();
    let clear = fs::read_to_string(&path).unwrap();
    assert!(!aster_vault::config::is_encrypted(&clear));
    assert_eq!(Config::parse(&clear).unwrap().vault.remember_minutes, 9);
}

#[test]
fn raw_editor_loads_plaintext_only_after_unlock_and_saves_ciphertext() {
    let dir = ConfigDir::new("raw-encrypted");
    let path = dir.file();
    let sealed = Vault::default().seal("master");
    let mut access = ConfigAccess::default();
    access.save_settings(&path, &mut Config::default(), &sealed, None).unwrap();
    access.set_encryption(&path, &sealed, "master", true).unwrap();

    let mut reopened = ConfigAccess::default();
    assert_eq!(reopened.load_document(&path).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    reopened.unlock(&path, "master").unwrap();
    let (original, config) = reopened.load_document(&path).unwrap();
    assert!(config.unwrap().vault.encrypt_config);
    let edited = format!("# retained\n{original}\n[terminal]\nfont-size = 19\n");
    let config = reopened.save_raw(&path, Some(&original), &edited).unwrap();
    assert_eq!(config.terminal.font_size, 19.0);
    let encrypted = fs::read_to_string(&path).unwrap();
    assert!(is_encrypted(&encrypted));
    assert!(!encrypted.contains("retained"));
    let (current, _) = reopened.load_document(&path).unwrap();
    assert_eq!(current, edited);
    assert_eq!(reopened.save_raw(&path, Some(&original), &edited).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    let without_vault = "version = 1\n";
    assert_eq!(reopened.save_raw(&path, Some(&edited), without_vault).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read_to_string(&path).unwrap(), encrypted);
}

#[test]
fn encrypted_future_config_cannot_be_overwritten_or_unprotected() {
    let dir = ConfigDir::new("future-encrypted");
    let path = dir.file();
    let sealed = Vault::default().seal("master");
    let cipher = ConfigCipher::new("master");
    let original = format!("version = 50\n[vault]\nsealed = '{sealed}'\n");
    let encrypted = cipher.encrypt(&original);
    fs::write(&path, &encrypted).unwrap();
    let mut access = ConfigAccess::default();
    assert!(access.unlock(&path, "master").unwrap().read_only());
    assert_eq!(access.save_raw(&path, Some(&original), "version = 1\n").unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(access.save_vault(&path, &sealed, "", None).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(access.save_settings(&path, &mut Config::default(), &sealed, None).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert!(access.set_encryption(&path, &sealed, "master", false).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), encrypted);
}

#[test]
fn cancelled_startup_keeps_the_encrypted_file_byte_for_byte() {
    let dir = ConfigDir::new("skip-encrypted");
    let path = dir.file();
    let sealed = aster_vault::Vault::default().seal("master");
    let mut access = ConfigAccess::default();
    access.save_settings(&path, &mut Config::default(), &sealed, None).unwrap();
    access.set_encryption(&path, &sealed, "master", true).unwrap();
    let encrypted = fs::read_to_string(&path).unwrap();
    let mut startup = ConfigAccess::default();
    startup.unlock(&path, "master").unwrap();
    startup.skip();
    assert!(startup.save_settings(&path, &mut Config::default(), "", None).is_err());
    assert!(startup.save_vault(&path, &sealed, "", None).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), encrypted);
}

#[test]
fn concurrent_settings_save_cannot_overwrite_a_new_encrypted_config() {
    let dir = ConfigDir::new("lock");
    let path = dir.file();
    let sealed = Vault::default().seal("master");
    let access = ConfigAccess::default();
    access.save_settings(&path, &mut Config::default(), &sealed, None).unwrap();
    let lock = lock_config(&path).unwrap();
    let (started, start) = std::sync::mpsc::channel();
    let (finished, finish) = std::sync::mpsc::channel();
    let other_path = path.clone();
    let other = std::thread::spawn(move || {
        started.send(()).unwrap();
        let result = ConfigAccess::default().save_settings(&other_path, &mut Config::default(), "", None);
        finished.send(result).unwrap();
    });
    start.recv().unwrap();
    assert!(matches!(finish.recv_timeout(std::time::Duration::from_millis(100)), Err(std::sync::mpsc::RecvTimeoutError::Timeout)));
    let mut config = access.load(&path).unwrap();
    config.vault.encrypt_config = true;
    let cipher = ConfigCipher::new("master");
    let current = fs::read_to_string(&path).unwrap();
    write_config(&path, &config, Some(&cipher), Some(&current)).unwrap();
    drop(lock);
    assert!(finish.recv_timeout(std::time::Duration::from_secs(5)).unwrap().is_err());
    other.join().unwrap();
    assert!(is_encrypted(&fs::read_to_string(path).unwrap()));
}
