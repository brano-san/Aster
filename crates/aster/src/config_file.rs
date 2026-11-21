use aster_core::config::Config;
use aster_core::settings::VaultIssue;
use aster_vault::config::{is_encrypted, ConfigCipher};
use aster_vault::{Vault, VaultError};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct ConfigAccess {
    cipher: Option<ConfigCipher>,
    skipped: bool,
}

fn lock_config(path: &Path) -> io::Result<fs::File> {
    let target = match path.canonicalize() {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_owned(),
        Err(error) => return Err(error),
    };
    let path = target.with_extension("toml.lock");
    #[cfg(windows)]
    use aster_platform_windows::config_lock::lock;
    #[cfg(target_os = "linux")]
    use aster_platform_linux::config_lock::lock;
    lock(&path)
}

pub fn read_initial(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            let _lock = lock_config(path)?;
            match fs::read_to_string(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let config = Config::default();
                    write_config(path, &config, None)?;
                    Ok(config.to_text())
                }
                result => result,
            }
        }
        result => result,
    }
}

fn parse(text: &str) -> io::Result<Config> {
    Config::parse(text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn vault_issue(error: VaultError) -> VaultIssue {
    match error {
        VaultError::WrongPassphrase => VaultIssue::WrongPassphrase,
        VaultError::Corrupted(_) => VaultIssue::Corrupted,
        VaultError::Unsupported(_) => VaultIssue::Unsupported,
    }
}

impl ConfigAccess {
    pub fn load(&self, path: &Path) -> io::Result<Config> {
        let text = fs::read_to_string(path)?;
        let encrypted = is_encrypted(&text);
        let mut config = if encrypted {
            let cipher = self.cipher.as_ref().ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "config is encrypted"))?;
            let clear = cipher.decrypt(&text).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "could not decrypt config"))?;
            parse(&clear)?
        } else {
            if self.cipher.is_some() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "config encryption changed on disk"));
            }
            parse(&text)?
        };
        config.vault.encrypt_config = encrypted;
        Ok(config)
    }

    pub fn unlock(&mut self, path: &Path, passphrase: &str) -> Result<Config, VaultIssue> {
        if self.skipped {
            return Err(VaultIssue::Locked);
        }
        let text = fs::read_to_string(path).map_err(|_| VaultIssue::ConfigReadFailed)?;
        let (clear, cipher) = ConfigCipher::open(&text, passphrase).map_err(vault_issue)?;
        let mut config = parse(&clear).map_err(|_| VaultIssue::ConfigReadFailed)?;
        config.vault.encrypt_config = true;
        self.cipher = Some(cipher);
        Ok(config)
    }

    pub fn skip(&mut self) {
        self.cipher = None;
        self.skipped = true;
    }

    fn writable(&self) -> io::Result<()> {
        if self.skipped {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "encrypted config was skipped"))
        } else {
            Ok(())
        }
    }

    pub fn save_settings(&self, path: &Path, config: &mut Config, sealed: &str) -> io::Result<()> {
        self.writable()?;
        let _lock = lock_config(path)?;
        config.vault.sealed = match self.load(path) {
            Ok(disk) => disk.vault.sealed,
            Err(e) if e.kind() == io::ErrorKind::NotFound && self.cipher.is_none() => sealed.to_owned(),
            Err(e) => return Err(e),
        };
        config.vault.encrypt_config = self.cipher.is_some();
        write_config(path, config, self.cipher.as_ref())
    }

    pub fn save_vault(&mut self, path: &Path, expected: &str, sealed: &str, rekey: Option<&str>) -> io::Result<bool> {
        self.writable()?;
        let _lock = lock_config(path)?;
        let mut config = self.load(path)?;
        if config.vault.sealed != expected {
            return Err(io::Error::other("vault changed on disk"));
        }
        config.vault.sealed = sealed.to_owned();
        if self.cipher.is_some() && sealed.is_empty() {
            config.vault.encrypt_config = false;
            write_config(path, &config, None)?;
            self.cipher = None;
        } else if let Some(passphrase) = rekey.filter(|_| self.cipher.is_some()) {
            let cipher = ConfigCipher::new(passphrase);
            write_config(path, &config, Some(&cipher))?;
            self.cipher = Some(cipher);
        } else {
            write_config(path, &config, self.cipher.as_ref())?;
        }
        Ok(config.vault.encrypt_config)
    }

    pub fn set_encryption(&mut self, path: &Path, sealed: &str, passphrase: &str, enabled: bool) -> Result<(), VaultIssue> {
        self.writable().map_err(|_| VaultIssue::SaveFailed)?;
        let _lock = lock_config(path).map_err(|_| VaultIssue::SaveFailed)?;
        let mut config = self.load(path).map_err(|_| VaultIssue::ConfigReadFailed)?;
        if sealed.is_empty() || config.vault.sealed != sealed {
            return Err(VaultIssue::NotConfigured);
        }
        Vault::open(sealed, passphrase).map_err(vault_issue)?;
        if config.vault.encrypt_config != enabled {
            let cipher = enabled.then(|| ConfigCipher::new(passphrase));
            config.vault.encrypt_config = enabled;
            write_config(path, &config, cipher.as_ref()).map_err(|_| VaultIssue::SaveFailed)?;
            self.cipher = cipher;
        }
        Ok(())
    }
}

pub fn write_defaults(dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join("defaults.toml");
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "defaults.toml is a symbolic link"));
    }
    write_config(&path, &Config::default(), None)?;
    Ok(path)
}

fn write_config(path: &Path, config: &Config, cipher: Option<&ConfigCipher>) -> io::Result<()> {
    let text = config.to_text();
    if text.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "could not serialize configuration"));
    }
    let text = match cipher { Some(cipher) => cipher.encrypt(&text), None => text };
    let target = match path.canonicalize() {
        Ok(target) => target,
        Err(e) if e.kind() == io::ErrorKind::NotFound => path.to_owned(),
        Err(e) => return Err(e),
    };
    let permissions = fs::metadata(&target).ok().map(|m| m.permissions());
    if permissions.as_ref().is_some_and(fs::Permissions::readonly) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "configuration is read-only"));
    }
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let temp = target.with_extension(format!("toml.{}.{nonce}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    let result = (|| {
        if let Some(permissions) = &permissions {
            file.set_permissions(permissions.clone())?;
        }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        if permissions.is_some() {
            return aster_platform_windows::replace_file(&target, &temp);
        }
        fs::rename(&temp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
#[path = "../test/config_file.rs"]
mod tests;
