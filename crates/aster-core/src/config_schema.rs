use toml::Value;
use crate::config::Config;

pub const CURRENT: u32 = 1;

pub fn version(value: &Value) -> Option<u32> {
    match value.get("version") {
        None => Some(0),
        Some(value) => value.as_integer().and_then(|n| u32::try_from(n).ok()),
    }
}

pub fn migrate(mut value: Value, from: u32) -> Value {
    for version in from..CURRENT {
        if version == 0 {
            if let Some(terminal) = value.get_mut("terminal").and_then(Value::as_table_mut) {
                for (old, current) in [
                    ("bracketedPaste", "bracketed-paste"),
                    ("trimWhitespaceOnPaste", "trim-whitespace-on-paste"),
                    ("pasteNewlinesAsSpaces", "paste-newlines-as-spaces"),
                ] {
                    if let Some(setting) = terminal.remove(old) {
                        terminal.entry(current).or_insert(setting);
                    }
                }
            }
        }
    }
    if from <= CURRENT {
        for (old_section, old_key, section, key) in [
            ("terminal", "environment", "terminal", "env"),
            ("appearance", "last-tab-closes-window", "terminal", "close-window-with-last-tab"),
            ("terminal", "focus-follows-mouse", "panes", "focus-follows-mouse"),
        ] {
            move_setting(&mut value, old_section, old_key, section, key);
        }
        if let Some(table) = value.as_table_mut() {
            table.insert("version".into(), Value::Integer(i64::from(CURRENT)));
        }
    }
    value
}

fn move_setting(value: &mut Value, old_section: &str, old_key: &str, section: &str, key: &str) {
    let Some(table) = value.as_table_mut() else { return };
    let Some(setting) = table.get_mut(old_section).and_then(Value::as_table_mut).and_then(|section| section.remove(old_key)) else { return };
    if let Some(section) = table.entry(section).or_insert_with(|| Value::Table(Default::default())).as_table_mut() {
        section.entry(key).or_insert(setting);
    }
}

pub fn decode_newer(source: Value) -> Config {
    if let Ok(config) = source.clone().try_into::<Config>() { return config; }
    let Value::Table(source) = source else { return Config::default() };
    let mut accepted = Value::Table(Default::default());
    for (key, value) in source {
        keep_compatible(&mut accepted, &mut vec![key], value);
    }
    accepted.try_into().unwrap_or_default()
}

fn keep_compatible(accepted: &mut Value, path: &mut Vec<String>, value: Value) {
    let mut candidate = accepted.clone();
    let mut slot = &mut candidate;
    for key in path.iter() {
        let Some(table) = slot.as_table_mut() else { return };
        slot = table.entry(key).or_insert_with(|| Value::Table(Default::default()));
    }
    *slot = value.clone();
    if candidate.clone().try_into::<Config>().is_ok() {
        *accepted = candidate;
    } else if let Value::Table(fields) = value {
        for (key, field) in fields {
            path.push(key);
            keep_compatible(accepted, path, field);
            path.pop();
        }
    }
}

#[derive(serde::Deserialize)]
struct VaultDocument {
    version: Option<toml::Spanned<Value>>,
    vault: Option<toml::Spanned<VaultFields>>,
}

#[derive(serde::Deserialize)]
struct VaultFields {
    sealed: Option<toml::Spanned<String>>,
}

pub fn replace_vault_seal(text: &str, sealed: &str) -> Option<String> {
    let source: VaultDocument = toml::from_str(text).ok()?;
    let mut expected: toml::Table = toml::from_str(text).ok()?;
    let field = Value::String(sealed.into());
    let literal = field.to_string();
    let vault = expected.entry("vault").or_insert_with(|| Value::Table(Default::default())).as_table_mut()?;
    let was_empty = vault.is_empty();
    vault.insert("sealed".into(), field);
    let accept = |candidate: String| (toml::from_str::<toml::Table>(&candidate).ok().as_ref() == Some(&expected)).then_some(candidate);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let insert = |at: usize, line: &str| accept(format!("{}{line}{}", text.get(..at).unwrap_or_default(), text.get(at..).unwrap_or_default()));

    if let Some(vault) = source.vault {
        if let Some(seal) = &vault.get_ref().sealed {
            let span = seal.span();
            return accept(format!("{}{literal}{}", text.get(..span.start)?, text.get(span.end..)?));
        }
        let span = vault.span();
        if let Some(inline) = text.get(span.clone()).filter(|s| s.starts_with('{') && s.ends_with('}')) {
            let comma = if was_empty { "" } else { ", " };
            if let Some(candidate) = insert(span.start + inline.len() - 1, &format!("{comma}sealed = {literal}")) { return Some(candidate); }
        }
        let mut at = 0;
        for line in text.split_inclusive('\n') {
            at += line.len();
            if !line.trim_start().starts_with('[') { continue; }
            let header = toml::from_str::<toml::Table>(line).ok();
            if header.as_ref().and_then(|h| h.get("vault")).and_then(Value::as_table).is_some_and(toml::Table::is_empty) {
                let newline = if line.ends_with('\n') { "" } else { eol };
                if let Some(candidate) = insert(at, &format!("{newline}sealed = {literal}{eol}")) { return Some(candidate); }
            }
        }
    } else {
        return accept(format!("{text}{eol}{eol}[vault]{eol}sealed = {literal}{eol}"));
    }
    let at = source.version.map_or(0, |version| {
        let end = version.span().end;
        text[end..].find('\n').map_or(text.len(), |offset| end + offset + 1)
    });
    let newline = if at == 0 || text.as_bytes().get(at - 1) == Some(&b'\n') { "" } else { eol };
    insert(at, &format!("{newline}vault.sealed = {literal}{eol}"))
}
