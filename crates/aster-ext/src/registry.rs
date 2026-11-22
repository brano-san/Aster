use crate::Manifest;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct Registry {
    pub entries: Vec<Entry>,
    pub errors: Vec<ScanError>,
    pub duplicate_ids: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub directory: PathBuf,
    pub manifest: Manifest,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct ScanError {
    pub directory: PathBuf,
    pub error: String,
}

impl Registry {
    pub fn scan(mut folders: Vec<(PathBuf, String)>, disabled: &[String]) -> Self {
        folders.sort_by(|a, b| a.0.cmp(&b.0));
        let mut candidates = BTreeMap::<_, Vec<_>>::new();
        let mut registry = Self::default();
        for (directory, text) in folders {
            match Manifest::parse(&text) {
                Ok(manifest) => candidates.entry(manifest.id.clone()).or_default().push((directory, manifest)),
                Err(error) => registry.errors.push(ScanError { directory, error }),
            }
        }
        for (id, mut candidates) in candidates {
            if candidates.len() == 1 {
                let (directory, manifest) = candidates.pop().unwrap();
                registry.entries.push(Entry { directory, manifest, enabled: !disabled.contains(&id) });
            } else {
                registry.duplicate_ids.insert(id.clone());
                registry.errors.extend(candidates.into_iter().map(|(directory, _)| ScanError {
                    directory, error: format!("Duplicate extension id: {id}"),
                }));
            }
        }
        registry
    }
}

#[cfg(test)]
#[path = "../test/registry.rs"]
mod tests;
