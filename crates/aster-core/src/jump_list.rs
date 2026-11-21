use crate::app::App;
use crate::config::Config;
use crate::i18n::t;
use crate::profile::Profile;
use std::collections::HashSet;

const LIMIT: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub title: &'static str,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JumpList {
    pub categories: [Category; 2],
    pub task: Item,
}

fn folder_key(path: &str) -> Option<String> {
    let absolute = matches!(path.as_bytes(), [drive, b':', b'\\' | b'/', ..] if drive.is_ascii_alphabetic())
        || path.starts_with(r"\\") || path.starts_with("//");
    (absolute && !path.chars().any(char::is_control))
        .then(|| path.replace('/', "\\").trim_end_matches('\\').to_ascii_lowercase())
}

pub fn remember_folder(folders: &mut Vec<String>, path: &str) -> bool {
    let Some(key) = folder_key(path) else { return false };
    if folders.first().and_then(|folder| folder_key(folder)).as_ref() == Some(&key) {
        return false;
    }
    folders.retain(|folder| folder_key(folder).as_ref() != Some(&key));
    folders.insert(0, path.to_owned());
    folders.truncate(LIMIT);
    true
}

pub fn build(config: &Config, detected: &[Profile], recent: &[String], folders: &[String]) -> JumpList {
    let visible = |p: &&Profile| !p.id.trim().is_empty() && !p.id.chars().any(char::is_control) && !config.profile_hidden(p);
    let mut ids = HashSet::new();
    let profiles: Vec<_> = recent.iter().filter_map(|id| {
        detected.iter().chain(&config.profiles).find(|profile| &profile.id == id).filter(visible)
    }).filter(|p| ids.insert(&p.id)).take(config.terminal.recent_profiles.min(LIMIT)).collect();
    let mut listed = HashSet::new();
    let profiles = profiles.into_iter().chain(config.profiles.iter().filter(visible))
        .filter(|p| listed.insert(&p.id)).take(LIMIT)
        .map(|p| Item { title: p.name.clone(), args: vec!["profile".to_owned(), p.id.clone()] }).collect();
    let mut paths = HashSet::new();
    let folders = folders.iter().filter(|dir| folder_key(dir).is_some_and(|key| paths.insert(key))).take(LIMIT)
        .map(|dir| Item { title: dir.clone(), args: vec!["open".to_owned(), dir.clone()] }).collect();
    JumpList {
        categories: [Category { title: t("Profiles"), items: profiles }, Category { title: t("Recent folders"), items: folders }],
        task: Item { title: t("New window").to_owned(), args: vec!["--new-window".to_owned()] },
    }
}

impl App {
    pub fn reported_local_cwd(&self) -> Option<String> {
        let pane = self.panes.get(&self.focused_pane()?)?;
        if !pane.profile.is_local() || pane.profile.is_wsl() || pane.profile.id.starts_with("ssh:") {
            return None;
        }
        pane.terminal.lock().cwd().map(str::to_owned)
    }
}

#[cfg(test)]
#[path = "../test/jump_list.rs"]
mod tests;
