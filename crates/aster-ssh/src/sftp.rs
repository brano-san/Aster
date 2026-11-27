use crossbeam_channel::Sender;
use russh_sftp::client::SftpSession;
use russh_sftp::client::{error::Error as SftpError, fs::File as SftpFile};
use russh_sftp::protocol::{OpenFlags, StatusCode};
use std::collections::HashMap;
use std::future::Future;
use std::hash::{DefaultHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;

const CHUNK: usize = 64 * 1024;
const PROGRESS_EVERY: Duration = Duration::from_millis(100);
const EDIT_POLL: Duration = if cfg!(test) { Duration::from_millis(60) } else { Duration::from_secs(1) };

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub link: bool,
    pub size: u64,
    pub modified: Option<u64>,
    pub mode: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    List { id: u64, paths: Vec<String> },
    OpenLink { id: u64, path: String },
    Download { id: u64, remote: String, dir: PathBuf },
    Upload { id: u64, local: PathBuf, dir: String },
    Mkdir { path: String },
    Remove { path: String, dir: bool },
    Rename { from: String, to: String },
    Edit { id: u64, remote: String, dir: PathBuf },
    Chmod { path: String, mode: u32, recursive: bool },
    Cancel { id: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Listing { id: u64, path: String, entries: Vec<Entry> },
    LinkFile { id: u64, path: String },
    NavigationFailed { id: u64, error: String },
    Progress { id: u64, done: u64, total: u64 },
    Downloaded { id: u64, local: PathBuf },
    Uploaded { id: u64, remote: String },
    Changed { dir: String },
    Editing { id: u64, local: PathBuf },
    Saved { id: u64, remote: String },
    Failed { id: Option<u64>, error: String },
}

impl Request {
    pub(crate) fn failure(&self, error: String) -> Reply {
        match self {
            Self::List { id, .. } | Self::OpenLink { id, .. } => Reply::NavigationFailed { id: *id, error },
            Self::Download { id, .. } | Self::Upload { id, .. } | Self::Edit { id, .. } | Self::Cancel { id } => Reply::Failed { id: Some(*id), error },
            _ => Reply::Failed { id: None, error },
        }
    }
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "." {
        name.to_owned()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

pub fn parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(i) => trimmed[..i].to_owned(),
        None if path.starts_with('/') => "/".to_owned(),
        None => ".".to_owned(),
    }
}

pub fn file_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

pub fn local_name(remote: &str) -> String {
    let name: String = file_name(remote).chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c }).collect();
    let name = name.trim_end_matches(['.', ' ']).to_owned();
    if name.is_empty() {
        "download".into()
    } else {
        name
    }
}

pub fn unique(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if first.symlink_metadata().is_err() {
        return first;
    }
    let (stem, ext) = match name.rfind('.').filter(|&i| i > 0) {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| p.symlink_metadata().is_err())
        .unwrap_or(first)
}

pub(super) async fn exclusive_upload(session: &SftpSession, path: &str) -> Result<SftpFile, String> {
    let flags = OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE;
    match session.open_with_flags(path, flags).await {
        Ok(file) => Ok(file),
        Err(error) => Err(upload_create_error(session, path, error).await),
    }
}

pub(super) async fn upload_create_error(session: &SftpSession, path: &str, error: SftpError) -> String {
    if session.symlink_metadata(path).await.is_ok() {
        format!("{path} already exists")
    } else {
        format!("{path}: {error}")
    }
}

pub(super) fn file_is_missing(error: &SftpError) -> bool {
    matches!(error, SftpError::Status(status) if status.status_code == StatusCode::NoSuchFile)
}

pub fn sort(entries: &mut [Entry]) {
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.name.cmp(&b.name)));
}

fn entry(name: String, meta: &russh_sftp::client::fs::Metadata) -> Entry {
    Entry {
        name,
        dir: meta.is_dir(),
        link: meta.is_symlink(),
        size: meta.size.unwrap_or(0),
        modified: meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()),
        mode: meta.permissions,
    }
}

#[derive(PartialEq, Eq)]
struct EditStamp {
    modified: Option<SystemTime>,
    size: u64,
    hash: u64,
}

async fn edit_stamp(path: &Path) -> Result<Option<EditStamp>, String> {
    let error = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e)),
    };
    let meta = file.metadata().await.map_err(error)?;
    let mut hash = DefaultHasher::new();
    let mut buf = vec![0; CHUNK];
    loop {
        let n = file.read(&mut buf).await.map_err(error)?;
        if n == 0 {
            break;
        }
        hash.write(&buf[..n]);
    }
    Ok(Some(EditStamp { modified: meta.modified().ok(), size: meta.len(), hash: hash.finish() }))
}

#[derive(Clone)]
pub struct Files {
    session: Arc<SftpSession>,
    replies: Sender<Reply>,
    running: Arc<Mutex<HashMap<u64, Arc<Notify>>>>,
}

impl Files {
    pub fn new(session: SftpSession, replies: Sender<Reply>) -> Self {
        Self { session: Arc::new(session), replies, running: Arc::default() }
    }

    pub fn handle(&self, request: Request) {
        let mut running = self.running.lock().unwrap_or_else(|error| error.into_inner());
        if let Request::Cancel { id } = request {
            if let Some(cancel) = running.get(&id) { cancel.notify_one(); }
            return;
        }
        let id = match &request {
            Request::Download { id, .. } | Request::Upload { id, .. } | Request::Edit { id, .. } => Some(*id),
            _ => None,
        };
        let cancel = Arc::new(Notify::new());
        if let Some(id) = id { running.insert(id, cancel.clone()); }
        drop(running);
        let me = self.clone();
        tokio::spawn(async move {
            let reply = me.run(&request, &cancel).await.unwrap_or_else(|error| request.failure(error));
            if let Some(id) = id {
                if let Ok(mut r) = me.running.lock() {
                    r.remove(&id);
                }
            }
            let _ = me.replies.send(reply);
        });
    }

    pub(super) fn cancel_all(&self) {
        let running = self.running.lock().unwrap_or_else(|error| error.into_inner());
        for cancel in running.values() { cancel.notify_one(); }
    }

    async fn listing(&self, id: u64, path: &str) -> Result<Reply, String> {
        let wanted = if path.is_empty() || path == "~" { "." } else { path };
        let path = self.session.canonicalize(wanted).await.map_err(|e| e.to_string())?;
        let mut entries: Vec<Entry> = self.session.read_dir(path.clone()).await.map_err(|e| e.to_string())?
            .filter(|e| !matches!(e.file_name().as_str(), "." | ".."))
            .map(|e| entry(e.file_name(), &e.metadata())).collect();
        sort(&mut entries);
        Ok(Reply::Listing { id, path, entries })
    }

    async fn run(&self, request: &Request, cancel: &Notify) -> Result<Reply, String> {
        let s = &self.session;
        let err = |e: russh_sftp::client::error::Error| e.to_string();
        match request {
            Request::List { id, paths } => {
                let mut error = String::new();
                for path in paths {
                    match self.listing(*id, path).await {
                        Ok(reply) => return Ok(reply),
                        Err(e) => error = e,
                    }
                }
                Err(error)
            }
            Request::OpenLink { id, path } => {
                let meta = s.metadata(path).await.map_err(err)?;
                if meta.is_dir() {
                    self.listing(*id, path).await
                } else if meta.is_regular() {
                    Ok(Reply::LinkFile { id: *id, path: path.clone() })
                } else {
                    Err(format!("{path} is not a regular file"))
                }
            }
            Request::Mkdir { path } => {
                s.create_dir(path.clone()).await.map_err(err)?;
                Ok(Reply::Changed { dir: parent(path) })
            }
            Request::Remove { path, dir } => {
                if *dir {
                    crate::sftp_tree::remove(s, path).await?;
                } else {
                    s.remove_file(path.clone()).await.map_err(err)?;
                }
                Ok(Reply::Changed { dir: parent(path) })
            }
            Request::Rename { from, to } => {
                s.rename(from, to).await.map_err(err)?;
                Ok(Reply::Changed { dir: parent(from) })
            }
            Request::Chmod { path, mode, recursive } => {
                crate::sftp_tree::chmod(s, path, *mode, *recursive).await?;
                Ok(Reply::Changed { dir: parent(path) })
            }
            Request::Download { id, remote, dir } => {
                let meta = until_cancelled(cancel, async { s.symlink_metadata(remote).await.map_err(err) }).await?;
                if meta.is_dir() && !meta.is_symlink() {
                    let mut progress = self.progress(*id);
                    let local = crate::sftp_tree::download_with_cancel(s, remote, dir, &mut progress, Some(cancel)).await?;
                    return Ok(Reply::Downloaded { id: *id, local });
                }
                self.download(*id, remote, dir, cancel).await
            }
            Request::Upload { id, local, dir } => self.upload(*id, local, dir, cancel).await,
            Request::Edit { id, remote, dir } => self.edit(*id, remote, dir, cancel).await,
            Request::Cancel { .. } => Err("nothing to cancel".into()),
        }
    }

    fn progress(&self, id: u64) -> impl FnMut(u64, u64) + Send {
        let replies = self.replies.clone();
        let mut last = Instant::now() - PROGRESS_EVERY;
        move |done, total| {
            if last.elapsed() >= PROGRESS_EVERY || done >= total {
                last = Instant::now();
                let _ = replies.send(Reply::Progress { id, done, total });
            }
        }
    }

    async fn edit(&self, id: u64, remote: &str, dir: &Path, cancel: &Notify) -> Result<Reply, String> {
        let Reply::Downloaded { local, .. } = self.download(id, remote, &dir.join(id.to_string()), cancel).await? else {
            return Err("download failed".into());
        };
        let prepared = until_cancelled(cancel, async {
            edit_stamp(&local).await?.ok_or_else(|| format!("{}: file disappeared", local.display()))
        }).await;
        let mut saved = match prepared {
            Ok(stamp) => stamp,
            Err(error) => {
                let _ = std::fs::remove_file(&local);
                if let Some(parent) = local.parent() { let _ = std::fs::remove_dir(parent); }
                return Err(error);
            }
        };
        let _ = self.replies.send(Reply::Editing { id, local: local.clone() });
        loop {
            until_cancelled(cancel, async { tokio::time::sleep(EDIT_POLL).await; Ok(()) }).await?;
            let Some(now) = edit_stamp(&local).await? else { continue };
            if now == saved {
                continue;
            }
            until_cancelled(cancel, async { tokio::time::sleep(EDIT_POLL).await; Ok(()) }).await?;
            if edit_stamp(&local).await?.as_ref() != Some(&now) {
                continue;
            }
            self.put(&local, remote).await?;
            saved = now;
            let _ = self.replies.send(Reply::Saved { id, remote: remote.to_owned() });
        }
    }

    async fn put(&self, local: &Path, remote: &str) -> Result<(), String> {
        let mut source = tokio::fs::File::open(local).await.map_err(|e| format!("{}: {e}", local.display()))?;
        let mut target = self.session.create(remote.to_owned()).await.map_err(|e| format!("{remote}: {e}"))?;
        tokio::io::copy(&mut source, &mut target).await.map_err(|e| format!("{remote}: {e}"))?;
        target.shutdown().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn download(&self, id: u64, remote: &str, dir: &Path, cancel: &Notify) -> Result<Reply, String> {
        let meta = until_cancelled(cancel, async { self.session.metadata(remote).await.map_err(|e| e.to_string()) }).await?;
        if !meta.is_regular() { return Err(format!("{remote} is not a regular file")); }
        let total = meta.size.unwrap_or(0);
        let mut source = until_cancelled(cancel, async { self.session.open(remote).await.map_err(|e| e.to_string()) }).await?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = local_name(remote);
        let (local, mut target) = loop {
            let local = unique(dir, &name);
            match tokio::fs::File::create_new(&local).await {
                Ok(file) => break (local, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("{}: {error}", local.display())),
            }
        };
        if let Err(e) = until_cancelled(cancel, self.copy(id, &mut source, &mut target, total)).await {
            let _ = target.shutdown().await;
            drop(target);
            let _ = std::fs::remove_file(&local);
            return Err(e);
        }
        Ok(Reply::Downloaded { id, local })
    }

    async fn upload(&self, id: u64, local: &Path, dir: &str, cancel: &Notify) -> Result<Reply, String> {
        let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("not a file")?;
        let meta = std::fs::metadata(local).map_err(|e| format!("{}: {e}", local.display()))?;
        if meta.is_dir() {
            let mut progress = self.progress(id);
            let remote = crate::sftp_tree::upload_with_cancel(&self.session, local, dir, &mut progress, Some(cancel)).await?;
            return Ok(Reply::Uploaded { id, remote });
        }
        if !meta.is_file() {
            return Err(format!("{name} is not a regular file"));
        }
        let remote = join(dir, &name);
        let mut source = tokio::fs::File::open(local).await.map_err(|e| format!("{}: {e}", local.display()))?;
        let mut target = exclusive_upload(&self.session, &remote).await?;
        let result = until_cancelled(cancel, async {
            self.copy(id, &mut source, &mut target, meta.len()).await?;
            target.shutdown().await.map_err(|e| e.to_string())
        }).await;
        if let Err(e) = result {
            let _ = target.shutdown().await;
            drop(target);
            if let Err(cleanup) = self.session.remove_file(&remote).await {
                if !file_is_missing(&cleanup) { return Err(format!("{e}; {remote}: cleanup failed: {cleanup}")); }
            }
            return Err(e);
        }
        Ok(Reply::Uploaded { id, remote })
    }

    async fn copy<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(&self, id: u64, from: &mut R, to: &mut W, total: u64) -> Result<(), String> {
        let mut buf = vec![0u8; CHUNK];
        let mut done = 0u64;
        let mut last = Instant::now() - PROGRESS_EVERY;
        loop {
            let n = from.read(&mut buf).await.map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            to.write_all(&buf[..n]).await.map_err(|e| e.to_string())?;
            done += n as u64;
            if last.elapsed() >= PROGRESS_EVERY {
                last = Instant::now();
                let _ = self.replies.send(Reply::Progress { id, done, total: total.max(done) });
            }
        }
        to.flush().await.map_err(|e| e.to_string())?;
        let _ = self.replies.send(Reply::Progress { id, done, total: total.max(done) });
        Ok(())
    }
}

pub(super) async fn until_cancelled<T>(cancel: &Notify, transfer: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    tokio::select! {
        biased;
        _ = cancel.notified() => Err("cancelled".into()),
        reply = transfer => reply,
    }
}

#[cfg(test)]
#[path = "../test/sftp.rs"]
mod tests;
