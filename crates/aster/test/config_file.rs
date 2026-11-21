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
    access.save_settings(&path, &mut settings, "").unwrap();
    access.save_vault(&path, "", "first", None).unwrap();
    settings.vault.remember_minutes = 35;
    access.save_settings(&path, &mut settings, "").unwrap();
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
    assert!(access.save_settings(&path, &mut Config::default(), "").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"[invalid");
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
    assert_eq!(fs::read_to_string(&defaults).unwrap(), Config::default().to_text());
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
    access.save_settings(&path, &mut config, &sealed).unwrap();
    access.set_encryption(&path, &sealed, "old", true).unwrap();
    let encrypted = fs::read_to_string(&path).unwrap();
    assert!(aster_vault::config::is_encrypted(&encrypted));
    assert!(!encrypted.contains("open-on-start"));
    let envelope: serde_json::Value = serde_json::from_str(&encrypted).unwrap();
    let encrypted = format!(" \r\n{}", serde_json::to_string_pretty(&envelope).unwrap().replace('\n', "\r\n"));
    fs::write(&path, &encrypted).unwrap();
    let mut reopened = ConfigAccess::default();
    assert!(reopened.unlock(&path, "wrong").is_err());
    assert!(reopened.save_settings(&path, &mut Config::default(), "").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), encrypted);
    let mut loaded = reopened.unlock(&path, "old").unwrap();
    assert!(!loaded.terminal.open_on_start);
    loaded.vault.remember_minutes = 9;
    reopened.save_settings(&path, &mut loaded, &sealed).unwrap();
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
fn cancelled_startup_keeps_the_encrypted_file_byte_for_byte() {
    let dir = ConfigDir::new("skip-encrypted");
    let path = dir.file();
    let sealed = aster_vault::Vault::default().seal("master");
    let mut access = ConfigAccess::default();
    access.save_settings(&path, &mut Config::default(), &sealed).unwrap();
    access.set_encryption(&path, &sealed, "master", true).unwrap();
    let encrypted = fs::read_to_string(&path).unwrap();
    let mut startup = ConfigAccess::default();
    startup.unlock(&path, "master").unwrap();
    startup.skip();
    assert!(startup.save_settings(&path, &mut Config::default(), "").is_err());
    assert!(startup.save_vault(&path, &sealed, "", None).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), encrypted);
}

#[test]
fn concurrent_settings_save_cannot_overwrite_a_new_encrypted_config() {
    let dir = ConfigDir::new("lock");
    let path = dir.file();
    let sealed = Vault::default().seal("master");
    let access = ConfigAccess::default();
    access.save_settings(&path, &mut Config::default(), &sealed).unwrap();
    let lock = lock_config(&path).unwrap();
    let (started, start) = std::sync::mpsc::channel();
    let (finished, finish) = std::sync::mpsc::channel();
    let other_path = path.clone();
    let other = std::thread::spawn(move || {
        started.send(()).unwrap();
        let result = ConfigAccess::default().save_settings(&other_path, &mut Config::default(), "");
        finished.send(result).unwrap();
    });
    start.recv().unwrap();
    assert!(matches!(finish.recv_timeout(std::time::Duration::from_millis(100)), Err(std::sync::mpsc::RecvTimeoutError::Timeout)));
    let mut config = access.load(&path).unwrap();
    config.vault.encrypt_config = true;
    let cipher = ConfigCipher::new("master");
    write_config(&path, &config, Some(&cipher)).unwrap();
    drop(lock);
    assert!(finish.recv_timeout(std::time::Duration::from_secs(5)).unwrap().is_err());
    other.join().unwrap();
    assert!(is_encrypted(&fs::read_to_string(path).unwrap()));
}
