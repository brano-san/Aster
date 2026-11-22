use aster_core::config::CustomScheme;
use aster_core::update::newer;
use aster_ext::{Manifest, relative_path};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use zip::ZipArchive;

pub const MAX_ARCHIVE: u64 = 64 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 256 * 1024 * 1024;
const MAX_TEXT: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const MANIFEST: &str = "extension.toml";
pub const STAGING_PREFIX: &str = ".aster-stage_";

struct File {
    path: String,
    index: usize,
    size: u64,
    #[cfg(unix)]
    executable_bits: u32,
}

pub struct Package<'a> {
    pub manifest: Manifest,
    archive: ZipArchive<Cursor<&'a [u8]>>,
    files: Vec<File>,
}

impl<'a> Package<'a> {
    pub fn read(bytes: &'a [u8]) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_ARCHIVE { return Err("Extension archive is too large".into()); }
        let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(text)?;
        if archive.len() > MAX_ENTRIES { return Err("Extension archive contains too many entries".into()); }
        let mut entries = HashSet::new();
        let mut paths = HashMap::new();
        let mut files = Vec::new();
        let mut dirs = HashSet::new();
        let mut total = 0u64;
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(text)?;
            let mut path = entry.name().replace('\\', "/");
            let directory = entry.is_dir();
            if directory { path = path.trim_end_matches('/').into(); }
            relative_path(&path)?;
            let kind = entry.unix_mode().unwrap_or_default() & 0o170000;
            if !matches!(kind, 0 | 0o100000 | 0o040000) || entry.is_symlink() {
                return Err(format!("Unsupported archive entry: {path}"));
            }
            if !entries.insert(path.clone()) {
                return Err(format!("Duplicate archive entry: {path}"));
            }
            for i in path.match_indices('/').map(|(i, _)| i).chain(std::iter::once(path.len())) {
                let component = &path[..i];
                if paths.insert(component.to_lowercase(), component.to_owned()).is_some_and(|known| known != component) {
                    return Err(format!("Conflicting archive path: {path}"));
                }
                if i < path.len() || directory { dirs.insert(component.to_owned()); }
            }
            if directory { continue; }
            total = total.saturating_add(entry.size());
            if total > MAX_EXTRACTED { return Err("Unpacked extension is too large".into()); }
            files.push(File {
                path, index, size: entry.size(),
                #[cfg(unix)]
                executable_bits: entry.unix_mode().unwrap_or_default() & 0o111,
            });
        }
        if files.iter().any(|file| dirs.contains(&file.path)) {
            return Err("An archive path is both a file and a directory".into());
        }
        if !files.iter().any(|file| file.path == MANIFEST) {
            let prefix = files.first().and_then(|file| file.path.split_once('/')).map(|(prefix, _)| format!("{prefix}/"))
                .ok_or("Missing extension.toml")?;
            for file in &mut files {
                file.path = file.path.strip_prefix(&prefix).ok_or("Expected one extension directory in the archive")?.into();
            }
        }
        let manifest = Manifest::parse(&Self::read_text(&mut archive, &files, MANIFEST)?)?;
        if let Some(command) = &manifest.command {
            let command = command.replace('\\', "/");
            if !files.iter().any(|file| file.path == command) {
                return Err(format!("Missing extension executable: {command}"));
            }
        }
        for scheme in &manifest.schemes {
            let source = Self::read_text(&mut archive, &files, &scheme.file)?;
            let scheme_data: CustomScheme = toml::from_str(&source).map_err(text)?;
            if scheme_data.name.trim().is_empty() || scheme_data.palette().is_none() {
                return Err(format!("Invalid extension color scheme: {}", scheme.file));
            }
        }
        Ok(Self { manifest, archive, files })
    }

    fn read_text(archive: &mut ZipArchive<Cursor<&[u8]>>, files: &[File], path: &str) -> Result<String, String> {
        let normalized = path.replace('\\', "/");
        let file = files.iter().find(|file| file.path == normalized).ok_or_else(|| format!("Missing extension file: {path}"))?;
        let mut content = String::new();
        archive.by_index(file.index).map_err(text)?.take(MAX_TEXT + 1).read_to_string(&mut content).map_err(text)?;
        if content.len() as u64 > MAX_TEXT { return Err(format!("Extension file is too large: {path}")); }
        Ok(content)
    }

    pub fn check_version(&self, installed: Option<&str>, expected: Option<(&str, &str)>) -> Result<(), String> {
        if expected.is_some_and(|(id, version)| self.manifest.id != id || self.manifest.version != version) {
            return Err("The extension manifest does not match the catalog".into());
        }
        if installed.is_some_and(|version| !newer(&self.manifest.version, version)) {
            return Err("The extension is already installed at this version or a newer version".into());
        }
        Ok(())
    }

    pub fn stage(mut self, root: &Path) -> Result<Staging, String> {
        let stage = Staging::new(root)?;
        for file in self.files {
            let path = stage.0.join(&file.path);
            if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(text)?; }
            let mut output = fs::File::options().create_new(true).write(true).open(&path).map_err(text)?;
            let mut input = self.archive.by_index(file.index).map_err(text)?.take(file.size + 1);
            if io::copy(&mut input, &mut output).map_err(text)? != file.size {
                return Err(format!("Archive entry size does not match: {}", file.path));
            }
            #[cfg(unix)]
            if file.executable_bits != 0 {
                use std::os::unix::fs::PermissionsExt;
                output.set_permissions(fs::Permissions::from_mode(0o644 | file.executable_bits)).map_err(text)?;
            }
        }
        #[cfg(unix)]
        if let Some(command) = self.manifest.command {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(stage.0.join(command.replace('\\', "/")), fs::Permissions::from_mode(0o755)).map_err(text)?;
        }
        Ok(stage)
    }
}

pub struct Staging(PathBuf);

impl Staging {
    fn new(root: &Path) -> Result<Self, String> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        fs::create_dir_all(root).map_err(text)?;
        loop {
            let path = root.join(format!("{STAGING_PREFIX}{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        }
    }

    pub fn place(self, root: &Path, destination: &Path, updating: bool) -> Result<(), String> {
        if !updating {
            if destination.symlink_metadata().is_ok() { return Err("The extension directory already exists".into()); }
            return fs::rename(&self.0, destination).map_err(text);
        }
        let backup = Self::new(root)?;
        let original = backup.0.join("original");
        fs::rename(destination, &original).map_err(text)?;
        if let Err(error) = fs::rename(&self.0, destination) {
            if let Err(rollback) = fs::rename(&original, destination) {
                let path = backup.0.display().to_string();
                std::mem::forget(backup);
                return Err(format!("{error}. Could not restore the extension from {path}: {rollback}"));
            }
            return Err(error.to_string());
        }
        Ok(())
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            if error.kind() != io::ErrorKind::NotFound {
                tracing::warn!("{}: {error}", self.0.display());
            }
        }
    }
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
#[path = "../test/extension_package.rs"]
mod tests;
