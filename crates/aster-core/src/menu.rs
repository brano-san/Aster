use crate::app::App;
use crate::input::{CursorIcon, Key};
use crate::keymap::Action;
use aster_gfx::render::Rect;

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Item { label: &'static str, action: Action, enabled: bool },
    Separator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub entries: Vec<Entry>,
    pub at: (f32, f32),
    pub selected: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuGeometry {
    pub frame: Rect,
    pub rows: Vec<(usize, Rect)>,
    pub separators: Vec<Rect>,
}

const WIDTH: f32 = 260.0;
const ITEM_H: f32 = 30.0;
const SEPARATOR_H: f32 = 9.0;
const PAD: f32 = 5.0;

fn item(label: &'static str, action: Action, enabled: bool) -> Entry {
    Entry::Item { label, action, enabled }
}

impl Menu {
    fn enabled(&self, i: usize) -> bool {
        matches!(self.entries.get(i), Some(Entry::Item { enabled: true, .. }))
    }

    fn step(&mut self, delta: isize) {
        let n = self.entries.len() as isize;
        if n == 0 || !(0..n as usize).any(|i| self.enabled(i)) {
            return;
        }
        let mut i = match self.selected {
            Some(s) => s as isize,
            None if delta > 0 => -1,
            None => n,
        };
        loop {
            i = (i + delta).rem_euclid(n);
            if self.enabled(i as usize) {
                self.selected = Some(i as usize);
                return;
            }
        }
    }

    pub fn action(&self, i: usize) -> Option<Action> {
        match self.entries.get(i) {
            Some(Entry::Item { action, enabled: true, .. }) => Some(*action),
            _ => None,
        }
    }
}

pub fn geometry(menu: &Menu, s: f32, bounds: (f32, f32)) -> MenuGeometry {
    let w = (WIDTH * s).round();
    let pad = (PAD * s).round();
    let item_h = (ITEM_H * s).round();
    let sep_h = (SEPARATOR_H * s).round();
    let h = 2.0 * pad + menu.entries.iter().map(|e| if matches!(e, Entry::Separator) { sep_h } else { item_h }).sum::<f32>();
    let x = menu.at.0.min(bounds.0 - w - 4.0 * s).max(0.0).round();
    let y = if menu.at.1 + h > bounds.1 { (menu.at.1 - h).max(0.0) } else { menu.at.1 }.round();
    let frame = Rect::new(x, y, w, h);
    let mut rows = Vec::new();
    let mut separators = Vec::new();
    let mut top = y + pad;
    for (i, e) in menu.entries.iter().enumerate() {
        match e {
            Entry::Separator => {
                separators.push(Rect::new(x + pad, top + (sep_h / 2.0).floor(), w - 2.0 * pad, (1.0 * s).round().max(1.0)));
                top += sep_h;
            }
            Entry::Item { .. } => {
                rows.push((i, Rect::new(x + pad, top, w - 2.0 * pad, item_h)));
                top += item_h;
            }
        }
    }
    MenuGeometry { frame, rows, separators }
}

impl App {
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn menu_geometry(&self) -> Option<MenuGeometry> {
        self.menu.as_ref().map(|m| geometry(m, self.scale, self.size))
    }

    fn open_menu(&mut self, entries: Vec<Entry>, x: f32, y: f32) {
        self.overlay = None;
        self.commit_rename();
        self.menu = Some(Menu { entries, at: (x, y), selected: None });
        self.set_cursor(CursorIcon::Default);
        self.dirty = true;
    }

    pub(crate) fn open_tab_menu(&mut self, index: usize, x: f32, y: f32) {
        self.activate(index);
        let n = self.tabs.len();
        let entries = vec![
            item("New tab", Action::NewTab, true),
            item("Duplicate", Action::DuplicateTab, true),
            item("Duplicate as administrator", Action::DuplicateAsAdmin, self.can_elevate() && self.focused_pane().and_then(|p| self.panes.get(&p)).is_some_and(|p| p.profile.is_local())),
            item("Rename", Action::RenameTab, true),
            item("Color", Action::SetTabColor, true),
            Entry::Separator,
            item("Move left", Action::MoveTabLeft, index > 0),
            item("Move right", Action::MoveTabRight, index + 1 < n),
            item("Panes to separate tabs", Action::ExplodeTab, self.tabs.get(index).is_some_and(|t| t.root.panes().len() > 1)),
            item("Combine all tabs", Action::CombineTabs, n > 1),
            Entry::Separator,
            item("Close", Action::CloseTab, true),
            item("Close other tabs", Action::CloseOtherTabs, n > 1),
            item("Close tabs to the right", Action::CloseTabsToRight, index + 1 < n),
            item("Close tabs to the left", Action::CloseTabsToLeft, index > 0),
            item("Reopen closed tab", Action::ReopenTab, !self.closed_tabs.is_empty()),
        ];
        self.open_menu(entries, x, y);
    }

    pub(crate) fn open_terminal_menu(&mut self, x: f32, y: f32) {
        let selection = self.focused_terminal().is_some_and(|t| t.lock().has_selection());
        let entries = vec![
            item("Copy", Action::Copy, selection),
            item("Paste", Action::Paste, true),
            item("Select all", Action::SelectAll, true),
            item("Clear", Action::Clear, true),
            Entry::Separator,
            item("Split to the right", Action::SplitRight, true),
            item("Split to the bottom", Action::SplitBottom, true),
            item("Close pane", Action::ClosePane, true),
            Entry::Separator,
            item("Search", Action::Search, true),
            item("Remote files", Action::OpenFiles, self.focused_pane().is_some_and(|p| self.can_browse(p))),
            item("Settings", Action::Settings, true),
        ];
        self.open_menu(entries, x, y);
    }

    pub(crate) fn close_menu(&mut self) {
        if self.menu.take().is_some() {
            self.dirty = true;
        }
    }

    fn run_menu(&mut self, i: usize) {
        let Some(action) = self.menu.as_ref().and_then(|m| m.action(i)) else { return };
        self.close_menu();
        self.run(action);
    }

    pub(crate) fn menu_key(&mut self, key: Key) {
        let Some(menu) = self.menu.as_mut() else { return };
        self.dirty = true;
        match key {
            Key::Escape => self.close_menu(),
            Key::Up => menu.step(-1),
            Key::Down | Key::Tab => menu.step(1),
            Key::Home => {
                menu.selected = None;
                menu.step(1);
            }
            Key::End => {
                menu.selected = None;
                menu.step(-1);
            }
            Key::Enter | Key::Space => {
                if let Some(i) = menu.selected {
                    self.run_menu(i);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn menu_hover(&mut self, x: f32, y: f32) {
        let Some(geo) = self.menu_geometry() else { return };
        let Some(menu) = self.menu.as_mut() else { return };
        let hovered = geo.rows.iter().find(|(i, r)| r.contains(x, y) && menu.enabled(*i)).map(|(i, _)| *i);
        if (hovered.is_some() || !geo.frame.contains(x, y)) && menu.selected != hovered {
            menu.selected = hovered;
            self.dirty = true;
        }
    }

    pub(crate) fn menu_click(&mut self, x: f32, y: f32) {
        let Some(geo) = self.menu_geometry() else { return };
        if !geo.frame.contains(x, y) {
            return self.close_menu();
        }
        if let Some((i, _)) = geo.rows.iter().find(|(_, r)| r.contains(x, y)) {
            self.run_menu(*i);
        }
    }
}

#[cfg(test)]
#[path = "../test/menu.rs"]
mod tests;
