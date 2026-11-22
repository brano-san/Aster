use serde::Deserialize;
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub commands: Vec<Command>,
    #[serde(default)]
    pub schemes: Vec<SchemeFile>,
    #[serde(default)]
    pub profiles: Profiles,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Command {
    pub id: String,
    pub title: String,
    pub shortcut: Option<String>,
    pub menu: Option<Menu>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Menu {
    Terminal,
    Tab,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct SchemeFile {
    pub file: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Profiles {
    #[serde(default)]
    pub provider: bool,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let manifest: Self = toml::from_str(text).map_err(|error| error.to_string())?;
        if !valid_id(&manifest.id) {
            return Err(format!("Invalid extension id: {}", manifest.id));
        }
        if manifest.name.trim().is_empty() || manifest.version.trim().is_empty() {
            return Err("Extension name and version must not be empty".into());
        }
        if let Some(command) = &manifest.command {
            relative_path(command)?;
        }
        let prefix = format!("{}.", manifest.id);
        let mut commands = HashSet::new();
        for command in &manifest.commands {
            if !command.id.strip_prefix(&prefix).is_some_and(|suffix| !suffix.trim().is_empty())
                || command.title.trim().is_empty() || !commands.insert(&command.id) {
                return Err(format!("Invalid or repeated command id or title: {}", command.id));
            }
        }
        for scheme in &manifest.schemes {
            relative_path(&scheme.file)?;
        }
        Ok(manifest)
    }
}

pub fn valid_id(id: &str) -> bool {
    id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'.'))
        && valid_component(id)
}

pub fn relative_path(path: &str) -> Result<(), String> {
    if path.split(['/', '\\']).all(valid_component) {
        Ok(())
    } else {
        Err(format!("Invalid extension path: {path}"))
    }
}

fn valid_component(part: &str) -> bool {
    if part.is_empty() || part.ends_with(['.', ' ']) || part.chars().any(|c| c.is_control() || "<>:\"|?*".contains(c)) {
        return false;
    }
    let base = part.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !base.strip_prefix("COM").or_else(|| base.strip_prefix("LPT"))
            .is_some_and(|number| matches!(number, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
}

#[cfg(test)]
#[path = "../test/manifest.rs"]
mod tests;
