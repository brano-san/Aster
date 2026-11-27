use crate::app::{App, Drag, Effect, open_target};
use crate::i18n::t;
use crate::links::{Link, find_links};
use crate::notices::Toast;
use crate::panes::PaneId;

const KEYS: &[u8] = b"asdfghjklqwertyuiopzxcvbnm";
const MAX_TARGETS: usize = 180;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Url,
    Path,
    Hash,
}

pub(crate) struct Target {
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) label: String,
    text: String,
    kind: Kind,
}

pub(crate) struct QuickSelection {
    pub(crate) pane: PaneId,
    pub(crate) open: bool,
    pub(crate) prefix: String,
    pub(crate) targets: Vec<Target>,
    seq: u64,
}

fn is_url(text: &str) -> bool {
    text.split_once("://").is_some_and(|(scheme, _)| ["http", "https", "ftp"].iter().any(|s| s.eq_ignore_ascii_case(scheme)))
}

fn has_file_suffix(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(name, ext)| {
        !name.is_empty() && ext.len() <= 12
            && ext.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && ext.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

fn extra_objects(text: &str) -> Vec<(Link, Kind)> {
    let separator = |c: char| c.is_whitespace() || "<>\x22'`|()[]{},;".contains(c);
    let mut start = 0;
    let mut out = Vec::new();
    for token in text.split(separator) {
        let target = token.trim_end_matches(['.', '!', '?']);
        let count = target.chars().count();
        let path = open_target(target);
        let kind = if (7..=64).contains(&target.len()) && target.bytes().all(|b| b.is_ascii_hexdigit()) {
            Some(Kind::Hash)
        } else if !path.contains("://") && count > 2 && (path.contains(['/', '\\']) || has_file_suffix(&path)) {
            Some(Kind::Path)
        } else {
            None
        };
        if let Some(kind) = kind {
            out.push((Link { start, end: start + count, target: target.into() }, kind));
        }
        start += token.chars().count() + 1;
    }
    out
}

fn quoted_paths(text: &str) -> Vec<(Link, Kind)> {
    let chars: Vec<char> = text.chars().collect();
    let mut paths = Vec::new();
    let mut cursor = 0;
    while let Some(start) = (cursor..chars.len()).find(|&i| matches!(chars[i], '\'' | '"')) {
        let quote = chars[start];
        let Some(end) = (start + 1..chars.len()).find(|&i| chars[i] == quote) else {
            cursor = start + 1;
            continue;
        };
        cursor = end + 1;
        let mut target: String = chars[start + 1..end].iter().collect();
        let mut suffix = cursor;
        for _ in 0..2 {
            if chars.get(suffix) != Some(&':') { break; }
            let digits = chars[suffix + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            if digits == 0 { break; }
            suffix += digits + 1;
        }
        target.extend(&chars[cursor..suffix]);
        let path = open_target(&target);
        if !path.contains("://") && (path.contains(['/', '\\']) || has_file_suffix(&path)) {
            paths.push((Link { start: start + 1, end: suffix, target }, Kind::Path));
        }
        cursor = suffix;
    }
    paths
}

fn collect(terminal: &aster_vt::Terminal, open: bool) -> Vec<Target> {
    let mut targets = Vec::new();
    for line in terminal.visible_logical_lines() {
        let mut objects = quoted_paths(&line.text);
        for link in find_links(&line.text) {
            let kind = if is_url(&link.target) { Kind::Url } else { Kind::Path };
            if !objects.iter().any(|(existing, _)| link.start < existing.end && existing.start < link.end) {
                objects.push((link, kind));
            }
        }
        for (object, kind) in extra_objects(&line.text) {
            if !objects.iter().any(|(existing, _)| object.start < existing.end && existing.start < object.end) {
                objects.push((object, kind));
            }
        }
        objects.sort_unstable_by_key(|(link, _)| link.start);
        for (link, kind) in objects {
            if open && kind == Kind::Hash {
                continue;
            }
            if let Some(&(line, column)) = line.cells.get(link.start) {
                targets.push(Target { line, column, label: String::new(), text: link.target, kind });
            }
            if targets.len() == MAX_TARGETS {
                break;
            }
        }
        if targets.len() == MAX_TARGETS {
            break;
        }
    }
    for (line, column, text) in terminal.visible_hyperlinks() {
        if targets.len() == MAX_TARGETS { break; }
        let kind = if is_url(&text) { Kind::Url } else if text.starts_with("file://") { Kind::Path } else { continue };
        if !targets.iter().any(|target| (target.line, target.column) == (line, column)) {
            targets.push(Target { line, column, label: String::new(), text, kind });
        }
    }
    targets.sort_unstable_by_key(|target| (target.line, target.column));
    let multi = targets.len() > KEYS.len();
    for (index, item) in targets.iter_mut().enumerate() {
        if multi {
            item.label.push(KEYS[index / KEYS.len()] as char);
        }
        item.label.push(KEYS[index % KEYS.len()] as char);
    }
    targets
}

impl App {
    pub(crate) fn start_quick_selection(&mut self, open: bool) {
        let Some(pane) = self.focused_pane() else { return };
        let Some(terminal) = self.terminal(pane) else { return };
        let (targets, seq) = {
            let terminal = terminal.lock();
            (collect(&terminal, open), terminal.seq())
        };
        self.close_search();
        self.cancel_quick_selection();
        self.overlay = None;
        self.drag = Drag::None;
        self.dirty = true;
        if targets.is_empty() {
            let now = self.clock();
            self.tab_toast = Some(Toast::new(t("No selectable objects in this pane").to_owned(), now, crate::app::TOAST, false));
        } else {
            self.quick_selection = Some(QuickSelection { pane, open, prefix: String::new(), targets, seq });
        }
    }

    pub(crate) fn cancel_quick_selection(&mut self) {
        self.dirty |= self.quick_selection.take().is_some();
    }

    pub(crate) fn quick_selection_char(&mut self, ch: char) {
        let Some(selection) = &mut self.quick_selection else { return };
        selection.prefix.push(ch.to_ascii_lowercase());
        let index = selection.targets.iter().position(|target| target.label == selection.prefix);
        let Some(index) = index else {
            if !selection.targets.iter().any(|target| target.label.starts_with(&selection.prefix)) {
                selection.prefix.clear();
            }
            self.dirty = true;
            return;
        };
        let Some(mut selection) = self.quick_selection.take() else { return };
        let target = selection.targets.swap_remove(index);
        self.dirty = true;
        if self.focused_pane() != Some(selection.pane) {
            return;
        }
        let unchanged = self.terminal(selection.pane).is_some_and(|terminal| {
            let terminal = terminal.lock();
            terminal.seq() == selection.seq || collect(&terminal, selection.open).into_iter().any(|candidate| {
                candidate.line == target.line && candidate.column == target.column && candidate.text == target.text && candidate.kind == target.kind
            })
        });
        if !unchanged {
            let now = self.clock();
            self.tab_toast = Some(Toast::new(t("Output changed. Select again.").to_owned(), now, crate::app::TOAST, false));
            return;
        }
        if !selection.open {
            self.push(Effect::SetClipboard(target.text));
        } else if target.kind == Kind::Url {
            self.push(Effect::Open(target.text));
        } else {
            let Some(pane) = self.panes.get(&selection.pane).filter(|pane| pane.profile.is_local() && !pane.profile.is_wsl()) else {
                let now = self.clock();
                self.tab_toast = Some(Toast::new(t("This path belongs to another environment").to_owned(), now, crate::app::TOAST, false));
                return;
            };
            let terminal = pane.terminal.lock();
            let reported_cwd = terminal.reported_cwd().map(str::to_owned);
            let initial_cwd = terminal.cwd().map(str::to_owned).or_else(|| pane.profile.cwd.clone());
            let pid = pane.pid;
            drop(terminal);
            self.push(Effect::OpenOutputPath { target: target.text, reported_cwd, initial_cwd, pid });
        }
    }

    pub fn output_path_failed(&mut self) {
        let now = self.clock();
        self.tab_toast = Some(Toast::new(t("Could not open the selected local path").to_owned(), now, crate::app::TOAST, false));
        self.dirty = true;
    }
}

#[cfg(test)]
#[path = "../test/output_tools.rs"]
mod tests;
