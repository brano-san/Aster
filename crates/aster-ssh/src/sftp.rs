use crossbeam_channel::Sender;
use russh_sftp::client::SftpSession;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
    List { path: String },
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
    Listing { path: String, entries: Vec<Entry> },
    Progress { id: u64, done: u64, total: u64 },
    Downloaded { id: u64, local: PathBuf },
    Uploaded { id: u64, remote: String },
    Changed { dir: String },
    Editing { id: u64, local: PathBuf },
    Saved { id: u64, remote: String },
    Failed { id: Option<u64>, error: String },
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
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.').filter(|&i| i > 0) {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
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
    running: Arc<Mutex<HashMap<u64, tokio::task::JoinHandle<()>>>>,
}

impl Files {
    pub fn new(session: SftpSession, replies: Sender<Reply>) -> Self {
        Self { session: Arc::new(session), replies, running: Arc::default() }
    }

    pub fn handle(&self, request: Request) {
        if let Request::Cancel { id } = request {
            if let Some(task) = self.running.lock().ok().and_then(|mut r| r.remove(&id)) {
                task.abort();
                let replies = self.replies.clone();
                tokio::spawn(async move {
                    if let Err(error) = task.await {
                        let error = if error.is_cancelled() { "cancelled".into() } else { error.to_string() };
                        let _ = replies.send(Reply::Failed { id: Some(id), error });
                    }
                });
            }
            return;
        }
        let id = match &request {
            Request::Download { id, .. } | Request::Upload { id, .. } | Request::Edit { id, .. } => Some(*id),
            _ => None,
        };
        let me = self.clone();
        let task = tokio::spawn(async move {
            let reply = me.run(request).await.unwrap_or_else(|error| Reply::Failed { id, error });
            if let Some(id) = id {
                if let Ok(mut r) = me.running.lock() {
                    r.remove(&id);
                }
            }
            let _ = me.replies.send(reply);
        });
        if let (Some(id), Ok(mut r)) = (id, self.running.lock()) {
            r.insert(id, task);
        }
    }

    async fn run(&self, request: Request) -> Result<Reply, String> {
        let s = &self.session;
        let err = |e: russh_sftp::client::error::Error| e.to_string();
        match request {
            Request::List { path } => {
                let wanted = if path.trim().is_empty() || path == "~" { ".".to_owned() } else { path };
                let path = s.canonicalize(wanted).await.map_err(err)?;
                let mut entries: Vec<Entry> = s.read_dir(path.clone()).await.map_err(err)?.filter(|e| e.file_name() != "." && e.file_name() != "..").map(|e| entry(e.file_name(), &e.metadata())).collect();
                sort(&mut entries);
                Ok(Reply::Listing { path, entries })
            }
            Request::Mkdir { path } => {
                s.create_dir(path.clone()).await.map_err(err)?;
                Ok(Reply::Changed { dir: parent(&path) })
            }
            Request::Remove { path, dir } => {
                if dir {
                    crate::sftp_tree::remove(s, &path).await?;
                } else {
                    s.remove_file(path.clone()).await.map_err(err)?;
                }
                Ok(Reply::Changed { dir: parent(&path) })
            }
            Request::Rename { from, to } => {
                s.rename(from.clone(), to).await.map_err(err)?;
                Ok(Reply::Changed { dir: parent(&from) })
            }
            Request::Chmod { path, mode, recursive } => {
                crate::sftp_tree::chmod(s, &path, mode, recursive).await?;
                Ok(Reply::Changed { dir: parent(&path) })
            }
            Request::Download { id, remote, dir } => {
                if s.metadata(remote.clone()).await.map_err(err)?.is_dir() {
                    let mut progress = self.progress(id);
                    let local = crate::sftp_tree::download(s, &remote, &dir, &mut progress).await?;
                    return Ok(Reply::Downloaded { id, local });
                }
                self.download(id, &remote, &dir).await
            }
            Request::Upload { id, local, dir } => self.upload(id, &local, &dir).await,
            Request::Edit { id, remote, dir } => self.edit(id, &remote, &dir).await,
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

    async fn edit(&self, id: u64, remote: &str, dir: &Path) -> Result<Reply, String> {
        let Reply::Downloaded { local, .. } = self.download(id, remote, &dir.join(id.to_string())).await? else {
            return Err("download failed".into());
        };
        let mut saved = edit_stamp(&local).await?.ok_or_else(|| format!("{}: file disappeared", local.display()))?;
        let _ = self.replies.send(Reply::Editing { id, local: local.clone() });
        loop {
            tokio::time::sleep(EDIT_POLL).await;
            let Some(now) = edit_stamp(&local).await? else { continue };
            if now == saved {
                continue;
            }
            tokio::time::sleep(EDIT_POLL).await;
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

    async fn download(&self, id: u64, remote: &str, dir: &Path) -> Result<Reply, String> {
        let total = self.session.metadata(remote).await.map_err(|e| e.to_string())?.size.unwrap_or(0);
        let mut source = self.session.open(remote).await.map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let local = unique(dir, &local_name(remote));
        let mut target = tokio::fs::File::create(&local).await.map_err(|e| format!("{}: {e}", local.display()))?;
        let copied = self.copy(id, &mut source, &mut target, total).await;
        drop(target);
        if let Err(e) = copied {
            let _ = std::fs::remove_file(&local);
            return Err(e);
        }
        Ok(Reply::Downloaded { id, local })
    }

    async fn upload(&self, id: u64, local: &Path, dir: &str) -> Result<Reply, String> {
        let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("not a file")?;
        let meta = std::fs::metadata(local).map_err(|e| format!("{}: {e}", local.display()))?;
        if meta.is_dir() {
            let mut progress = self.progress(id);
            let remote = crate::sftp_tree::upload(&self.session, local, dir, &mut progress).await?;
            return Ok(Reply::Uploaded { id, remote });
        }
        if !meta.is_file() {
            return Err(format!("{name} is not a regular file"));
        }
        let remote = join(dir, &name);
        if self.session.try_exists(remote.clone()).await.unwrap_or(false) {
            return Err(format!("{remote} already exists"));
        }
        let mut source = tokio::fs::File::open(local).await.map_err(|e| format!("{}: {e}", local.display()))?;
        let mut target = self.session.create(remote.clone()).await.map_err(|e| e.to_string())?;
        if let Err(e) = self.copy(id, &mut source, &mut target, meta.len()).await {
            drop(target);
            let _ = self.session.remove_file(remote).await;
            return Err(e);
        }
        target.shutdown().await.map_err(|e| e.to_string())?;
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

#[cfg(test)]
#[path = "../test/sftp.rs"]
mod tests;
