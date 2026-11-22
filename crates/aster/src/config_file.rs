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
    sync_written: bool,
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
                    write_config(path, &config, None, None)?;
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
    fn plain_text<'a>(&self, text: &'a str) -> io::Result<std::borrow::Cow<'a, str>> {
        if is_encrypted(text) {
            let cipher = self.cipher.as_ref().ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "config is encrypted"))?;
            cipher.decrypt(text).map(std::borrow::Cow::Owned).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "could not decrypt config"))
        } else if self.cipher.is_some() {
            Err(io::Error::new(io::ErrorKind::InvalidData, "config encryption changed on disk"))
        } else {
            Ok(std::borrow::Cow::Borrowed(text))
        }
    }

    fn parse_document(&self, text: &str) -> Result<Config, String> {
        let mut config = Config::parse(text)?;
        config.vault.encrypt_config = self.cipher.is_some();
        Ok(config)
    }

    pub fn load_document(&self, path: &Path) -> io::Result<(String, Result<Config, String>)> {
        let disk = fs::read_to_string(path)?;
        let text = self.plain_text(&disk)?.into_owned();
        let config = self.parse_document(&text);
        Ok((text, config))
    }

    pub fn load(&self, path: &Path) -> io::Result<Config> {
        self.load_document(path)?.1.map_err(invalid_data)
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

    pub fn save_settings(&self, path: &Path, config: &mut Config, sealed: &str, base: Option<&Config>) -> io::Result<()> {
        self.writable()?;
        let _lock = lock_config(path)?;
        let existing = source(path)?;
        config.vault.sealed = match &existing {
            Some(disk) => {
                let disk = self.read_writable(disk)?;
                if let Some(base) = base.filter(|base| self.sync_written && *base != &disk) {
                    *config = aster_core::sync::rebase_settings(base, config, &disk).map_err(invalid_data)?;
                }
                disk.vault.sealed
            }
            None if self.cipher.is_none() => sealed.to_owned(),
            None => return Err(io::Error::new(io::ErrorKind::NotFound, "encrypted configuration is missing")),
        };
        config.vault.encrypt_config = self.cipher.is_some();
        write_config(path, config, self.cipher.as_ref(), existing.as_deref())
    }

    pub fn save_vault(&mut self, path: &Path, expected: &str, sealed: &str, rekey: Option<&str>) -> io::Result<bool> {
        self.writable()?;
        let _lock = lock_config(path)?;
        let disk = fs::read_to_string(path)?;
        let text = self.plain_text(&disk)?;
        let mut config = self.parse_document(&text).map_err(invalid_data)?;
        ensure_writable(&config)?;
        if config.vault.sealed != expected {
            return Err(io::Error::other("vault changed on disk"));
        }
        if self.cipher.is_some() && sealed.is_empty() {
            config.vault.sealed.clear();
            config.vault.encrypt_config = false;
            write_config(path, &config, None, Some(&disk))?;
            self.cipher = None;
        } else {
            let updated = Config::replace_vault_seal(&text, sealed)
                .ok_or_else(|| invalid_data("could not update the vault configuration".into()))?;
            let next = rekey.filter(|_| self.cipher.is_some()).map(ConfigCipher::new);
            write_document(path, &updated, next.as_ref().or(self.cipher.as_ref()), Some(Some(&disk)))?;
            if next.is_some() {
                self.cipher = next;
            }
        }
        Ok(self.cipher.is_some())
    }

    pub fn set_encryption(&mut self, path: &Path, sealed: &str, passphrase: &str, enabled: bool) -> Result<(), VaultIssue> {
        self.writable().map_err(|_| VaultIssue::SaveFailed)?;
        let _lock = lock_config(path).map_err(|_| VaultIssue::SaveFailed)?;
        let disk = fs::read_to_string(path).map_err(|_| VaultIssue::ConfigReadFailed)?;
        let mut config = self.read_writable(&disk).map_err(|_| VaultIssue::ConfigReadFailed)?;
        if sealed.is_empty() || config.vault.sealed != sealed {
            return Err(VaultIssue::NotConfigured);
        }
        Vault::open(sealed, passphrase).map_err(vault_issue)?;
        if config.vault.encrypt_config != enabled {
            let cipher = enabled.then(|| ConfigCipher::new(passphrase));
            config.vault.encrypt_config = enabled;
            write_config(path, &config, cipher.as_ref(), Some(&disk)).map_err(|_| VaultIssue::SaveFailed)?;
            self.cipher = cipher;
        }
        Ok(())
    }
}

fn invalid_data(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn ensure_writable(config: &Config) -> io::Result<()> {
    if config.read_only() {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, config.read_only_message()))
    } else {
        Ok(())
    }
}

impl ConfigAccess {
    fn read_writable(&self, disk: &str) -> io::Result<Config> {
        let config = self.parse_document(&self.plain_text(disk)?).map_err(invalid_data)?;
        ensure_writable(&config)?;
        Ok(config)
    }

    pub fn load_sync(&self, path: &Path) -> io::Result<(String, Config, std::time::SystemTime)> {
        self.writable()?;
        let _lock = lock_config(path)?;
        let (text, config) = self.load_document(path)?;
        let config = config.map_err(invalid_data)?;
        ensure_writable(&config)?;
        Ok((text, config, fs::metadata(path)?.modified()?))
    }

    pub fn save_sync(&mut self, path: &Path, expected: &str, text: &str) -> io::Result<()> {
        self.writable()?;
        ensure_writable(&self.parse_document(text).map_err(invalid_data)?)?;
        let _lock = lock_config(path)?;
        let disk = fs::read_to_string(path)?;
        let current = self.plain_text(&disk)?;
        expect_source(Some(&current), Some(expected))?;
        ensure_writable(&self.parse_document(&current).map_err(invalid_data)?)?;
        if current != text {
            write_document(path, text, self.cipher.as_ref(), Some(Some(&disk)))?;
            self.sync_written = true;
        }
        Ok(())
    }

    pub fn save_raw(&self, path: &Path, expected: Option<&str>, text: &str) -> io::Result<Config> {
        self.writable()?;
        let config = self.parse_document(text).map_err(invalid_data)?;
        ensure_writable(&config)?;
        let _lock = lock_config(path)?;
        let disk = source(path)?;
        if disk.is_none() && self.cipher.is_some() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "encrypted configuration is missing"));
        }
        let existing = disk.as_deref().map(|disk| self.plain_text(disk)).transpose()?;
        if let Some(old) = existing.as_deref().and_then(|source| self.parse_document(source).ok()) {
            ensure_writable(&old)?;
            if self.cipher.is_some() && old.vault.sealed != config.vault.sealed {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, VaultIssue::SaveFailed.message()));
            }
        }
        expect_source(existing.as_deref(), expected)?;
        write_document(path, text, self.cipher.as_ref(), Some(disk.as_deref()))?;
        Ok(config)
    }
}

fn source(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn expect_source(existing: Option<&str>, expected: Option<&str>) -> io::Result<()> {
    if existing != expected {
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "configuration changed on disk"))
    } else {
        Ok(())
    }
}

pub fn write_defaults(dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join("defaults.toml");
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "defaults.toml is a symbolic link"));
    }
    write_text(&path, &Config::defaults_text(), None)?;
    Ok(path)
}

fn write_config(path: &Path, config: &Config, cipher: Option<&ConfigCipher>, expected: Option<&str>) -> io::Result<()> {
    ensure_writable(config)?;
    let text = config.to_text();
    if text.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "could not serialize configuration"));
    }
    write_document(path, &text, cipher, Some(expected))
}

fn write_document(path: &Path, text: &str, cipher: Option<&ConfigCipher>, expected: Option<Option<&str>>) -> io::Result<()> {
    match cipher {
        Some(cipher) => write_text(path, &cipher.encrypt(text), expected),
        None => write_text(path, text, expected),
    }
}

fn write_text(path: &Path, text: &str, expected: Option<Option<&str>>) -> io::Result<()> {
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
        if let Some(expected) = expected {
            expect_source(source(&target)?.as_deref(), expected)?;
        }
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
