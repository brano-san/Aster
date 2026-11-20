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
        let _ = fs::remove_dir(&self.0);
    }
}

#[test]
fn stale_settings_and_vault_updates_preserve_each_others_changes() {
    let dir = ConfigDir::new("merge");
    let path = dir.file();
    let mut settings = Config::default();
    save_settings(&path, &mut settings, "").unwrap();
    save_vault(&path, "", "first").unwrap();
    settings.vault.remember_minutes = 35;
    save_settings(&path, &mut settings, "").unwrap();
    save_vault(&path, "first", "second").unwrap();
    assert!(save_vault(&path, "first", "").is_err());
    let saved = read(&path).unwrap();
    assert_eq!(saved.vault.remember_minutes, 35);
    assert_eq!(saved.vault.sealed, "second");
}

#[test]
fn unreadable_config_is_preserved_after_failed_writes() {
    let dir = ConfigDir::new("invalid");
    let path = dir.file();
    fs::write(&path, "[invalid").unwrap();
    assert!(save_vault(&path, "", "new").is_err());
    assert!(save_settings(&path, &mut Config::default(), "").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"[invalid");
}
