use crate::fuzzy::fuzzy;
use crate::i18n::{t, tf};
use crate::panes::PaneId;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayKind {
    Commands,
    Profiles,
    ProfileTemplates,
    Schemes,
    TabColors,
    SerialSpeed(PaneId),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pick {
    pub label: String,
    pub hint: String,
    pub value: String,
    pub group: String,
    pub rank: u32,
    pub badge: String,
    pub tint: Option<u32>,
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
pub const MANAGE: &str = "manage:";
pub const CLEAR_RECENT: &str = "clear-recent:";

pub const VISIBLE: usize = 12;

const SERIAL_SPEEDS: &[u32] = &[110, 300, 1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600, 1500000];

fn speed_pick(baud: u32) -> Pick {
    let value = baud.to_string();
    Pick { label: value.clone(), value, ..Pick::default() }
}

fn quick_pick(label: String, kind: &str, value: String) -> Pick {
    Pick { label, hint: kind.to_owned(), value, group: t("Connect").to_owned(), badge: "\u{2192}".into(), ..Pick::default() }
}

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

    pub fn serial_speed(pane: PaneId) -> Self {
        Self::new(OverlayKind::SerialSpeed(pane), SERIAL_SPEEDS.iter().copied().map(speed_pick).collect())
    }

    pub fn placeholder(&self) -> &'static str {
        match self.kind {
            OverlayKind::Commands => t("Type a command"),
            OverlayKind::Profiles => t("Select a profile or enter an address"),
            OverlayKind::ProfileTemplates => t("Select a profile template"),
            OverlayKind::Schemes => t("Select a color scheme"),
            OverlayKind::TabColors => t("Select a tab color"),
            OverlayKind::SerialSpeed(_) => t("Select or enter the serial port speed"),
        }
    }

    fn refilter(&mut self) {
        self.items.truncate(self.base);
        if matches!(self.kind, OverlayKind::SerialSpeed(_)) {
            self.filter_serial_speeds();
            return;
        }
        let commands = matches!(self.kind, OverlayKind::Commands);
        let profiles = matches!(self.kind, OverlayKind::Profiles);
        let english = |p: &Pick| commands.then(|| crate::keymap::Action::from_id(&p.value)).flatten().map(|a| a.source_label()).filter(|e| *e != p.label);
        let extra = |p: &Pick| profiles.then(|| fuzzy(&self.query, &p.hint).or_else(|| fuzzy(&self.query, &p.group))).flatten().map(|m| m.score - 20);
        let mut scored: Vec<(i32, usize, Vec<Range<usize>>)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                fuzzy(&self.query, &p.label)
                    .map(|m| (m.score, i, m.ranges))
                    .or_else(|| english(p).and_then(|e| fuzzy(&self.query, &e)).map(|m| (m.score, i, Vec::new())))
                    .or_else(|| extra(p).map(|score| (score, i, Vec::new())))
            })
            .collect();
        if !self.query.trim().is_empty() {
            let items = &self.items;
            scored.retain(|(_, i, _)| !items[*i].value.starts_with(CLEAR_RECENT));
            scored.sort_by(|a, b| items[a.1].rank.cmp(&items[b.1].rank).then(b.0.cmp(&a.0)).then(a.1.cmp(&b.1)));
        }
        self.shown = scored.into_iter().map(|(_, i, r)| (i, r)).collect();
        if let Some(address) = profiles.then(|| aster_proto::address::parse_url(&self.query)).flatten() {
            let kind = match address.protocol {
                aster_proto::address::Protocol::Telnet => "Telnet",
                aster_proto::address::Protocol::Raw => "TCP",
            };
            self.items.push(quick_pick(tf("Connect to {target}", &[("target", &address.display())]), kind, format!("{QUICK}{}", address.display())));
            self.shown.insert(0, (self.items.len() - 1, Vec::new()));
        } else if let Some(target) = self.quick.then(|| crate::ssh::parse_target(&self.query)).flatten() {
            self.items.push(quick_pick(tf("Connect to {target}", &[("target", &target.display())]), "SSH", format!("{QUICK}{}", self.query.trim())));
            let at = if self.query.contains('@') || self.shown.is_empty() { 0 } else { self.shown.len() };
            self.shown.insert(at, (self.items.len() - 1, Vec::new()));
        }
        self.selected = 0;
        self.scroll = 0;
    }

    fn filter_serial_speeds(&mut self) {
        self.shown.clear();
        self.selected = 0;
        self.scroll = 0;
        let query = self.query.trim();
        if query.is_empty() {
            self.shown.extend((0..self.items.len()).map(|i| (i, Vec::new())));
            self.selected = self.items.iter().position(|p| p.value == "115200").unwrap_or(0);
        } else if query.bytes().all(|b| b.is_ascii_digit()) {
            let Ok(baud @ 50..=4_000_000) = query.parse::<u32>() else { return };
            self.shown.push((self.items.len(), Vec::new()));
            self.items.push(speed_pick(baud));
        }
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

    pub fn selected_pick(&self) -> Option<&Pick> {
        self.shown.get(self.selected).map(|(i, _)| &self.items[*i])
    }

    pub fn header_before(&self, position: usize) -> Option<&str> {
        let (item, _) = self.shown.get(position)?;
        let group = self.items[*item].group.as_str();
        if group.is_empty() {
            return None;
        }
        let previous = position.checked_sub(1).and_then(|p| self.shown.get(p)).map(|(i, _)| self.items[*i].group.as_str());
        (position == self.scroll || previous != Some(group)).then_some(group)
    }

    pub fn query_text(&self) -> &str {
        &self.query
    }

    pub fn set_query(&mut self, text: &str) {
        self.query = text.to_owned();
        self.refilter();
    }
}

#[cfg(test)]
#[path = "../test/overlay.rs"]
mod tests;
