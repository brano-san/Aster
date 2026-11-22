use crate::app::{App, Drag, Effect, SETTINGS_PANE};
use crate::input::{CursorIcon, Key, Mods, MouseButton};
use crate::keymap::Action;
use crate::panes::Side;
use crate::profile::ProfileGroup;
use crate::profiles_page::{self, Header, TreeKind, TreeRow};
use crate::settings::ProfileRow;
use aster_gfx::render::Rect;
use std::collections::BTreeSet;

pub(crate) const MIN_WIDTH: f32 = 144.0;
pub(crate) const MAX_WIDTH: f32 = 480.0;
pub(crate) const DEFAULT_WIDTH: f32 = 240.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Focus {
    #[default]
    None,
    Filter,
    Tree,
}

#[derive(Default)]
pub(crate) struct ProfileSidebar {
    pub query: String,
    pub collapsed: BTreeSet<Header>,
    pub profiles: Vec<ProfileRow>,
    pub rows: Vec<TreeRow>,
    pub selected: usize,
    pub scroll: usize,
    pub focus: Focus,
}

pub(crate) struct Geometry {
    pub rect: Rect,
    pub filter: Rect,
    pub list: Rect,
    pub grip: Rect,
    pub row_height: f32,
}

impl Geometry {
    pub fn visible(&self) -> usize {
        (self.list.h / self.row_height).floor() as usize
    }

    pub fn row(&self, position: usize) -> Rect {
        Rect::new(self.list.x, self.list.y + position as f32 * self.row_height, self.list.w, self.row_height)
    }

    pub fn hit_row(&self, x: f32, y: f32) -> Option<usize> {
        let index = ((y - self.list.y) / self.row_height) as usize;
        (self.list.contains(x, y) && index < self.visible()).then_some(index)
    }
}

impl ProfileSidebar {
    fn refilter(&mut self, groups: &[ProfileGroup]) {
        self.rows = profiles_page::tree(groups, &self.profiles, &self.collapsed, &self.query);
        self.rows.retain(|r| matches!(r.kind, TreeKind::Header(_) | TreeKind::Profile(_)));
        if let Some(index) = self.rows.iter().position(|row| row.kind == TreeKind::Header(Header::Ungrouped)) {
            if self.rows.get(index + 1).is_none_or(|row| row.depth == 0) && !self.collapsed.contains(&Header::Ungrouped) {
                self.rows.remove(index);
            }
        }
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }

    fn reveal(&mut self, visible: usize) {
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(visible));
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if visible > 0 && self.selected >= self.scroll + visible {
            self.scroll = self.selected + 1 - visible;
        }
    }
}

pub(crate) fn width(value: f32) -> f32 {
    if value.is_nan() { DEFAULT_WIDTH } else { value.clamp(MIN_WIDTH, MAX_WIDTH) }
}

impl App {
    pub(crate) fn place_profile_sidebar(&self, chrome: &mut crate::tabs::Chrome) {
        if !self.config.appearance.profile_sidebar || self.files.is_some() || self.focused_pane() == Some(SETTINGS_PANE) {
            return;
        }
        let r = chrome.content;
        let logical = match self.drag {
            Drag::SidebarResize { width } => width,
            _ => self.config.appearance.profile_sidebar_width,
        };
        let w = (width(logical) * self.ui()).round().min((r.w * 0.5).floor());
        if w < 76.0 * self.ui() || r.h < 54.0 * self.ui() {
            return;
        }
        chrome.profile_sidebar = Rect::new(r.x, r.y, w, r.h);
        chrome.content = Rect::new(r.x + w, r.y, r.w - w, r.h);
    }

    pub(crate) fn sidebar_geometry(&self) -> Option<Geometry> {
        let rect = self.chrome.profile_sidebar;
        if rect.w <= 0.0 || !self.config.appearance.profile_sidebar || self.files.is_some() || self.focused_pane() == Some(SETTINGS_PANE) {
            return None;
        }
        let s = self.ui();
        let pad = (8.0 * s).round();
        let filter = Rect::new(rect.x + pad, rect.y + pad, rect.w - 2.0 * pad, (30.0 * s).round());
        let list_y = filter.bottom() + (6.0 * s).round();
        let list = Rect::new(rect.x + (4.0 * s).round(), list_y, rect.w - pad, (rect.bottom() - list_y - pad).max(0.0));
        let grip_w = (5.0 * s).round();
        let grip = Rect::new(rect.right() - grip_w, rect.y, grip_w, rect.h);
        Some(Geometry { rect, filter, list, grip, row_height: (26.0 * s).round().max(1.0) })
    }

    pub(crate) fn sync_profile_sidebar(&mut self) {
        if !self.config.appearance.profile_sidebar {
            self.profile_sidebar.focus = Focus::None;
            return;
        }
        let profiles = self.profile_rows().into_iter().filter(|row| !row.hidden).collect();
        self.profile_sidebar.profiles = profiles;
        self.profile_sidebar.refilter(&self.config.groups);
        self.profile_sidebar.scroll = 0;
        self.dirty = true;
    }

    pub(crate) fn toggle_profile_sidebar(&mut self) {
        let mut config = self.config.clone();
        config.appearance.profile_sidebar = !config.appearance.profile_sidebar;
        self.set_config(config);
        if self.config.appearance.profile_sidebar {
            self.profile_sidebar.focus = Focus::Filter;
        }
        self.push_save_config();
    }

    pub(crate) fn sidebar_focused(&self) -> bool {
        self.profile_sidebar.focus != Focus::None && self.sidebar_geometry().is_some()
    }

    pub(crate) fn blur_sidebar(&mut self) {
        if self.profile_sidebar.focus != Focus::None {
            self.profile_sidebar.focus = Focus::None;
            self.dirty = true;
        }
    }

    fn update_sidebar_query(&mut self, visible: usize) {
        let browser = &mut self.profile_sidebar;
        browser.refilter(&self.config.groups);
        browser.selected = browser.rows.iter().position(|row| matches!(row.kind, TreeKind::Profile(_))).unwrap_or(0);
        browser.scroll = 0;
        browser.reveal(visible);
        self.dirty = true;
    }

    pub(crate) fn sidebar_text(&mut self, text: &str) {
        let Some(geo) = self.sidebar_geometry() else { return };
        self.profile_sidebar.focus = Focus::Filter;
        self.profile_sidebar.query.extend(text.chars().filter(|c| !c.is_control()));
        self.update_sidebar_query(geo.visible());
    }

    pub(crate) fn sidebar_key(&mut self, key: Key, mods: Mods) -> bool {
        let Some(geo) = self.sidebar_geometry().filter(|_| self.profile_sidebar.focus != Focus::None) else { return false };
        let browser = &mut self.profile_sidebar;
        let visible = geo.visible();
        let last = browser.rows.len().saturating_sub(1);
        let page = visible.max(1);
        match key {
            Key::Escape => self.blur_sidebar(),
            Key::Tab if !mods.ctrl => {
                browser.focus = match (browser.focus, mods.shift) {
                    (Focus::Filter, false) => Focus::Tree,
                    (Focus::Tree, true) => Focus::Filter,
                    _ => Focus::None,
                };
            }
            Key::Up => browser.selected = browser.selected.saturating_sub(1),
            Key::Down => browser.selected = (browser.selected + 1).min(last),
            Key::PageUp => browser.selected = browser.selected.saturating_sub(page),
            Key::PageDown => browser.selected = (browser.selected + page).min(last),
            Key::Home if browser.focus == Focus::Tree => browser.selected = 0,
            Key::End if browser.focus == Focus::Tree => browser.selected = last,
            Key::Backspace => {
                if mods.ctrl { browser.query.clear(); } else { browser.query.pop(); }
                self.update_sidebar_query(visible);
                return true;
            }
            Key::Enter => {
                self.activate_sidebar_row(visible);
                return true;
            }
            Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
            _ => return false,
        }
        self.profile_sidebar.reveal(visible);
        self.dirty = true;
        true
    }

    fn activate_sidebar_row(&mut self, visible: usize) {
        let browser = &mut self.profile_sidebar;
        let Some(row) = browser.rows.get(browser.selected).cloned() else { return };
        match row.kind {
            TreeKind::Header(header) => {
                if !browser.query.trim().is_empty() { return; }
                if !browser.collapsed.remove(&header) { browser.collapsed.insert(header); }
                browser.refilter(&self.config.groups);
                browser.reveal(visible);
                self.dirty = true;
            }
            TreeKind::Profile(i) => {
                let id = browser.profiles[i].id.clone();
                self.run_sidebar_profile(&id, Action::NewTab);
            }
            _ => {}
        }
    }

    pub(crate) fn sidebar_down(&mut self, x: f32, y: f32, button: MouseButton, clicks: u8) -> bool {
        let Some(geo) = self.sidebar_geometry().filter(|geo| geo.rect.contains(x, y)) else {
            self.blur_sidebar();
            return false;
        };
        let browser = &mut self.profile_sidebar;
        if geo.grip.contains(x, y) && button == MouseButton::Left {
            self.drag = Drag::SidebarResize { width: width(self.config.appearance.profile_sidebar_width) };
            return true;
        }
        if geo.filter.contains(x, y) && button == MouseButton::Left {
            browser.focus = Focus::Filter;
            self.dirty = true;
            return true;
        }
        if let Some(i) = geo.hit_row(x, y).map(|i| i + browser.scroll).filter(|&i| i < browser.rows.len()) {
            let kind = browser.rows[i].kind.clone();
            browser.selected = i;
            browser.focus = Focus::Tree;
            self.dirty = true;
            match (kind, button) {
                (TreeKind::Profile(i), MouseButton::Right) => {
                    let id = browser.profiles[i].id.clone();
                    self.open_sidebar_profile_menu(id, x, y);
                }
                (TreeKind::Header(_), MouseButton::Left) if clicks < 2 => self.activate_sidebar_row(geo.visible()),
                (TreeKind::Profile(_), MouseButton::Left) if clicks >= 2 => self.activate_sidebar_row(geo.visible()),
                _ => {}
            }
        }
        true
    }

    pub(crate) fn sidebar_move(&mut self, x: f32, y: f32) -> bool {
        let Some(geo) = self.sidebar_geometry().filter(|geo| geo.rect.contains(x, y)) else { return false };
        let cursor = if geo.grip.contains(x, y) { CursorIcon::ResizeHorizontal }
            else if geo.filter.contains(x, y) { CursorIcon::Text }
            else { CursorIcon::Default };
        self.set_cursor(cursor);
        self.dirty = true;
        true
    }

    pub(crate) fn sidebar_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let Some(geo) = self.sidebar_geometry().filter(|geo| geo.rect.contains(x, y)) else { return false };
        let browser = &mut self.profile_sidebar;
        let rows = (dy.abs() * 3.0).round().max(1.0) as usize;
        browser.scroll = if dy > 0.0 {
            browser.scroll.saturating_sub(rows)
        } else {
            browser.scroll.saturating_add(rows).min(browser.rows.len().saturating_sub(geo.visible()))
        };
        self.dirty = true;
        true
    }

    pub(crate) fn resize_sidebar(&mut self, x: f32) {
        let r = self.chrome.profile_sidebar;
        if r.w > 0.0 {
            self.drag = Drag::SidebarResize { width: width((x - r.x) / self.ui()).round() };
            self.set_cursor(CursorIcon::ResizeHorizontal);
            self.dirty = true;
        }
    }

    pub(crate) fn save_sidebar_width(&mut self, width: f32) {
        if self.config.appearance.profile_sidebar_width != width {
            self.config.appearance.profile_sidebar_width = width;
            self.push_save_config();
        }
    }

    pub(crate) fn run_sidebar_profile(&mut self, id: &str, action: Action) {
        let Some(profile) = self.profiles.iter().find(|profile| profile.id == id && !self.config.profile_hidden(profile)).cloned() else { return };
        self.blur_sidebar();
        match action {
            Action::NewTab | Action::SplitRight => {
                self.note_recent(id);
                if action == Action::NewTab { self.open_tab(profile); } else { self.split_with(Side::Right, profile); }
            }
            Action::Settings => {
                self.select_profile_settings(id);
                self.edit_profile_settings_row(id);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../test/profile_sidebar.rs"]
mod tests;
