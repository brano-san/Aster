use crate::app::{App, Effect, TOAST};
use crate::config::CustomScheme;
use crate::keymap::{Action as KeyAction, Keymap};
use crate::menu::Entry;
use crate::picker::ProfileAction;
use aster_ext::{Manifest, ScanError};
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Stopped,
    Starting,
    Running,
    Failed(String),
}

#[derive(Clone, Debug, Deserialize)]
pub struct Profile {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub icon: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Installed {
    pub manifest: Manifest,
    pub directory: PathBuf,
    pub status: Status,
    pub schemes: Vec<CustomScheme>,
    pub profiles: Vec<Profile>,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub installed: Vec<Installed>,
    pub errors: Vec<ScanError>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub url: String,
}

#[derive(Clone, Debug)]
pub enum Request {
    Run { extension: String, command: String, title: String, cwd: Option<String>, cwd_pid: Option<u32> },
    Catalog(String),
    InstallFile(PathBuf),
    InstallCatalog(CatalogEntry),
    Remove(String),
    OpenFolder(Option<String>),
}

#[derive(Debug)]
pub enum Reply {
    Catalog { url: String, result: Result<Vec<CatalogEntry>, String> },
    Operation(Result<(), String>),
}

pub fn https_url(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        let host = rest.split('/').next().unwrap_or_default();
        !host.is_empty() && !host.contains(['@', '?', '#'])
            && !rest.contains('\\') && !rest.chars().any(|c| c.is_whitespace() || c.is_control())
    })
}

#[derive(Debug)]
pub enum Action {
    Write(String),
    Notify(String),
    Clipboard(String),
}

impl App {
    pub fn set_extensions(&mut self, snapshot: Arc<Snapshot>) {
        self.extensions = snapshot;
        self.refresh_extension_contributions();
    }

    pub(crate) fn extension_entries(&self) -> impl Iterator<Item = &Installed> {
        self.extensions.installed.iter().filter(|entry| !self.config.extensions.disabled.contains(&entry.manifest.id))
    }

    fn extension_commands(&self) -> impl Iterator<Item = (&Manifest, &aster_ext::Command)> {
        self.extension_entries().filter(|entry| entry.status == Status::Running)
            .flat_map(|entry| entry.manifest.commands.iter().map(|command| (&entry.manifest, command)))
    }

    pub(crate) fn extension_keymap(&self) -> Keymap {
        let commands = self.extension_commands().map(|(manifest, command)| (key_action(manifest, command), command.shortcut.as_deref())).collect();
        Keymap::with_extensions(&self.config.hotkeys, self.config.appearance.mru_tabs, commands)
    }

    pub(crate) fn dynamic_actions(&self) -> Vec<ProfileAction> {
        let mut actions = self.profile_actions();
        actions.extend(self.extension_commands().map(|(manifest, command)| ProfileAction {
            action: key_action(manifest, command), name: Some(command_label(manifest, command)),
        }));
        actions
    }

    pub(crate) fn append_extension_menu(&self, entries: &mut Vec<Entry>, menu: aster_ext::Menu) {
        let mut commands = self.extension_commands().filter(|(_, command)| command.menu == Some(menu)).peekable();
        if commands.peek().is_some() { entries.push(Entry::Separator); }
        entries.extend(commands.map(|(manifest, command)| Entry::Item {
            label: Cow::Owned(command_label(manifest, command)), action: key_action(manifest, command), enabled: true,
        }));
    }

    pub(crate) fn run_extension(&mut self, extension: String, command: String) {
        if !self.extension_commands().any(|(manifest, item)| manifest.id == extension && item.id == command) {
            return;
        }
        let (title, cwd, cwd_pid) = self.focused_pane().and_then(|id| self.panes.get(&id)).map(|pane| {
            let cwd = pane.terminal.lock().cwd().map(str::to_owned);
            let local = pane.profile.is_local() && !pane.profile.id.starts_with("ssh:") && !pane.profile.is_wsl();
            let cwd_pid = if cwd.is_none() && local { pane.pid } else { None };
            let title = pane.name.as_ref().or(pane.title.as_ref()).unwrap_or(&pane.profile.name).clone();
            (title, cwd, cwd_pid)
        }).unwrap_or_default();
        self.effects.push(Effect::Extension(Request::Run { extension, command, title, cwd, cwd_pid }));
    }

    pub(crate) fn extension_profiles(&self) -> Vec<crate::profile::Profile> {
        let mut seen = HashSet::new();
        self.extension_entries().flat_map(|entry| entry.profiles.iter().map(move |profile| {
            let id = format!("extension:{}:{}", entry.manifest.id, profile.name);
            let mut result = crate::profile::Profile::new(&id, &profile.name, &profile.command, &[]);
            result.args.clone_from(&profile.args);
            result.cwd.clone_from(&profile.cwd);
            result.icon.clone_from(&profile.icon);
            result.builtin = true;
            result
        })).filter(|profile| seen.insert(profile.id.clone())).collect()
    }

    pub(crate) fn extension_profile_group(&self, id: &str) -> Option<&str> {
        let (extension, _) = id.strip_prefix("extension:")?.split_once(':')?;
        self.extension_entries().find(|entry| entry.manifest.id == extension).map(|entry| entry.manifest.name.as_str())
    }

    pub(crate) fn extension_schemes(&self, base: impl Iterator<Item = String>) -> Vec<(aster_vt::schemes::Scheme, String)> {
        let mut names: HashSet<_> = base.map(|name| name.to_ascii_lowercase()).collect();
        self.extension_entries().flat_map(|entry| entry.schemes.iter().filter_map(move |scheme| Some((entry, scheme, scheme.palette()?))))
            .map(|(entry, scheme, palette)| {
                let mut name = scheme.name.clone();
                if !names.insert(name.to_ascii_lowercase()) {
                    let qualified = format!("{} ({})", scheme.name, entry.manifest.id);
                    name = qualified.clone();
                    let mut counter = 2;
                    while !names.insert(name.to_ascii_lowercase()) {
                        name = format!("{qualified} {counter}");
                        counter += 1;
                    }
                }
                (aster_vt::schemes::Scheme { name, palette }, entry.manifest.name.clone())
            }).collect()
    }

    pub fn extension_action(&mut self, action: Action) {
        match action {
            Action::Write(text) => {
                if let Some(pane) = self.focused_pane().filter(|pane| self.panes.get(pane).is_some_and(|pane| pane.exited.is_none())) {
                    self.write(pane, text.into_bytes());
                }
            }
            Action::Notify(text) => self.toast(text, TOAST),
            Action::Clipboard(text) => self.effects.push(Effect::SetClipboard(text)),
        }
    }
}

fn key_action(manifest: &Manifest, command: &aster_ext::Command) -> KeyAction {
    KeyAction::Extension { extension: manifest.id.clone(), command: command.id.clone() }
}

fn command_label(manifest: &Manifest, command: &aster_ext::Command) -> String {
    format!("{}: {}", manifest.name, command.title)
}

#[cfg(test)]
#[path = "../test/extensions.rs"]
mod tests;
