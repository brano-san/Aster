use crate::fuzzy::fuzzy;
use crate::i18n::{t, tf};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayKind {
    Commands,
    Profiles,
    Schemes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub label: String,
    pub hint: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct Overlay {
    pub kind: OverlayKind,
    pub items: Vec<Pick>,
    pub query: String,
    pub shown: Vec<(usize, Vec<Range<usize>>)>,
    pub selected: usize,
    pub scroll: usize,
    pub quick: bool,
    base: usize,
}

pub const QUICK: &str = "quick:";

pub const VISIBLE: usize = 12;

impl Overlay {
    pub fn new(kind: OverlayKind, items: Vec<Pick>) -> Self {
        let base = items.len();
        let mut o = Self { kind, items, query: String::new(), shown: Vec::new(), selected: 0, scroll: 0, quick: false, base };
        o.refilter();
        o
    }

    pub fn with_quick_connect(mut self) -> Self {
        self.quick = true;
        self.refilter();
        self
    }

    pub fn placeholder(&self) -> &'static str {
        match self.kind {
            OverlayKind::Commands => t("Type a command"),
            OverlayKind::Profiles => t("Select a profile"),
            OverlayKind::Schemes => t("Select a color scheme"),
        }
    }

    fn refilter(&mut self) {
        self.items.truncate(self.base);
        let mut scored: Vec<(i32, usize, Vec<Range<usize>>)> = self.items.iter().enumerate().filter_map(|(i, p)| fuzzy(&self.query, &p.label).map(|m| (m.score, i, m.ranges))).collect();
        if !self.query.trim().is_empty() {
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        }
        self.shown = scored.into_iter().map(|(_, i, r)| (i, r)).collect();
        if let Some(target) = self.quick.then(|| crate::ssh::parse_target(&self.query)).flatten() {
            self.items.push(Pick { label: tf("Connect to {target}", &[("target", &target.display())]), hint: "SSH".into(), value: format!("{QUICK}{}", self.query.trim()) });
            let at = if self.query.contains('@') || self.shown.is_empty() { 0 } else { self.shown.len() };
            self.shown.insert(at, (self.items.len() - 1, Vec::new()));
        }
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn insert(&mut self, text: &str) {
        self.query.push_str(text);
        self.refilter();
    }

    pub fn backspace(&mut self, word: bool) {
        if word {
            let trimmed = self.query.trim_end();
            let cut = trimmed.rfind(char::is_whitespace).map_or(0, |i| i + 1);
            self.query.truncate(cut);
        } else {
            self.query.pop();
        }
        self.refilter();
    }

    pub fn step(&mut self, delta: isize) {
        if self.shown.is_empty() {
            return;
        }
        let n = self.shown.len() as isize;
        let next = self.selected as isize + delta;
        self.selected = if delta.abs() == 1 { next.rem_euclid(n) } else { next.clamp(0, n - 1) } as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + VISIBLE {
            self.scroll = self.selected + 1 - VISIBLE;
        }
    }

    pub fn select(&mut self, index: usize) {
        if index < self.shown.len() {
            self.selected = index;
            self.scroll = index.saturating_sub(VISIBLE / 2).min(self.shown.len().saturating_sub(VISIBLE));
        }
    }

    pub fn selected_value(&self) -> Option<String> {
        self.shown.get(self.selected).map(|(i, _)| self.items[*i].value.clone())
    }
}

#[cfg(test)]
#[path = "../test/overlay.rs"]
mod tests;
