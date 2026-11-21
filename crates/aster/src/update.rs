use aster_core::update::{base64_decode, error_cancelled, error_missing, error_off, error_signature, is_local, local_path, newer, parse_manifest_since, percent, Release, UpdateEvent, UpdateRequest, CURRENT, MANIFEST_URL, PLATFORM};
use aster_platform::{Platform, PlatformError};
use crossbeam_channel::{unbounded, Receiver, Sender};
use ed25519_dalek::{Signature, VerifyingKey};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub const PUBLIC_KEY: &str = "ck84ndereTC9qrwgBubjDGa8ciOcAwTxMlPxe5Rt3rE=";
pub const BINARY: &str = if cfg!(windows) { "aster.exe" } else { "aster" };
pub const INSTALL_ELEVATED: &str = "--install-update";
const RESTARTED_FROM: &str = "ASTER_RESTARTED_FROM";
const LOCAL_MANIFEST: &str = "ASTER_UPDATE_MANIFEST";
const QUIET_RUNS: [&str; 2] = ["ASTER_NO_UPDATE", "ASTER_CAPTURE"];
const OLD_SLOTS: usize = 8;
const PROBE: &str = ".aster-update-probe";
const EXIT_WAIT: Duration = Duration::from_secs(30);
const CANCELLED: i64 = 1223;
const CHUNK: usize = 64 * 1024;

static RESTARTED: AtomicBool = AtomicBool::new(false);
static RELAUNCH: OnceLock<PathBuf> = OnceLock::new();

pub type Wake = Arc<dyn Fn() + Send + Sync>;

pub enum Reply {
    Ui(UpdateEvent),
    Restart,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    ReadOnly(String),
    Failed(String),
}

pub fn network_allowed() -> bool {
    QUIET_RUNS.iter().all(|v| std::env::var_os(v).is_none())
}

pub fn decide(manifest: &[u8], platform: &str, current: &str, local: bool) -> Result<Option<Release>, String> {
    let release = parse_manifest_since(&String::from_utf8_lossy(manifest), platform, current, local)?;
    Ok(newer(&release.version, current).then_some(release))
}

pub fn verify(bytes: &[u8], signature: &str, public_key: &str) -> Result<(), String> {
    let key = base64_decode(public_key).and_then(|k| <[u8; 32]>::try_from(k).ok()).and_then(|k| VerifyingKey::from_bytes(&k).ok()).ok_or_else(error_signature)?;
    let signature = base64_decode(signature).and_then(|s| Signature::from_slice(&s).ok()).ok_or_else(error_signature)?;
    key.verify_strict(bytes, &signature).map_err(|_| error_signature())
}

#[cfg(windows)]
pub fn binary(archive: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(io::Cursor::new(archive)).map_err(|e| e.to_string())?;
    let index = (0..zip.len()).find(|&i| zip.by_index(i).is_ok_and(|f| f.is_file() && f.name().rsplit(['/', '\\']).next() == Some(BINARY))).ok_or_else(|| error_missing(BINARY))?;
    let mut file = zip.by_index(index).map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(not(windows))]
pub fn binary(archive: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let top_level = path.components().filter(|c| matches!(c, std::path::Component::Normal(_))).count() <= 2;
        if top_level && path.file_name().is_some_and(|f| f == BINARY) && entry.header().entry_type().is_file() {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            return Ok(bytes);
        }
    }
    Err(error_missing(BINARY))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(suffix);
    path.with_file_name(name)
}

fn old_name(live: &Path, k: usize) -> PathBuf {
    with_suffix(live, &if k == 0 { ".old".to_owned() } else { format!(".old{k}") })
}

fn old_slot(live: &Path) -> PathBuf {
    for k in 0..OLD_SLOTS {
        let slot = old_name(live, k);
        let _ = fs::remove_file(&slot);
        if !slot.exists() {
            return slot;
        }
    }
    old_name(live, OLD_SLOTS)
}

fn old_files(exe: &Path) -> Vec<PathBuf> {
    (0..=OLD_SLOTS).map(|k| old_name(exe, k)).collect()
}

pub fn remove_old(exe: &Path) {
    for old in old_files(exe) {
        let _ = fs::remove_file(old);
    }
}

pub fn writable(dir: &Path) -> bool {
    let probe = dir.join(PROBE);
    let made = fs::OpenOptions::new().write(true).create_new(true).open(&probe).is_ok();
    let _ = fs::remove_file(&probe);
    made
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn replace(live: &Path, new: &Path) -> io::Result<Option<PathBuf>> {
    if !live.exists() {
        return fs::rename(new, live).map(|()| None);
    }
    let old = old_slot(live);
    fs::rename(live, &old)?;
    if let Err(e) = fs::rename(new, live) {
        let _ = fs::rename(&old, live);
        return Err(e);
    }
    Ok(Some(old))
}

pub fn swap(dir: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    let staged: Vec<(PathBuf, PathBuf)> = files.iter().map(|(name, _)| (dir.join(name), with_suffix(&dir.join(name), ".new"))).collect();
    let discard = || {
        for (_, new) in &staged {
            let _ = fs::remove_file(new);
        }
    };
    for ((_, new), (_, bytes)) in staged.iter().zip(files) {
        if let Err(e) = write_new(new, bytes) {
            discard();
            return Err(format!("{}: {e}", new.display()));
        }
    }
    let mut swapped: Vec<(&Path, Option<PathBuf>)> = Vec::new();
    for (live, new) in &staged {
        match replace(live, new) {
            Ok(old) => swapped.push((live.as_path(), old)),
            Err(e) => {
                for (live, old) in swapped.iter().rev() {
                    let _ = fs::remove_file(live);
                    if let Some(old) = old {
                        let _ = fs::rename(old, live);
                    }
                }
                discard();
                return Err(format!("{}: {e}", live.display()));
            }
        }
    }
    Ok(())
}

pub type Elevate<'a> = &'a mut dyn FnMut(&[u8], &str) -> Result<bool, String>;

fn unpack(archive: &[u8], exe: &Path) -> Result<(), String> {
    let dir = exe.parent().unwrap_or(Path::new("."));
    let name = exe.file_name().map_or_else(|| BINARY.to_owned(), |n| n.to_string_lossy().into_owned());
    swap(dir, &[(name, binary(archive)?)])
}

pub fn install(release: &Release, exe: &Path, public_key: &str, download: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>, installing: &mut dyn FnMut(), elevate: Option<Elevate>) -> Result<(), Failure> {
    let can_write = writable(exe.parent().unwrap_or(Path::new(".")));
    install_into(release, exe, public_key, download, installing, elevate, can_write)
}

pub fn install_into(release: &Release, exe: &Path, public_key: &str, download: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>, installing: &mut dyn FnMut(), elevate: Option<Elevate>, can_write: bool) -> Result<(), Failure> {
    let dir = exe.parent().unwrap_or(Path::new("."));
    let read_only = || Failure::ReadOnly(dir.display().to_string());
    let elevate = match (can_write, elevate) {
        (true, _) => None,
        (false, Some(e)) if dir.is_dir() => Some(e),
        (false, _) => return Err(read_only()),
    };
    let archive = download(&release.url).map_err(Failure::Failed)?;
    verify(&archive, &release.signature, public_key).map_err(Failure::Failed)?;
    installing();
    match elevate {
        None => unpack(&archive, exe).map_err(Failure::Failed),
        Some(e) => match e(&archive, &release.signature) {
            Ok(true) => Ok(()),
            Ok(false) => Err(read_only()),
            Err(e) => Err(Failure::Failed(e)),
        },
    }
}

pub fn install_elevated(archive: &Path, signature: &str, public_key: &str, exe: &Path, delete_at_reboot: &mut dyn FnMut(&Path)) -> Result<(), String> {
    let bytes = fs::read(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    verify(&bytes, signature, public_key)?;
    unpack(&bytes, exe)?;
    for old in old_files(exe) {
        if fs::remove_file(&old).is_err() && fs::symlink_metadata(&old).is_ok() {
            delete_at_reboot(&old);
        }
    }
    Ok(())
}

pub fn install_from_command_line<P: Platform>(archive: Option<&String>, signature: Option<&String>) -> i32 {
    let (Some(archive), Some(signature)) = (archive.map(PathBuf::from), signature) else { return 2 };
    let result = running_exe().and_then(|exe| install_elevated(&archive, signature, PUBLIC_KEY, &exe, &mut |p| P::delete_at_reboot(p)));
    match result {
        Ok(()) => 0,
        Err(e) => {
            let _ = fs::write(error_file(&archive), e);
            1
        }
    }
}

fn error_file(archive: &Path) -> PathBuf {
    with_suffix(archive, ".error")
}

fn elevated<P: Platform>(archive: &[u8], signature: &str) -> Result<bool, String> {
    let file = std::env::temp_dir().join(format!("aster-update-{}.zip", std::process::id()));
    fs::write(&file, archive).map_err(|e| format!("{}: {e}", file.display()))?;
    let _ = fs::remove_file(error_file(&file));
    let ran = P::run_elevated(&[INSTALL_ELEVATED.to_owned(), file.to_string_lossy().into_owned(), signature.to_owned()]);
    let reason = fs::read_to_string(error_file(&file)).ok();
    let _ = fs::remove_file(&file);
    let _ = fs::remove_file(error_file(&file));
    match ran {
        Ok(0) => Ok(true),
        Ok(code) => Err(reason.unwrap_or_else(|| format!("the installer stopped with code {code}"))),
        Err(e) if e.code == CANCELLED => Ok(false),
        Err(e) => Err(e.message),
    }
}

pub fn read_local(path: &Path, progress: &mut dyn FnMut(u64, Option<u64>) -> bool) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let total = file.metadata().ok().map(|m| m.len());
    let mut out = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..n]);
        if !progress(out.len() as u64, total) {
            return Err(error_cancelled());
        }
    }
}

fn fetch<P: Platform>(url: &str, progress: &mut dyn FnMut(u64, Option<u64>) -> bool) -> Result<Vec<u8>, String> {
    if let Some(path) = local_path(url) {
        return read_local(Path::new(path), progress);
    }
    let mut out = Vec::new();
    P::download(url, &mut out, progress).map_err(|e: PlatformError| e.message)?;
    Ok(out)
}

fn check<P: Platform>() -> Result<Option<Release>, String> {
    let (manifest, local) = match std::env::var_os(LOCAL_MANIFEST) {
        Some(path) => (fs::read(&path).map_err(|e| format!("{}: {e}", Path::new(&path).display()))?, true),
        None if !network_allowed() => return Err(error_off()),
        None => match fetch::<P>(MANIFEST_URL, &mut |_, _| true) {
            Err(e) if e.contains("404") => return Ok(None),
            fetched => (fetched?, false),
        },
    };
    decide(&manifest, PLATFORM, CURRENT, local)
}

pub fn running_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(fs::canonicalize(&exe).unwrap_or(exe))
}

fn run<P: Platform>(jobs: Receiver<UpdateRequest>, tx: Sender<Reply>, wake: Wake, cancel: Arc<AtomicBool>) {
    let send = |r: Reply| {
        let _ = tx.send(r);
        wake();
    };
    for job in jobs {
        let release = match job {
            UpdateRequest::Install(release) => release,
            UpdateRequest::Check { manual } => {
                send(Reply::Ui(UpdateEvent::Checked { manual, found: check::<P>() }));
                continue;
            }
            UpdateRequest::Cancel => continue,
        };
        cancel.store(false, Ordering::Release);
        let mut shown = None;
        let mut download = |url: &str| {
            if !is_local(url) && !network_allowed() {
                return Err(error_off());
            }
            fetch::<P>(url, &mut |received, total| {
                let step = percent(received, total).map_or(received >> 20, u64::from);
                if shown != Some(step) {
                    shown = Some(step);
                    send(Reply::Ui(UpdateEvent::Progress { received, total }));
                }
                !cancel.load(Ordering::Acquire)
            })
            .map_err(|e| if cancel.load(Ordering::Acquire) { error_cancelled() } else { e })
        };
        let result = match running_exe() {
            Ok(exe) => {
                let mut elevate = elevated::<P>;
                install(&release, &exe, PUBLIC_KEY, &mut download, &mut || send(Reply::Ui(UpdateEvent::Installing)), cfg!(windows).then_some(&mut elevate as Elevate)).map(|()| exe)
            }
            Err(e) => Err(Failure::Failed(e)),
        };
        match result {
            Ok(exe) => {
                tracing::info!("installed Aster {} into {}, restarting", release.version, exe.display());
                let _ = RELAUNCH.set(exe);
                send(Reply::Restart);
            }
            Err(Failure::ReadOnly(dir)) => send(Reply::Ui(UpdateEvent::ReadOnly(dir))),
            Err(Failure::Failed(e)) => {
                tracing::warn!("cannot install Aster {}: {e}", release.version);
                send(Reply::Ui(UpdateEvent::Failed(e)));
            }
        }
    }
}

pub struct UpdateWorker {
    jobs: Sender<UpdateRequest>,
    pub events: Receiver<Reply>,
    cancel: Arc<AtomicBool>,
}

impl UpdateWorker {
    pub fn start<P: Platform>(wake: Wake) -> io::Result<Self> {
        let (jobs, rx) = unbounded();
        let (tx, events) = unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::Builder::new().name("aster-update".into()).spawn(move || run::<P>(rx, tx, wake, flag))?;
        Ok(Self { jobs, events, cancel })
    }

    pub fn submit(&self, req: UpdateRequest) {
        if req == UpdateRequest::Cancel {
            self.cancel.store(true, Ordering::Release);
        }
        let _ = self.jobs.send(req);
    }
}

pub fn on_start<P: Platform>() {
    if let Some(pid) = std::env::var_os(RESTARTED_FROM) {
        std::env::remove_var(RESTARTED_FROM);
        if let Some(pid) = pid.to_str().and_then(|p| p.parse().ok()) {
            P::wait_exit(pid, EXIT_WAIT);
        }
        RESTARTED.store(true, Ordering::Release);
    }
    if let Ok(exe) = running_exe() {
        remove_old(&exe);
    }
}

pub fn restarted() -> bool {
    RESTARTED.load(Ordering::Acquire)
}

pub fn relaunch_if_installed() {
    let Some(exe) = RELAUNCH.get() else { return };
    if let Err(e) = std::process::Command::new(exe).env(RESTARTED_FROM, std::process::id().to_string()).spawn() {
        tracing::error!("cannot start the updated Aster {}: {e}", exe.display());
    }
}

#[cfg(test)]
#[path = "../test/update.rs"]
mod tests;
