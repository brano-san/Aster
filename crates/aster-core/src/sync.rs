use crate::config_schema::{self, CURRENT};
use crate::i18n::{t, tf};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use toml::{Table, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    Off,
    Folder,
    Git,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Preferences {
    pub backend: Backend,
    pub folder: String,
    pub repository: String,
    pub name: String,
    pub parts: Vec<Part>,
    pub automatic: bool,
    pub last_synced: u64,
}

impl Default for Preferences {
    fn default() -> Self {
        Self { backend: Backend::Off, folder: String::new(), repository: String::new(), name: String::new(),
            parts: Part::DEFAULT.to_vec(), automatic: false, last_synced: 0 }
    }
}

impl Preferences {
    pub fn same_store(&self, other: &Self) -> bool {
        self.backend == other.backend && match self.backend {
            Backend::Off => true,
            Backend::Folder => self.folder == other.folder,
            Backend::Git => self.repository == other.repository,
        }
    }

    pub fn same_target(&self, other: &Self) -> bool {
        self.same_store(other) && self.name == other.name && self.parts == other.parts
    }

    pub fn ready(&self) -> bool {
        match self.backend {
            Backend::Off => false,
            Backend::Folder => !self.folder.trim().is_empty(),
            Backend::Git => !self.repository.trim().is_empty(),
        }
    }

    pub fn name<'a>(&'a self, machine: &'a str) -> &'a str {
        let name = self.name.trim();
        if name.is_empty() { machine } else { name }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    List,
    Automatic,
    Download(String),
    Upload { name: String, new: bool },
    Delete(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub machine: String,
    pub modified: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Issue {
    Unavailable,
    StorePath,
    GitMissing,
    Git(&'static str),
    Name,
    Exists,
    Missing,
    Changed,
    StoreChanged,
    Io(String),
    Snapshot(String),
}

impl Issue {
    pub fn message(&self) -> String {
        match self {
            Self::Unavailable => t("Config sync is unavailable").into(),
            Self::StorePath => t("The sync store path is unavailable").into(),
            Self::GitMissing => t("Git was not found on PATH").into(),
            Self::Git(operation) => tf("Git {operation} failed", &[("operation", operation)]),
            Self::Name => t("Enter a valid configuration name").into(),
            Self::Exists => t("A configuration with this name already exists").into(),
            Self::Missing => t("The configuration snapshot no longer exists").into(),
            Self::Changed => t("The local configuration changed during sync. Try again.").into(),
            Self::StoreChanged => t("The snapshot changed in the store during sync. Try again.").into(),
            Self::Io(error) => tf("Cannot access the sync store: {error}", &[("error", error)]),
            Self::Snapshot(error) => error.clone(),
        }
    }
}

impl From<std::io::Error> for Issue {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub entries: Vec<Entry>,
    pub saved: Option<u64>,
    pub conflict: Option<String>,
    pub git_conflicts: Vec<String>,
    pub applied: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Syncing,
    Synced(u64),
    Error(Issue),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub machine: String,
    pub status: Status,
    pub entries: Vec<Entry>,
    pub conflict: Option<String>,
    pub git_conflicts: Vec<String>,
}

pub(crate) fn git_conflict_message(names: &[String]) -> String {
    tf("Git sync preserved local snapshots as {names}.", &[("names", &names.join(", "))])
}

impl crate::app::App {
    pub fn set_sync_view(&mut self, view: View) {
        if view != self.sync_view {
            self.sync_view = view;
            self.refresh_sync_rows();
            self.dirty = true;
        }
    }

    pub fn notify_sync_conflict(&mut self, name: &str) {
        self.notify_sync_conflict_body(tf("Remote settings applied. Local settings saved as {name}.", &[("name", name)]));
    }

    pub fn notify_git_sync_conflicts(&mut self, names: &[String]) {
        self.notify_sync_conflict_body(git_conflict_message(names));
    }

    fn notify_sync_conflict_body(&mut self, body: String) {
        self.push(crate::app::Effect::Notify(crate::notifications::Notification {
            tab: self.tabs.get(self.active).map_or(0, |tab| tab.key),
            title: t("Configuration sync conflict").into(),
            body,
            action: t("Open").into(),
        }));
    }
}

pub fn valid_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 220 || name.trim() != name || name.ends_with('.') ||
        name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')) {
        return false;
    }
    let base = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") { return false; }
    let number = base.strip_prefix("COM").or_else(|| base.strip_prefix("LPT"));
    !matches!(number, Some("0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Part {
    Shortcuts,
    Appearance,
    Profiles,
    Terminal,
    Vault,
}

impl Part {
    pub const ALL: [Self; 5] = [Self::Shortcuts, Self::Appearance, Self::Profiles, Self::Terminal, Self::Vault];
    pub const DEFAULT: [Self; 4] = [Self::Shortcuts, Self::Appearance, Self::Profiles, Self::Terminal];

    fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Shortcuts => &["hotkeys"],
            Self::Appearance => &["appearance", "colors", "color-schemes", "window", "dock"],
            Self::Profiles => &["profiles", "hidden-profiles", "groups", "profile-defaults"],
            Self::Terminal => &["terminal", "input", "ssh", "panes"],
            Self::Vault => &["vault"],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    pub machine: String,
    pub saved: u64,
    pub schema: u32,
}

#[derive(Serialize, Deserialize)]
struct Header {
    machine: String,
    saved: u64,
    schema: u32,
    #[serde(default)]
    parts: Option<Vec<Part>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Upload,
    Download,
    Conflict,
    Nothing,
}

pub fn decide(local_changed_at: u64, remote_saved: u64, last_synced: u64) -> Direction {
    match (local_changed_at > last_synced, remote_saved > last_synced) {
        (true, true) => Direction::Conflict,
        (true, false) => Direction::Upload,
        (false, true) => Direction::Download,
        (false, false) => Direction::Nothing,
    }
}

fn invalid_document() -> String {
    t("Invalid configuration snapshot").into()
}

fn read_header(table: &Table) -> Result<Header, String> {
    table.get("sync").cloned().ok_or_else(invalid_document)?.try_into().map_err(|_| invalid_document())
}

pub fn metadata(text: &str) -> Result<Metadata, String> {
    let Header { machine, saved, schema, .. } = read_header(&toml::from_str(text).map_err(|_| invalid_document())?)?;
    Ok(Metadata { machine, saved, schema })
}

pub fn same_snapshot(a: &str, b: &str) -> bool {
    fn without_time(text: &str) -> Option<Table> {
        let mut table: Table = toml::from_str(text).ok()?;
        let header = table.get_mut("sync")?.as_table_mut()?;
        header.remove("saved");
        header.remove("machine");
        Some(table)
    }
    without_time(a).is_some_and(|a| without_time(b).as_ref() == Some(&a))
}

fn check_schema(schema: u32) -> Result<(), String> {
    if schema > CURRENT {
        return Err(tf("Sync schema {version} is newer than this build supports", &[("version", &schema.to_string())]));
    }
    Ok(())
}

fn read_local(text: &str) -> Result<Table, String> {
    let value: Value = toml::from_str(text).map_err(|_| invalid_document())?;
    let schema = config_schema::version(&value).ok_or_else(invalid_document)?;
    check_schema(schema)?;
    let Value::Table(table) = config_schema::migrate(value, schema) else { return Err(invalid_document()) };
    Ok(table)
}

fn shared_field(table: &str, key: &str) -> bool {
    if table == "vault" { return key == "sealed"; }
    if matches!(key, "portable" | "portable-path" | "portable-dir") { return false; }
    !matches!((table, key),
        ("window" | "appearance", "x" | "y" | "left" | "top" | "width" | "height" | "position" | "size" | "maximized" | "monitor")
        | ("appearance", "background-image")
        | ("dock", "screen")
        | ("ssh", "agent-pipe" | "windows-agent" | "x11-display")
    )
}

fn validate_seal(value: &Value) -> Result<(), String> {
    let sealed = value.as_str().ok_or_else(invalid_document)?;
    if sealed.is_empty() { return Ok(()); }
    if sealed.strip_prefix("aster-vault:1:").is_some_and(|payload| {
        let content = payload.trim_end_matches('=');
        payload.len() >= 76 && payload.len() % 4 == 0 && payload.len() - content.len() <= 2
            && content.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
    }) {
        return Ok(());
    }
    Err(t("The sync snapshot requires an encrypted vault").into())
}

fn filter_shared(key: &str, value: &Value) -> Result<Option<Value>, String> {
    if key == "vault" {
        let vault = value.as_table().ok_or_else(invalid_document)?;
        if let Some(seal) = vault.get("sealed") { validate_seal(seal)?; }
    }
    match value {
        Value::Table(table) => {
            let fields: Table = table.iter().filter(|(field, _)| shared_field(key, field)).map(|(k, v)| (k.clone(), v.clone())).collect();
            Ok((!fields.is_empty()).then_some(Value::Table(fields)))
        }
        value => Ok(Some(value.clone())),
    }
}

pub fn extract(local_toml: &str, parts: &[Part]) -> String {
    extract_at(local_toml, parts, "", 0).unwrap_or_default()
}

pub fn extract_at(local_toml: &str, parts: &[Part], machine: &str, saved: u64) -> Result<String, String> {
    let local = read_local(local_toml)?;
    let mut snapshot = Table::new();
    for &part in parts {
        for &key in part.keys() {
            if let Some(value) = local.get(key).map(|value| filter_shared(key, value)).transpose()?.flatten() {
                snapshot.insert(key.into(), value);
            }
        }
    }
    let header = Header { machine: machine.into(), saved, schema: CURRENT, parts: Some(parts.to_vec()) };
    snapshot.insert("sync".into(), Value::try_from(header).map_err(|_| invalid_document())?);
    toml::to_string_pretty(&snapshot).map_err(|_| invalid_document())
}

pub fn merge(local_toml: &str, remote_toml: &str, parts: &[Part]) -> Result<String, String> {
    let mut local = read_local(local_toml)?;
    let remote: Table = toml::from_str(remote_toml).map_err(|_| invalid_document())?;
    let header = read_header(&remote)?;
    check_schema(header.schema)?;
    let remote = config_schema::migrate(Value::Table(remote), header.schema);
    for &part in parts {
        if header.parts.as_ref().is_some_and(|parts| !parts.contains(&part)) { continue; }
        for &key in part.keys() {
            let mut next = remote.get(key).map(|value| filter_shared(key, value)).transpose()?.flatten();
            if let Some(previous) = local.remove(key).and_then(|value| match value { Value::Table(table) => Some(table), _ => None }) {
                let fields = previous.into_iter().filter(|(field, _)| !shared_field(key, field)).collect::<Table>();
                if !fields.is_empty() {
                    let target = next.get_or_insert_with(|| Value::Table(Table::new())).as_table_mut().ok_or_else(invalid_document)?;
                    target.extend(fields);
                }
            }
            if let Some(value) = next { local.insert(key.into(), value); }
        }
    }
    write_local(local)
}

pub fn checkpoint(local_toml: &str, saved: u64) -> Result<String, String> {
    let mut local = read_local(local_toml)?;
    let sync = local.entry("sync").or_insert_with(|| Value::Table(Table::new())).as_table_mut().ok_or_else(invalid_document)?;
    sync.insert("last-synced".into(), Value::try_from(saved).map_err(|_| invalid_document())?);
    write_local(local)
}

pub fn rebase_settings(base: &crate::config::Config, edited: &crate::config::Config, disk: &crate::config::Config) -> Result<crate::config::Config, String> {
    let reset_checkpoint = !base.sync.same_target(&edited.sync);
    let checkpoint = edited.sync.last_synced;
    let base = Table::try_from(base).map_err(|_| invalid_document())?;
    let edited = Table::try_from(edited).map_err(|_| invalid_document())?;
    let mut disk = Table::try_from(disk).map_err(|_| invalid_document())?;
    apply_edits(&base, &edited, &mut disk);
    let mut config: crate::config::Config = disk.try_into().map_err(|_| invalid_document())?;
    if reset_checkpoint { config.sync.last_synced = checkpoint; }
    Ok(config)
}

fn apply_edits(base: &Table, edited: &Table, disk: &mut Table) {
    disk.retain(|key, _| !base.contains_key(key) || edited.contains_key(key));
    for (key, value) in edited {
        let old = base.get(key);
        if old == Some(value) { continue; }
        if let (Some(Value::Table(base)), Value::Table(edited), Some(Value::Table(disk))) = (old, value, disk.get_mut(key)) {
            apply_edits(base, edited, disk);
        } else if let Some(merged) = disk.get(key).and_then(|disk| merge_edited_items(key, old?, value, disk)) {
            disk.insert(key.clone(), merged);
        } else {
            disk.insert(key.clone(), value.clone());
        }
    }
}

fn merge_edited_items(key: &str, base: &Value, edited: &Value, disk: &Value) -> Option<Value> {
    let field = match key {
        "profiles" | "groups" => "id",
        "color-schemes" => "name",
        _ => return None,
    };
    let base = OrderedItems::new(base.as_array()?, field)?;
    let edited = OrderedItems::new(edited.as_array()?, field)?;
    let disk = OrderedItems::new(disk.as_array()?, field)?;
    let (first, last) = if base.order == edited.order { (&disk.order, &edited.order) } else { (&edited.order, &disk.order) };
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();
    for &id in first.iter().chain(last).filter(|&&id| seen.insert(id)) {
        let original = base.items.get(id).copied();
        let change = edited.items.get(id).copied();
        let current = disk.items.get(id).copied();
        let value = match (original, change, current) {
            (old, new, current) if old == new => current.cloned(),
            (Some(Value::Table(old)), Some(Value::Table(new)), Some(Value::Table(current))) => {
                let mut current = current.clone();
                apply_edits(old, new, &mut current);
                Some(Value::Table(current))
            }
            (_, new, _) => new.cloned(),
        };
        merged.extend(value);
    }
    Some(Value::Array(merged))
}

struct OrderedItems<'a> {
    order: Vec<&'a str>,
    items: BTreeMap<&'a str, &'a Value>,
}

impl<'a> OrderedItems<'a> {
    fn new(values: &'a [Value], field: &str) -> Option<Self> {
        let mut order = Vec::with_capacity(values.len());
        let mut items = BTreeMap::new();
        for value in values {
            let id = value.get(field)?.as_str()?;
            if id.is_empty() || items.insert(id, value).is_some() { return None; }
            order.push(id);
        }
        Some(Self { order, items })
    }
}

fn write_local(mut local: Table) -> Result<String, String> {
    local.remove("version");
    let body = toml::to_string_pretty(&local).map_err(|_| invalid_document())?;
    Ok(if body.is_empty() { format!("version = {CURRENT}\n") } else { format!("version = {CURRENT}\n\n{body}") })
}

pub fn date(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = era * 400 + year_of_era + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn conflict_name(name: &str, saved: u64) -> String {
    format!("{name} (conflict {})", date(saved))
}

#[cfg(test)]
#[path = "../test/sync.rs"]
mod tests;
