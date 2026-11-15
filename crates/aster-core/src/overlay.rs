use crate::fuzzy::fuzzy;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayKind {
    Commands,
    Profiles,
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
}

pub const VISIBLE: usize = 12;

impl Overlay {
    pub fn new(kind: OverlayKind, items: Vec<Pick>) -> Self {
        let mut o = Self { kind, items, query: String::new(), shown: Vec::new(), selected: 0, scroll: 0 };
        o.refilter();
        o
    }

    pub fn placeholder(&self) -> &'static str {
        match self.kind {
            OverlayKind::Commands => "Type a command",
            OverlayKind::Profiles => "Select a profile",
        }
    }

    fn refilter(&mut self) {
        let mut scored: Vec<(i32, usize, Vec<Range<usize>>)> = self.items.iter().enumerate().filter_map(|(i, p)| fuzzy(&self.query, &p.label).map(|m| (m.score, i, m.ranges))).collect();
        if !self.query.trim().is_empty() {
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        }
        self.shown = scored.into_iter().map(|(_, i, r)| (i, r)).collect();
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

    pub fn selected_value(&self) -> Option<String> {
        self.shown.get(self.selected).map(|(i, _)| self.items[*i].value.clone())
    }
}

#[cfg(test)]
#[path = "../test/overlay.rs"]
mod tests;
