use crate::extension_package::{MAX_ARCHIVE, STAGING_PREFIX, Package};
use crate::extension_process::{Process, Update};
use aster_core::config::CustomScheme;
use aster_core::extensions::{Action, CatalogEntry, Installed, Reply, Request, Snapshot, Status, https_url};
use aster_ext::{Registry, ScanError};
use aster_platform::{Platform, WindowId};
use crossbeam_channel::{Sender, select_biased, unbounded};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{Read, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

const MANIFEST: &str = "extension.toml";

pub enum Event {
    Snapshot(Arc<Snapshot>),
    Action { extension: String, action: Action },
    Reply(WindowId, Reply),
}

enum Input {
    Scan { disabled: Vec<String>, language: String },
    Request(WindowId, Request),
}

enum Download {
    Catalog(WindowId, String, Result<Vec<CatalogEntry>, String>),
    Package(WindowId, CatalogEntry, Result<Vec<u8>, String>),
}

pub struct Host(Sender<Input>);

pub struct Worker(Option<JoinHandle<()>>);

struct Mounted {
    installed: Installed,
    generation: u64,
    enabled: bool,
    process: Option<Process>,
}

impl Host {
    pub fn start<P: Platform>(root: Option<PathBuf>, post: impl Fn(Event) + Send + 'static) -> std::io::Result<(Self, Worker)> {
        let (input, requests) = unbounded();
        let (updates, process_events) = unbounded();
        let (downloaded, downloads) = unbounded();
        let worker = thread::Builder::new().name("aster-extensions".into()).spawn(move || {
            let mut state = State::default();
            loop {
                select_biased! {
                    recv(requests) -> request => match request {
                        Ok(Input::Scan { disabled, language }) => {
                            state.disabled = disabled;
                            state.language = language;
                            state.reconcile::<P>(root.as_deref(), &updates);
                            post(state.snapshot());
                        }
                        Ok(Input::Request(_, Request::Run { extension, command, title, cwd, cwd_pid })) => {
                            let cwd = cwd.or_else(|| cwd_pid.and_then(P::process_cwd));
                            state.run(&extension, &command, &title, cwd.as_deref());
                        }
                        Ok(Input::Request(window, Request::Catalog(url))) => {
                            let reply_url = url.clone();
                            if let Err(error) = fetch(&downloaded, move || {
                                let result = download::<P>(&url, 1024 * 1024).and_then(|bytes| catalog(&bytes));
                                Download::Catalog(window, url, result)
                            }) {
                                post(Event::Reply(window, Reply::Catalog { url: reply_url, result: Err(error.to_string()) }));
                            }
                        }
                        Ok(Input::Request(window, Request::InstallCatalog(entry))) => {
                            if let Err(error) = fetch(&downloaded, move || {
                                let result = download::<P>(&entry.url, MAX_ARCHIVE);
                                Download::Package(window, entry, result)
                            }) {
                                post(Event::Reply(window, Reply::Operation(Err(error.to_string()))));
                            }
                        }
                        Ok(Input::Request(window, Request::InstallFile(path))) => {
                            let result = read_archive(&path).and_then(|bytes| state.install(root.as_deref(), &bytes, None));
                            state.reconcile::<P>(root.as_deref(), &updates);
                            post(state.snapshot());
                            post(Event::Reply(window, Reply::Operation(result)));
                        }
                        Ok(Input::Request(window, Request::Remove(id))) => {
                            let result = state.remove(root.as_deref(), &id);
                            state.reconcile::<P>(root.as_deref(), &updates);
                            post(state.snapshot());
                            post(Event::Reply(window, Reply::Operation(result)));
                        }
                        Ok(Input::Request(window, Request::OpenFolder(id))) => {
                            if let Err(error) = state.open_folder::<P>(root.as_deref(), id.as_deref()) {
                                post(Event::Reply(window, Reply::Operation(Err(error))));
                            }
                        }
                        Err(_) => break,
                    },
                    recv(downloads) -> event => match event {
                        Ok(Download::Catalog(window, url, result)) => post(Event::Reply(window, Reply::Catalog { url, result })),
                        Ok(Download::Package(window, entry, bytes)) => {
                            let result = bytes.and_then(|bytes| state.install(root.as_deref(), &bytes, Some((&entry.id, &entry.version))));
                            state.reconcile::<P>(root.as_deref(), &updates);
                            post(state.snapshot());
                            post(Event::Reply(window, Reply::Operation(result)));
                        }
                        Err(_) => {}
                    },
                    recv(process_events) -> event => {
                        let Ok((extension, generation, update)) = event else { continue };
                        let Some(entry) = state.mounted.get_mut(&extension).filter(|entry| entry.generation == generation) else { continue };
                        match update {
                            Update::Ready(profiles) => {
                                entry.installed.profiles = profiles.into_iter().filter(|p| !p.name.trim().is_empty() && !p.command.trim().is_empty()).collect();
                                entry.installed.status = Status::Running;
                            }
                            Update::Failed(error) => {
                                entry.installed.profiles.clear();
                                entry.installed.status = Status::Failed(error);
                                entry.process.take();
                            }
                            Update::Action(action) => {
                                post(Event::Action { extension, action });
                                continue;
                            }
                        }
                        post(state.snapshot());
                    }
                }
            }
            stop_all(state.mounted.into_values());
        })?;
        Ok((Self(input), Worker(Some(worker))))
    }

    pub fn scan(&self, disabled: Vec<String>, language: String) {
        let _ = self.0.send(Input::Scan { disabled, language });
    }

    pub fn send(&self, window: WindowId, request: Request) {
        let _ = self.0.send(Input::Request(window, request));
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            let _ = worker.join();
        }
    }
}

fn stop_all(entries: impl IntoIterator<Item = Mounted>) {
    let mut processes: Vec<_> = entries.into_iter().filter_map(|entry| entry.process).collect();
    for process in &mut processes {
        process.stop();
    }
    for process in processes {
        let _ = process.thread.join();
    }
}

#[derive(Default)]
struct State {
    mounted: BTreeMap<String, Mounted>,
    errors: Vec<ScanError>,
    generation: u64,
    disabled: Vec<String>,
    language: String,
}

impl State {
    fn reconcile<P: Platform>(&mut self, root: Option<&Path>, updates: &Sender<(String, u64, Update)>) {
        let registry = root.map(|root| scan(root, &self.disabled)).unwrap_or_default();
        self.errors = registry.errors;
        let mut previous = std::mem::take(&mut self.mounted);
        for entry in registry.entries {
            let id = entry.manifest.id.clone();
            let schemes = if entry.enabled { load_schemes(&entry, &mut self.errors) } else { Vec::new() };
            if previous.get(&id).is_some_and(|old| old.installed.directory == entry.directory
                && old.installed.manifest == entry.manifest && old.enabled == entry.enabled) {
                let mut old = previous.remove(&id).unwrap();
                old.installed.schemes = schemes;
                self.mounted.insert(id, old);
                continue;
            }
            stop_all(previous.remove(&id));
            self.generation += 1;
            let mut installed = Installed { manifest: entry.manifest, directory: entry.directory, status: Status::Stopped, schemes, profiles: Vec::new() };
            let mut process = None;
            if entry.enabled && installed.manifest.command.is_some() {
                match Process::start::<P>(&installed, self.generation, self.language.clone(), updates.clone()) {
                    Ok(started) => {
                        process = Some(started);
                        installed.status = Status::Starting;
                    }
                    Err(error) => installed.status = Status::Failed(error.to_string()),
                }
            }
            self.mounted.insert(id, Mounted { installed, generation: self.generation, enabled: entry.enabled, process });
        }
        stop_all(previous.into_values());
    }

    fn snapshot(&self) -> Event {
        Event::Snapshot(Arc::new(Snapshot {
            installed: self.mounted.values().map(|entry| entry.installed.clone()).collect(), errors: self.errors.clone(),
        }))
    }

    fn run(&self, extension: &str, command: &str, title: &str, cwd: Option<&str>) {
        if let Some(entry) = self.mounted.get(extension).filter(|entry| entry.installed.status == Status::Running
            && entry.installed.manifest.commands.iter().any(|item| item.id == command)) {
            if let Some(process) = &entry.process {
                process.send(command, title, cwd);
            }
        }
    }

    fn install(&mut self, root: Option<&Path>, bytes: &[u8], expected: Option<(&str, &str)>) -> Result<(), String> {
        let root = root.ok_or("Extensions folder is unavailable")?;
        let package = Package::read(bytes)?;
        let id = package.manifest.id.clone();
        let registry = scan(root, &self.disabled);
        if registry.duplicate_ids.contains(&id) {
            return Err(format!("Duplicate extension id: {id}"));
        }
        let existing = registry.entries.iter().find(|entry| entry.manifest.id == id);
        package.check_version(existing.map(|entry| entry.manifest.version.as_str()), expected)?;
        let destination = existing.map_or_else(|| root.join(&id), |entry| entry.directory.clone());
        let stage = package.stage(root)?;
        stop_all(self.mounted.remove(&id));
        stage.place(root, &destination, existing.is_some())
    }

    fn remove(&mut self, root: Option<&Path>, id: &str) -> Result<(), String> {
        let root = root.ok_or("Extensions folder is unavailable")?;
        let path = self.directory(id)?.to_owned();
        if !direct_subdirectory(root, &path) {
            return Err("Extension directory is outside the extensions folder".into());
        }
        let manifest = aster_ext::Manifest::parse(&read_text(&path, MANIFEST)?)?;
        if manifest.id != id { return Err("The extension directory has changed".into()); }
        stop_all(self.mounted.remove(id));
        fs::remove_dir_all(path).map_err(|error| error.to_string())
    }

    fn open_folder<P: Platform>(&self, root: Option<&Path>, id: Option<&str>) -> Result<(), String> {
        let root = root.ok_or("Extensions folder is unavailable")?;
        fs::create_dir_all(root).map_err(|error| error.to_string())?;
        let path = match id { Some(id) => self.directory(id)?, None => root };
        P::open(&path.to_string_lossy());
        Ok(())
    }

    fn directory(&self, id: &str) -> Result<&Path, String> {
        self.mounted.get(id).map(|entry| entry.installed.directory.as_path()).ok_or_else(|| "Extension is no longer installed".into())
    }
}

fn fetch(out: &Sender<Download>, job: impl FnOnce() -> Download + Send + 'static) -> std::io::Result<()> {
    let out = out.clone();
    thread::Builder::new().name("aster-extension-download".into()).spawn(move || {
        let _ = out.send(job());
    }).map(|_| ())
}

fn download<P: Platform>(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    if !https_url(url) { return Err("Only HTTPS addresses are supported".into()); }
    let mut bytes = Vec::new();
    let mut within_limit = true;
    let result = P::download(url, &mut bytes, &mut |received, total| {
        within_limit = received <= limit && total.is_none_or(|total| total <= limit);
        within_limit
    });
    if !within_limit { return Err("Extension download is too large".into()); }
    result.map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn read_archive(path: &Path) -> Result<Vec<u8>, String> {
    if !path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("zip")) {
        return Err("Choose a .zip extension archive".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path).and_then(|file| file.take(MAX_ARCHIVE + 1).read_to_end(&mut bytes)).map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn catalog(bytes: &[u8]) -> Result<Vec<CatalogEntry>, String> {
    let mut entries: Vec<CatalogEntry> = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let mut ids = HashSet::new();
    for entry in &entries {
        if !aster_ext::valid_id(&entry.id) || entry.name.trim().is_empty() || entry.version.trim().is_empty()
            || !https_url(&entry.url) || !ids.insert(&entry.id) {
            return Err(format!("Invalid or duplicate catalog entry: {}", entry.id));
        }
    }
    entries.sort_by_cached_key(|entry| entry.name.to_lowercase());
    Ok(entries)
}

fn direct_subdirectory(root: &Path, directory: &Path) -> bool {
    let Ok(root) = root.canonicalize() else { return false };
    directory.canonicalize().is_ok_and(|path| path.parent() == Some(root.as_path()) && path.file_name() == directory.file_name())
}

fn scan(root: &Path, disabled: &[String]) -> Registry {
    let dirs = match fs::read_dir(root) {
        Ok(dirs) => dirs,
        Err(error) if error.kind() == ErrorKind::NotFound => return Registry::default(),
        Err(error) => return Registry { errors: vec![ScanError { directory: root.into(), error: error.to_string() }], ..Registry::default() },
    };
    let mut sources = Vec::new();
    let mut errors = Vec::new();
    for dir in dirs {
        let path = match dir {
            Ok(dir) if dir.file_type().is_ok_and(|kind| kind.is_dir() && !kind.is_symlink()) => dir.path(),
            Ok(_) => continue,
            Err(error) => {
                errors.push(ScanError { directory: root.into(), error: error.to_string() });
                continue;
            }
        };
        if path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(STAGING_PREFIX)) || !direct_subdirectory(root, &path) {
            continue;
        }
        match read_text(&path, MANIFEST) {
            Ok(text) => sources.push((path, text)),
            Err(error) => errors.push(ScanError { directory: path, error }),
        }
    }
    let mut registry = Registry::scan(sources, disabled);
    registry.errors.append(&mut errors);
    registry
}

fn load_schemes(entry: &aster_ext::Entry, errors: &mut Vec<ScanError>) -> Vec<CustomScheme> {
    entry.manifest.schemes.iter().filter_map(|scheme| {
        match read_text(&entry.directory, &scheme.file).and_then(|text| toml::from_str::<CustomScheme>(&text).map_err(|error| error.to_string())) {
            Ok(scheme) if !scheme.name.trim().is_empty() && scheme.palette().is_some() => Some(scheme),
            result => {
                let error = result.err().unwrap_or_else(|| "Invalid extension color scheme".into());
                errors.push(ScanError { directory: entry.directory.join(&scheme.file), error });
                None
            }
        }
    }).collect()
}

pub fn entry_path(directory: &Path, relative: &str) -> Result<PathBuf, String> {
    aster_ext::relative_path(relative)?;
    let base = directory.canonicalize().map_err(|error| error.to_string())?;
    let file = relative.split(['/', '\\']).fold(base.clone(), |path, part| path.join(part))
        .canonicalize().map_err(|error| error.to_string())?;
    if !file.starts_with(&base) || !file.is_file() {
        return Err(format!("Extension file is outside its directory or is not a file: {relative}"));
    }
    Ok(file)
}

fn read_text(directory: &Path, relative: &str) -> Result<String, String> {
    let file = fs::File::open(entry_path(directory, relative)?).map_err(|error| error.to_string())?;
    let mut text = String::new();
    const LIMIT: u64 = 1024 * 1024;
    file.take(LIMIT + 1).read_to_string(&mut text).map_err(|error| error.to_string())?;
    if text.len() as u64 > LIMIT { return Err(format!("Extension file is too large: {relative}")); }
    Ok(text)
}
