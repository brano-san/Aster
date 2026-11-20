use aster_core::config::Config;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub fn read(path: &Path) -> io::Result<Config> {
    Config::parse(&fs::read_to_string(path)?).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid configuration"))
}

fn write(path: &Path, config: &Config) -> io::Result<()> {
    let text = config.to_text();
    if text.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "could not serialize configuration"));
    }
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

pub fn save_settings(path: &Path, config: &mut Config, sealed: &str) -> io::Result<()> {
    config.vault.sealed = match read(path) {
        Ok(disk) => disk.vault.sealed,
        Err(e) if e.kind() == io::ErrorKind::NotFound => sealed.to_owned(),
        Err(e) => return Err(e),
    };
    write(path, config)
}

pub fn save_vault(path: &Path, expected: &str, sealed: &str) -> io::Result<()> {
    let mut config = read(path)?;
    if config.vault.sealed != expected {
        return Err(io::Error::other("vault changed on disk"));
    }
    config.vault.sealed = sealed.to_owned();
    write(path, &config)
}

#[cfg(test)]
#[path = "../test/config_file.rs"]
mod tests;
