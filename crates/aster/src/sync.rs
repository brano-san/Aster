use aster_core::sync::{self, Backend, Direction, Entry, Issue, Operation, Preferences, Report};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const STORE_DIR: &str = "aster-sync";

#[derive(Clone)]
pub struct Worker(crossbeam_channel::Sender<(Preferences, Operation)>);

pub enum Event {
    Machine(String),
    Tick,
    Finished(Result<Report, Issue>),
}

impl Worker {
    pub fn start(path: Option<PathBuf>, access: Arc<Mutex<crate::config_file::ConfigAccess>>, post: impl Fn(Event) + Send + 'static) -> io::Result<Self> {
        let (tx, rx) = crossbeam_channel::unbounded();
        std::thread::Builder::new().name("aster-sync".into()).spawn(move || {
            let machine = machine_name();
            post(Event::Machine(machine.clone()));
            let interval = Duration::from_secs(60);
            let mut next_tick = Instant::now() + interval;
            loop {
                if Instant::now() >= next_tick {
                    post(Event::Tick);
                    next_tick = Instant::now() + interval;
                }
                match rx.recv_deadline(next_tick) {
                    Ok((preferences, operation)) => post(Event::Finished(
                        path.as_deref().ok_or(Issue::Unavailable).and_then(|path| perform(path, &access, &machine, &preferences, operation))
                    )),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                }
            }
        })?;
        Ok(Self(tx))
    }

    pub fn request(&self, preferences: Preferences, operation: Operation) -> bool {
        self.0.send((preferences, operation)).is_ok()
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Baseline {
    target: u64,
    local: u64,
    remote: Option<u64>,
}

fn fingerprint(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn conflict_copy(store: &Store, name: &str, snapshot: &str, machine: &str, now: u64) -> Result<String, Issue> {
    let mut end = name.len().min(180);
    while !name.is_char_boundary(end) { end -= 1; }
    let base = sync::conflict_name(&name[..end], now);
    for number in 1..=999 {
        let name = if number == 1 { base.clone() } else { format!("{base} ({number})") };
        match store.write(&name, snapshot, true, machine) {
            Ok(()) => return Ok(name),
            Err(Issue::Exists) => {
                if store.read(&name)?.is_some_and(|existing| sync::same_snapshot(&existing, snapshot)) { return Ok(name); }
            }
            Err(error) => return Err(error),
        }
    }
    Err(Issue::Exists)
}

fn perform(path: &Path, access: &Mutex<crate::config_file::ConfigAccess>, machine: &str, preferences: &Preferences, operation: Operation) -> Result<Report, Issue> {
    let load = || {
        let (text, config, modified) = access.lock().expect("config access").load_sync(path)?;
        if !preferences.same_target(&config.sync) || operation == Operation::Automatic && !config.sync.automatic {
            return Err(Issue::Changed);
        }
        Ok((text, config.sync.last_synced, modified))
    };
    let initial = if operation == Operation::List { None } else { Some(load()?) };
    let dir = path.parent().ok_or(Issue::Unavailable)?;
    let store = match preferences.backend {
        Backend::Folder => Store::folder(Path::new(preferences.folder.trim()))?,
        Backend::Git => Store::git(dir, preferences.repository.trim())?,
        Backend::Off => return Err(Issue::Unavailable),
    };
    let mut report = Report { git_conflicts: store.recovered.clone(), ..Default::default() };
    if operation == Operation::List {
        report.entries = store.list()?;
        return Ok(report);
    }
    let (local, last_synced, modified) = if preferences.backend == Backend::Git { load()? } else { initial.ok_or(Issue::Unavailable)? };
    let primary = preferences.name(machine);
    let now = seconds(SystemTime::now());
    if let Operation::Delete(name) = operation {
        store.delete(&name, machine)?;
        report.entries = store.list()?;
        return Ok(report);
    }
    let parts = &preferences.parts;
    let snapshot = sync::extract_at(&local, parts, machine, now).map_err(Issue::Snapshot)?;
    let state_path = dir.join("sync-state.json");
    let target = fingerprint((preferences.backend, preferences.folder.trim(), preferences.repository.trim(), primary, parts));
    let local_fingerprint = |text: &str| sync::extract_at(text, parts, "", 0).map(fingerprint).map_err(Issue::Snapshot);
    let mut merge_into = None;
    let mut current_remote = None;
    match operation {
        Operation::Automatic => {
            let remote = store.read(primary)?;
            let remote_fingerprint = remote.as_deref().map(fingerprint);
            let candidate = remote.as_deref().map(|remote| sync::merge(&local, remote, parts).map_err(Issue::Snapshot)).transpose()?;
            let current = local_fingerprint(&local)?;
            let differs = candidate.as_deref().map(local_fingerprint).transpose()?.is_none_or(|merged| merged != current);
            let baseline = read(&state_path)?.and_then(|text| serde_json::from_str::<Baseline>(&text).ok()).filter(|base| base.target == target);
            let identical = remote.as_deref().is_some_and(|remote| sync::same_snapshot(remote, &snapshot));
            let direction = if identical {
                Direction::Nothing
            } else if let Some(base) = &baseline {
                let local = u64::from(current != base.local);
                let remote = u64::from(differs && remote.is_some() && remote_fingerprint != base.remote);
                sync::decide(local, remote, 0)
            } else if !differs {
                Direction::Nothing
            } else if last_synced == 0 {
                sync::decide(u64::from(remote.is_none() || current != local_fingerprint("")?), u64::from(remote.is_some()), 0)
            } else {
                let saved = remote.as_deref().map(sync::metadata).transpose().map_err(Issue::Snapshot)?.map_or(0, |meta| meta.saved);
                sync::decide(seconds(modified), saved, last_synced)
            };
            current_remote = remote_fingerprint;
            match direction {
                Direction::Nothing if baseline.is_some_and(|base| base.local == current && base.remote == current_remote) || remote.is_none() => {}
                Direction::Nothing => report.saved = Some(last_synced.max(now)),
                Direction::Upload => {
                    store.write_snapshot(primary, &snapshot, false, machine, Some(remote.as_deref()))?;
                    current_remote = Some(fingerprint(&snapshot));
                    report.saved = Some(now);
                }
                Direction::Conflict | Direction::Download => {
                    if direction == Direction::Conflict {
                        report.conflict = Some(conflict_copy(&store, primary, &snapshot, machine, now)?);
                    }
                    merge_into = candidate;
                    report.saved = Some(now);
                }
            }
        }
        Operation::Upload { name, new } => {
            store.write(&name, &snapshot, new, machine)?;
            if name == primary {
                current_remote = Some(fingerprint(&snapshot));
            }
            report.saved = Some(now);
        }
        Operation::Download(name) => {
            let remote = store.read(&name)?.ok_or(Issue::Missing)?;
            merge_into = Some(sync::merge(&local, &remote, parts).map_err(Issue::Snapshot)?);
            if name == primary { current_remote = Some(fingerprint(&remote)); }
            report.saved = Some(now);
        }
        Operation::List | Operation::Delete(_) => unreachable!(),
    }
    report.applied = merge_into.is_some();
    if let Some(saved) = report.saved {
        if report.applied || current_remote.is_some() {
            let merged = sync::checkpoint(merge_into.as_deref().unwrap_or(&local), saved).map_err(Issue::Snapshot)?;
            access.lock().expect("config access").save_sync(path, &local, &merged).map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists { Issue::Changed } else { error.into() }
            })?;
            if current_remote.is_some() {
                let base = Baseline { target, local: local_fingerprint(&merged)?, remote: current_remote };
                let text = serde_json::to_string(&base).map_err(|_| Issue::Unavailable)?;
                atomic_write(&state_path, &text)?;
            }
        }
    }
    report.entries = store.list()?;
    Ok(report)
}

struct Store {
    root: PathBuf,
    git: bool,
    recovered: Vec<String>,
}

fn unix_nanos() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
}

fn seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn machine_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"].into_iter().find_map(|key| std::env::var(key).ok().filter(|v| sync::valid_name(v)))
        .or_else(|| fs::read_to_string("/etc/hostname").ok().map(|v| v.trim().into()).filter(|v: &String| sync::valid_name(v)))
        .unwrap_or_else(|| "Aster".into())
}

fn git<I, S>(dir: &Path, operation: &'static str, args: I) -> Result<Output, Issue>
where I: IntoIterator<Item = S>, S: AsRef<std::ffi::OsStr> {
    let mut command = Command::new("git");
    command.current_dir(dir).args(["-c", "protocol.ext.allow=never", "-c", "core.hooksPath=/dev/null", "-c", "core.autocrlf=false"])
        .args(args).env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null());
    for key in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"] { command.env_remove(key); }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.output().map_err(|error| if error.kind() == io::ErrorKind::NotFound { Issue::GitMissing } else { Issue::Git(operation) })
}

fn git_ok<I, S>(dir: &Path, operation: &'static str, args: I) -> Result<Output, Issue>
where I: IntoIterator<Item = S>, S: AsRef<std::ffi::OsStr> {
    let output = git(dir, operation, args)?;
    if output.status.success() { Ok(output) } else { Err(Issue::Git(operation)) }
}

fn clone_repo(dir: &Path, target: &Path, repository: &str) -> Result<(), Issue> {
    let folder = target.file_name().ok_or(Issue::StorePath)?;
    git_ok(dir, "clone", ["clone".as_ref(), "--".as_ref(), repository.as_ref(), folder])?;
    Ok(())
}

fn replace_checkout(root: &Path, pending: &Path, nonce: u128) -> Result<(), Issue> {
    let previous = root.with_file_name(format!("sync-repo.previous.{nonce}"));
    fs::rename(root, &previous)?;
    if let Err(error) = fs::rename(pending, root) {
        let _ = fs::rename(previous, root);
        return Err(error.into());
    }
    Ok(())
}

fn git_head(root: &Path, reference: &str) -> Result<Option<String>, Issue> {
    let output = git(root, "rev-parse", ["rev-parse", "--verify", reference])?;
    Ok(output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().into()))
}

fn git_revision_file(root: &Path, revision: &str, file: &str) -> Result<Option<String>, Issue> {
    let output = git(root, "show", ["show", &format!("{revision}:{file}")])?;
    if !output.status.success() { return Ok(None); }
    String::from_utf8(output.stdout).map(Some).map_err(|_| Issue::Git("show"))
}

fn recover_checkout(dir: &Path, root: &Path, repository: &str, local: &str, upstream: &str) -> Result<Vec<String>, Issue> {
    let common = git(root, "merge-base", ["merge-base", local, upstream])?;
    let ancestor = common.status.success().then(|| String::from_utf8_lossy(&common.stdout).trim().to_owned());
    if ancestor.as_deref().is_some_and(|base| base == local || base == upstream) { return Err(Issue::Git("pull")); }
    let changes = if let Some(ancestor) = &ancestor {
        git_ok(root, "diff", ["diff", "--name-only", "--no-renames", "-z", ancestor, local, "--", STORE_DIR])?
    } else {
        git_ok(root, "ls-tree", ["ls-tree", "-r", "--name-only", "-z", local, "--", STORE_DIR])?
    };
    let nonce = unix_nanos();
    let pending = dir.join(format!("sync-repo.pending.{nonce}"));
    clone_repo(dir, &pending, repository)?;
    for key in ["user.name", "user.email"] {
        let value = git(root, "config", ["config", "--local", "--get", key])?;
        if value.status.success() {
            git_ok(&pending, "config", ["config", "--local", key, String::from_utf8_lossy(&value.stdout).trim_end()])?;
        }
    }
    let store = Store { root: pending.join(STORE_DIR), git: true, recovered: Vec::new() };
    let mut conflicts = Vec::new();
    for file in changes.stdout.split(|&byte| byte == 0).filter_map(|bytes| std::str::from_utf8(bytes).ok()) {
        let Some(name) = file.strip_prefix("aster-sync/").and_then(|file| file.strip_suffix(".toml")).filter(|name| sync::valid_name(name)) else { continue };
        let previous = ancestor.as_deref().map(|base| git_revision_file(root, base, file)).transpose()?.flatten();
        let local = git_revision_file(root, local, file)?;
        let remote = store.read(name)?;
        if remote == local { continue; }
        let machine = local.as_deref().and_then(|text| sync::metadata(text).ok()).map_or_else(machine_name, |meta| meta.machine);
        match (local, remote == previous) {
            (Some(local), true) => store.write(name, &local, false, &machine)?,
            (Some(local), false) => conflicts.push(conflict_copy(&store, name, &local, &machine, seconds(SystemTime::now()))?),
            (None, true) => store.delete(name, &machine)?,
            (None, false) => {}
        }
    }
    replace_checkout(root, &pending, nonce)?;
    Ok(conflicts)
}

fn ensure_dir(path: &Path) -> Result<(), Issue> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(Issue::StorePath),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn read(path: &Path) -> Result<Option<String>, Issue> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(Some(fs::read_to_string(path)?)),
        Ok(_) => Err(Issue::StorePath),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn atomic_write(path: &Path, text: &str) -> Result<(), Issue> {
    let temp = path.with_extension(format!("{}.{}.tmp", std::process::id(), unix_nanos()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() { let _ = fs::remove_file(temp); }
    result.map_err(Into::into)
}

impl Store {
    fn folder(folder: &Path) -> Result<Self, Issue> {
        if !folder.is_dir() { return Err(Issue::StorePath); }
        let root = folder.join(STORE_DIR);
        ensure_dir(&root)?;
        Ok(Self { root, git: false, recovered: Vec::new() })
    }

    fn git(config_dir: &Path, repository: &str) -> Result<Self, Issue> {
        let root = config_dir.join("sync-repo");
        ensure_dir(&root)?;
        let mut recovered = Vec::new();
        if !root.exists() {
            clone_repo(config_dir, &root, repository)?;
        } else {
            let dot_git = root.join(".git");
            ensure_dir(&dot_git)?;
            if !dot_git.is_dir() { return Err(Issue::StorePath); }
            let origin = git_ok(&root, "config", ["config", "--get", "remote.origin.url"])?;
            if String::from_utf8_lossy(&origin.stdout).trim_end() != repository {
                let id = unix_nanos();
                let pending = config_dir.join(format!("sync-repo.pending.{id}"));
                clone_repo(config_dir, &pending, repository)?;
                replace_checkout(&root, &pending, id)?;
            } else {
                recovered = Self::refresh_checkout(config_dir, &root, repository)?;
            }
        }
        let store = root.join(STORE_DIR);
        ensure_dir(&store)?;
        Ok(Self { root: store, git: true, recovered })
    }

    fn refresh_checkout(dir: &Path, root: &Path, repository: &str) -> Result<Vec<String>, Issue> {
        let local = git_head(root, "HEAD")?;
        let upstream = git_head(root, "@{upstream}")?;
        if upstream.is_none() && git_ok(root, "ls-remote", ["ls-remote", "--heads", "origin"])?.stdout.is_empty() {
            if local.is_some() { git_ok(root, "push", ["push", "--set-upstream", "origin", "HEAD"])?; }
            return Ok(Vec::new());
        }
        if git(root, "pull", ["pull", "--ff-only"])?.status.success() {
            if upstream.is_some() && !git_ok(root, "rev-list", ["rev-list", "@{upstream}..HEAD"])?.stdout.is_empty() {
                git_ok(root, "push", ["push", "--set-upstream", "origin", "HEAD"])?;
            }
            return Ok(Vec::new());
        }
        let local = local.ok_or(Issue::Git("pull"))?;
        let upstream = match git_head(root, "@{upstream}")? {
            Some(upstream) => upstream,
            None => {
                let branch = git_ok(root, "branch", ["branch", "--show-current"])?;
                let branch = String::from_utf8_lossy(&branch.stdout);
                git_head(root, &format!("refs/remotes/origin/{}", branch.trim()))?.ok_or(Issue::Git("pull"))?
            }
        };
        recover_checkout(dir, root, repository, &local, &upstream)
    }

    fn path(&self, name: &str) -> Result<PathBuf, Issue> {
        if !sync::valid_name(name) { return Err(Issue::Name); }
        ensure_dir(&self.root)?;
        Ok(self.root.join(format!("{name}.toml")))
    }

    fn list(&self) -> Result<Vec<Entry>, Issue> {
        ensure_dir(&self.root)?;
        let dir = match fs::read_dir(&self.root) {
            Ok(dir) => dir,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut entries = Vec::new();
        for entry in dir {
            let entry = entry?;
            if !entry.file_type()?.is_file() { continue; }
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("toml") { continue; }
            let Some(name) = path.file_stem().and_then(|value| value.to_str()).filter(|name| sync::valid_name(name)) else { continue };
            let modified = entry.metadata()?.modified().map_or(0, seconds);
            let metadata = read(&path)?.and_then(|text| sync::metadata(&text).ok());
            let (machine, modified) = metadata.map_or_else(|| (String::new(), modified), |info| {
                (info.machine, if self.git { info.saved } else { modified })
            });
            entries.push(Entry { name: name.into(), machine, modified });
        }
        entries.sort_unstable_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
        Ok(entries)
    }

    fn read(&self, name: &str) -> Result<Option<String>, Issue> {
        read(&self.path(name)?)
    }

    fn write(&self, name: &str, text: &str, new: bool, machine: &str) -> Result<(), Issue> {
        self.write_snapshot(name, text, new, machine, None)
    }

    fn lock_mutation(&self) -> Result<Option<fs::File>, Issue> {
        if self.git { return Ok(None); }
        #[cfg(windows)]
        use aster_platform_windows::config_lock::lock;
        #[cfg(target_os = "linux")]
        use aster_platform_linux::config_lock::lock;
        let lock_path = self.root.parent().ok_or(Issue::StorePath)?.join(".aster-sync.lock");
        Ok(Some(lock(&lock_path)?))
    }

    fn write_snapshot(&self, name: &str, text: &str, new: bool, machine: &str, expected: Option<Option<&str>>) -> Result<(), Issue> {
        let path = self.path(name)?;
        let _lock = self.lock_mutation()?;
        fs::create_dir_all(&self.root)?;
        let existing = self.read(name)?;
        if expected.is_some_and(|expected| existing.as_deref() != expected) { return Err(Issue::StoreChanged); }
        if new && (existing.is_some() || self.list()?.iter().any(|entry| entry.name.eq_ignore_ascii_case(name))) {
            return Err(Issue::Exists);
        }
        if existing.as_deref() != Some(text) { atomic_write(&path, text)?; }
        self.publish(name, machine)
    }

    fn delete(&self, name: &str, machine: &str) -> Result<(), Issue> {
        let _lock = self.lock_mutation()?;
        let path = self.path(name)?;
        if self.read(name)?.is_none() { return Err(Issue::Missing); }
        fs::remove_file(path)?;
        self.publish(name, machine)
    }

    fn publish(&self, name: &str, machine: &str) -> Result<(), Issue> {
        if !self.git { return Ok(()); }
        let root = self.root.parent().ok_or(Issue::StorePath)?;
        let path = format!("{STORE_DIR}/{name}.toml");
        let pathspec = format!(":(literal){path}");
        git_ok(root, "add", ["add", "-A", "--", &pathspec])?;
        let staged = git_ok(root, "diff", ["diff", "--cached", "--name-only", "--", &pathspec])?;
        if !staged.stdout.is_empty() {
            git_ok(root, "commit", ["commit", "--only", "-m", &format!("{machine}: {name}"), "--", &pathspec])?;
        }
        git_ok(root, "push", ["push", "--set-upstream", "origin", "HEAD"])?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../test/sync.rs"]
mod tests;
