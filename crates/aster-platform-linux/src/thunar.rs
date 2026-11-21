use crate::desktop;
use std::fs;
use std::io::{self, ErrorKind};
use std::ops::Range;
use std::path::Path;

const ID: &str = "com.brano-san.Aster.open";
const EMPTY: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions aster-created=\"true\"></actions>\n";
const SELF_CLOSING: &str = r#"action aster-empty-root="true""#;

fn invalid_xml() -> io::Error {
    io::Error::new(ErrorKind::InvalidData, "invalid custom actions XML")
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

pub fn action(executable: &str, label: &str, empty_root: bool) -> String {
    let name = xml(label);
    let command = xml(&format!("exec {} open %f", super::explorer::shell_quote(&executable.replace('%', "%%"))));
    let tag = if empty_root { SELF_CLOSING } else { "action" };
    format!("<{tag}>\n  <icon>aster</icon>\n  <name>{name}</name>\n  <unique-id>{ID}</unique-id>\n  <command>{command}</command>\n  <patterns>*</patterns>\n  <range>1</range>\n  <directories/>\n</action>")
}

fn tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    let mut brackets = 0usize;
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '[') => brackets += 1,
            (None, ']') => brackets = brackets.saturating_sub(1),
            (None, '>') if brackets == 0 => return Some(at),
            _ => {}
        }
    }
    None
}

#[derive(Default)]
struct Actions {
    open_end: usize,
    close: usize,
    empty_slash: Option<usize>,
    owned: Option<(Range<usize>, bool)>,
}

fn locate(text: &str) -> io::Result<Actions> {
    let mut doc = Actions::default();
    let mut stack: Vec<(&str, usize, usize, bool)> = Vec::new();
    let mut owned = false;
    let mut root = false;
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find('<') {
        let start = cursor + offset;
        let tail = &text[start + 1..];
        if let Some((prefix, end)) = [("!--", "-->"), ("![CDATA[", "]]>"), ("?", "?>")].into_iter().find(|(p, _)| tail.starts_with(p)) {
            cursor = start + 1 + prefix.len() + tail[prefix.len()..].find(end).ok_or_else(invalid_xml)? + end.len();
            continue;
        }
        let end = tag_end(tail).ok_or_else(invalid_xml)?;
        cursor = start + 2 + end;
        let tag = &tail[..end];
        if tag.starts_with("!DOCTYPE") {
            continue;
        }
        if let Some(end_tag) = tag.strip_prefix('/') {
            let (name, open_start, open_end, marker) = stack.pop().ok_or_else(invalid_xml)?;
            if name != end_tag.trim() {
                return Err(invalid_xml());
            }
            match (stack.len(), name) {
                (2, "unique-id") if stack[1].0 == "action" => owned = text[open_end..start].trim() == ID,
                (1, "action") => {
                    if owned && doc.owned.replace((open_start..cursor, marker)).is_some() {
                        return Err(invalid_xml());
                    }
                    owned = false;
                }
                (0, "actions") => doc.close = start,
                _ => {}
            }
            continue;
        }
        let tag = tag.trim_end();
        let empty = tag.ends_with('/');
        let name = tag.split(|c: char| c == '/' || c.is_whitespace()).next().filter(|s| !s.is_empty()).ok_or_else(invalid_xml)?;
        if stack.is_empty() {
            if root || name != "actions" {
                return Err(invalid_xml());
            }
            root = true;
            doc.open_end = cursor;
            if empty {
                doc.empty_slash = Some(start + tag.len());
            }
        }
        if !empty {
            stack.push((name, start, cursor, tag == SELF_CLOSING));
        }
    }
    if !root || !stack.is_empty() {
        return Err(invalid_xml());
    }
    Ok(doc)
}

pub fn edit(text: &str, change: Option<(&str, &str)>) -> io::Result<String> {
    let doc = locate(text)?;
    let mut text = text.to_owned();
    match (doc.owned, change) {
        (Some((range, marker)), Some((path, label))) => text.replace_range(range, &action(path, label, marker)),
        (Some((range, true)), None) if range.start == doc.open_end && range.end == doc.close && text[doc.close..].starts_with("</actions>") => {
            text.replace_range(doc.open_end - 1..doc.close + "</actions".len(), "/");
        }
        (Some((range, _)), None) => text.replace_range(range, ""),
        (None, Some((path, label))) => {
            if let Some(slash) = doc.empty_slash {
                text.replace_range(slash..doc.open_end - 1, &format!(">{}</actions", action(path, label, true)));
            } else {
                text.insert_str(doc.close, &action(path, label, false));
            }
        }
        (None, None) => {}
    }
    Ok(text)
}

pub fn set(config: &Path, change: Option<(&str, &str)>) -> io::Result<()> {
    let folder = config.join("Thunar");
    if !folder.is_dir() {
        return Ok(());
    }
    let path = folder.join("uca.xml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound && change.is_some() => EMPTY.to_owned(),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let updated = edit(&text, change)?;
    if updated == text {
        return Ok(());
    }
    if updated == EMPTY && change.is_none() {
        desktop::remove(&path)
    } else {
        desktop::write(&path, updated.as_bytes())
    }
}

#[cfg(test)]
#[path = "../test/thunar.rs"]
mod tests;
