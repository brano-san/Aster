use russh_sftp::client::SftpSession;
use std::path::{Path, PathBuf};
use crate::sftp::{join, local_name, unique};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
}

pub trait Progress: Send {
    fn advance(&mut self, done: u64, total: u64);
}

impl<F: FnMut(u64, u64) + Send> Progress for F {
    fn advance(&mut self, done: u64, total: u64) { self(done, total); }
}

async fn remote_tree(s: &SftpSession, path: &str) -> Result<Vec<(String, russh_sftp::client::fs::Metadata)>, String> {
    let mut pending = vec![path.to_owned()];
    let mut entries = Vec::new();
    while let Some(path) = pending.pop() {
        let meta = s.symlink_metadata(&path).await.map_err(|e| format!("{path}: {e}"))?;
        if meta.is_dir() && !meta.is_symlink() {
            for entry in s.read_dir(&path).await.map_err(|e| format!("{path}: {e}"))? {
                let name = entry.file_name();
                if name == "." || name == ".." { continue; }
                if name.is_empty() || name.contains('/') || name.contains('\\') { return Err(format!("{path}: invalid entry name {name:?}")); }
                pending.push(join(&path, &name));
            }
        }
        entries.push((path, meta));
    }
    Ok(entries)
}

fn local_tree(path: &Path) -> Result<Vec<(PathBuf, std::fs::Metadata)>, String> {
    let mut pending = vec![path.to_owned()];
    let mut entries = Vec::new();
    while let Some(path) = pending.pop() {
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if meta.is_dir() && !meta.is_symlink() {
            for entry in std::fs::read_dir(&path).map_err(|e| format!("{}: {e}", path.display()))? {
                pending.push(entry.map_err(|e| format!("{}: {e}", path.display()))?.path());
            }
        }
        entries.push((path, meta));
    }
    Ok(entries)
}

pub async fn size(s: &SftpSession, path: &str) -> Result<Size, String> {
    let mut result = Size::default();
    for (_, meta) in remote_tree(s, path).await? {
        if meta.is_symlink() { continue; }
        if meta.is_dir() { result.dirs += 1; } else if meta.is_regular() { result.files += 1; result.bytes += meta.size.unwrap_or(0); }
    }
    Ok(result)
}

pub async fn local_size(path: &Path) -> Result<Size, String> {
    let mut result = Size::default();
    for (_, meta) in local_tree(path)? {
        if meta.is_symlink() { continue; }
        if meta.is_dir() { result.dirs += 1; } else if meta.is_file() { result.files += 1; result.bytes += meta.len(); }
    }
    Ok(result)
}

pub async fn remove(s: &SftpSession, path: &str) -> Result<Size, String> {
    let mut result = Size::default();
    for (path, meta) in remote_tree(s, path).await?.into_iter().rev() {
        if meta.is_dir() && !meta.is_symlink() {
            s.remove_dir(&path).await.map_err(|e| format!("{path}: {e}"))?;
            result.dirs += 1;
        } else {
            s.remove_file(&path).await.map_err(|e| format!("{path}: {e}"))?;
            result.files += 1;
            if !meta.is_symlink() { result.bytes += meta.size.unwrap_or(0); }
        }
    }
    Ok(result)
}

pub async fn chmod(s: &SftpSession, path: &str, mode: u32, recursive: bool) -> Result<u64, String> {
    let entries = if recursive { remote_tree(s, path).await? } else {
        vec![(path.to_owned(), s.symlink_metadata(path).await.map_err(|e| format!("{path}: {e}"))?)]
    };
    let mut count = 0;
    for (path, meta) in entries.into_iter().rev() {
        if meta.is_symlink() { continue; }
        let mut attrs = russh_sftp::protocol::FileAttributes::empty();
        attrs.permissions = Some((meta.permissions.unwrap_or(0) & !0o7777) | (mode & 0o7777));
        s.set_metadata(&path, attrs).await.map_err(|e| format!("{path}: {e}"))?;
        count += 1;
    }
    Ok(count)
}

async fn copy<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(from: &mut R, to: &mut W, progress: &mut dyn Progress, done: &mut u64, total: u64) -> std::io::Result<()> {
    let mut buf = vec![0; 64 * 1024];
    loop {
        let n = from.read(&mut buf).await?;
        if n == 0 { break; }
        to.write_all(&buf[..n]).await?;
        *done += n as u64;
        progress.advance(*done, total);
    }
    to.shutdown().await
}

pub async fn download(s: &SftpSession, remote: &str, into: &Path, progress: &mut dyn Progress) -> Result<PathBuf, String> {
    let total = size(s, remote).await?.bytes;
    let entries = remote_tree(s, remote).await?;
    std::fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
    let root = unique(into, &local_name(remote));
    let mut created = false;
    let result = async {
        let mut done = 0;
        for (path, meta) in entries {
            if meta.is_symlink() { continue; }
            let relative = if path == remote { "" } else if remote == "." || remote.is_empty() { &path } else {
                path.strip_prefix(remote.trim_end_matches('/')).ok_or_else(|| format!("{path}: outside requested tree"))?.trim_start_matches('/')
            };
            if relative.split('/').any(|p| p == ".." || p.contains('\\') || p.contains(':')) { return Err(format!("{path}: invalid name")); }
            let local = if relative.is_empty() { root.clone() } else { root.join(relative) };
            if meta.is_dir() {
                std::fs::create_dir(&local).map_err(|e| format!("{}: {e}", local.display()))?;
                created = true;
            } else if meta.is_regular() {
                let mut from = s.open(&path).await.map_err(|e| format!("{path}: {e}"))?;
                let mut to = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&local).await.map_err(|e| format!("{}: {e}", local.display()))?;
                created = true;
                copy(&mut from, &mut to, progress, &mut done, total).await.map_err(|e| format!("{path} -> {}: {e}", local.display()))?;
            }
        }
        Ok(())
    }.await;
    if let Err(error) = result {
        if created {
            let cleanup = if root.is_dir() { std::fs::remove_dir_all(&root) } else { std::fs::remove_file(&root) };
            if let Err(cleanup) = cleanup { return Err(format!("{error}; {}: cleanup failed: {cleanup}", root.display())); }
        }
        return Err(error);
    }
    Ok(root)
}

pub async fn upload(s: &SftpSession, local: &Path, into: &str, progress: &mut dyn Progress) -> Result<String, String> {
    let total = local_size(local).await?.bytes;
    let entries = local_tree(local)?;
    let name = local.file_name().ok_or_else(|| format!("{}: missing name", local.display()))?.to_string_lossy();
    let root = join(into, &name);
    if s.try_exists(&root).await.map_err(|e| format!("{root}: {e}"))? { return Err(format!("{root} already exists")); }
    let mut created = false;
    let result = async {
        let mut done = 0;
        for (path, meta) in entries {
            if meta.is_symlink() { continue; }
            let mut remote = root.clone();
            for part in path.strip_prefix(local).map_err(|e| format!("{}: {e}", path.display()))?.components() { remote = join(&remote, &part.as_os_str().to_string_lossy()); }
            if meta.is_dir() {
                s.create_dir(&remote).await.map_err(|e| format!("{remote}: {e}"))?;
                created = true;
            } else if meta.is_file() {
                let mut from = tokio::fs::File::open(&path).await.map_err(|e| format!("{}: {e}", path.display()))?;
                let mut to = s.create(&remote).await.map_err(|e| format!("{remote}: {e}"))?;
                created = true;
                copy(&mut from, &mut to, progress, &mut done, total).await.map_err(|e| format!("{} -> {remote}: {e}", path.display()))?;
            }
        }
        Ok(())
    }.await;
    if let Err(error) = result {
        if created {
            if let Err(cleanup) = remove(s, &root).await { return Err(format!("{error}; cleanup failed: {cleanup}")); }
        }
        return Err(error);
    }
    Ok(root)
}

#[cfg(test)]
#[path = "../test/sftp_tree.rs"]
mod tests;
