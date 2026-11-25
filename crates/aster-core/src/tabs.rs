use crate::app::{App, Drag, Effect, Tab};
use crate::config::{Appearance, TabBarPosition, TabWidth};
use crate::input::{Key, Mods};
use crate::panes::{Node, Side};
use crate::rearrange::DropZone;
use aster_gfx::render::Rect;

pub const TAB_H: f32 = 36.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabSlot {
    pub rect: Rect,
    pub close: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chrome {
    pub tab_bar: Rect,
    pub caption: Rect,
    pub tabs: Vec<TabSlot>,
    pub plus: Rect,
    pub tab_menu: Rect,
    pub settings: Rect,
    pub min: Rect,
    pub max: Rect,
    pub close: Rect,
    pub content: Rect,
    pub profile_sidebar: Rect,
    pub vertical: bool,
}

pub fn chrome(w: f32, h: f32, s: f32, tabs: usize, pinned: usize, active: usize, appearance: &Appearance) -> Chrome {
    use TabBarPosition::*;
    let position = appearance.tab_bar_position;
    let vertical = matches!(position, Left | Right);
    let row = (TAB_H * s).round();
    let caption_h = (if position == Top { row } else { (32.0 * s).round() }).min(h);
    let caption = Rect::new(0.0, 0.0, w, caption_h);
    let button_w = (46.0 * s).round().min((w / 3.0).floor());
    let caption_button = |n| Rect::new(w - n * button_w, 0.0, button_w, caption_h);
    let (min, max, close) = (caption_button(3.0), caption_button(2.0), caption_button(1.0));
    let below = Rect::new(0.0, caption_h, w, h - caption_h);
    let side = (220.0 * s).round().min((w / 2.0).floor());
    let bottom = row.min(below.h);
    let (tab_bar, content) = match position {
        Top => (caption, below),
        Bottom => (Rect::new(0.0, h - bottom, w, bottom), Rect::new(0.0, below.y, w, below.h - bottom)),
        Left => (Rect::new(0.0, below.y, side, below.h), Rect::new(side, below.y, w - side, below.h)),
        Right => (Rect::new(w - side, below.y, side, below.h), Rect::new(0.0, below.y, w - side, below.h)),
    };
    let mut layout = Chrome { tab_bar, caption, min, max, close, content, vertical, ..Chrome::default() };
    layout.place_tabs(s, tabs, pinned, active, appearance);
    layout
}

impl Chrome {
    fn place_tabs(&mut self, s: f32, tabs: usize, pinned: usize, active: usize, appearance: &Appearance) {
        let tab_bar = self.tab_bar;
        let row = (TAB_H * s).round();
        let top = appearance.tab_bar_position == TabBarPosition::Top;
        let span = if self.vertical { tab_bar.h } else if top { self.min.x } else { tab_bar.w };
        let margin = (8.0 * s).round().min(span);
        let plus_span = row.min(span - margin);
        let settings_span = if span - margin - plus_span >= row { row } else { 0.0 };
        let remaining = span - margin - plus_span - settings_span;
        let menu_width = (24.0 * s).round();
        let menu_span = if appearance.tab_menu_button && tabs > 0 && remaining >= menu_width { menu_width } else { 0.0 };
        let gap = (4.0 * s).round().min(remaining - menu_span);
        let available = (remaining - menu_span - gap - (12.0 * s).round()).max(0.0);
        let pin_width = (48.0 * s).round();
        let preferred = (if appearance.tab_width == TabWidth::Fixed { 200.0 } else { 220.0 } * s).round();
        let regular_width = ((available - pinned as f32 * pin_width) / tabs.saturating_sub(pinned).max(1) as f32)
            .clamp((84.0 * s).round(), preferred).floor();
        let tab_span = |i| if self.vertical { row } else if i < pinned { pin_width } else { regular_width };
        let active = active.min(tabs.saturating_sub(1));
        let mut first = active;
        let mut occupied = tab_span(active);
        let mut visible_pins = 0;
        while visible_pins < pinned.min(active) && occupied + tab_span(visible_pins) <= available {
            occupied += tab_span(visible_pins);
            visible_pins += 1;
        }
        while first > visible_pins && occupied + tab_span(first - 1) <= available {
            first -= 1;
            occupied += tab_span(first);
        }
        let inset = (6.0 * s).round();
        let mut cursor = margin;
        let mut slots = Vec::with_capacity(tabs);
        for i in 0..tabs {
            let length = tab_span(i).min(available);
            if (visible_pins..first).contains(&i) || cursor + length > margin + available {
                slots.push(TabSlot { rect: Rect::new(tab_bar.x, tab_bar.y, 0.0, 0.0), close: Rect::default() });
                continue;
            }
            let rect = if self.vertical {
                let width = if i < pinned { pin_width } else { preferred };
                Rect::new(tab_bar.x + inset.min(tab_bar.w), tab_bar.y + cursor, width.min((tab_bar.w - 2.0 * inset).max(0.0)), length)
            } else {
                let pad = inset.min(tab_bar.h);
                Rect::new(cursor, tab_bar.y + if top { pad } else { 0.0 }, length, tab_bar.h - pad)
            };
            let close_size = (24.0 * s).round().min(rect.h);
            let close = if appearance.tab_close_buttons && i >= pinned && rect.w >= close_size + 2.0 * inset {
                Rect::new(rect.right() - close_size - inset, rect.y + ((rect.h - close_size) / 2.0).round(), close_size, close_size)
            } else {
                Rect::default()
            };
            slots.push(TabSlot { rect, close });
            cursor += length;
        }
        self.tabs = slots;
        self.plus = if self.vertical {
            Rect::new(tab_bar.x + inset.min(tab_bar.w), tab_bar.y + cursor + gap, row.min((tab_bar.w - inset).max(0.0)), plus_span)
        } else {
            let height = (row * 0.8).round().min(tab_bar.h);
            let pad = if top { inset.min(tab_bar.h - height) } else { 0.0 };
            Rect::new(cursor + gap, tab_bar.y + ((tab_bar.h + pad - height) / 2.0).round(), plus_span, height)
        };
        if menu_span > 0.0 {
            let plus = self.plus;
            self.tab_menu = if self.vertical {
                Rect::new(plus.x, plus.bottom(), plus.w, menu_span)
            } else {
                Rect::new(plus.right(), plus.y, menu_span, plus.h)
            };
        }
        if settings_span > 0.0 {
            let start = span - margin - settings_span;
            self.settings = if self.vertical {
                Rect::new(self.plus.x, tab_bar.y + start, self.plus.w, settings_span)
            } else {
                Rect::new(tab_bar.x + start, self.plus.y, settings_span, self.plus.h)
            };
        }
    }
}

pub fn bare_chrome(w: f32, h: f32) -> Chrome {
    Chrome { content: Rect::new(0.0, 0.0, w, h), ..Chrome::default() }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rename {
    pub tab: usize,
    pub text: String,
}

#[derive(Default)]
pub(crate) struct MruTabs {
    order: Vec<u64>,
    selected: Option<usize>,
}

impl MruTabs {
    pub(crate) fn visit(&mut self, key: u64) {
        if self.order.first() != Some(&key) {
            self.cancel();
            self.order.retain(|&old| old != key);
            self.order.insert(0, key);
        }
    }

    pub(crate) fn remove(&mut self, key: u64) {
        if let Some(index) = self.order.iter().position(|&old| old == key) {
            self.order.remove(index);
            self.selected = self.selected.filter(|&i| i != index && self.order.len() > 1).map(|i| if i > index { i - 1 } else { i });
        }
    }

    fn step(&mut self, live: &[u64], reverse: bool) {
        if self.selected.is_none() {
            self.order.retain(|key| live.contains(key));
            for key in live {
                if !self.order.contains(key) {
                    self.order.push(*key);
                }
            }
        }
        let count = self.order.len();
        if count > 1 {
            let current = self.selected.unwrap_or(0);
            self.selected = Some((current + if reverse { count - 1 } else { 1 }) % count);
        }
    }

    pub(crate) fn selection(&self) -> Option<(&[u64], usize)> {
        self.selected.map(|index| (self.order.as_slice(), index))
    }

    fn finish(&mut self) -> Option<u64> {
        self.selected.take().map(|index| self.order[index])
    }

    pub(crate) fn cancel(&mut self) {
        self.selected = None;
    }
}

impl App {
    pub(crate) fn tab_chrome(&self) -> Chrome {
        let (w, h) = self.size;
        let mut chrome = if self.tab_bar_hidden() {
            bare_chrome(w, h)
        } else {
            chrome(w, h, self.ui(), self.tabs.len(), self.tabs.partition_point(|tab| tab.pinned), self.active, &self.config.appearance)
        };
        self.place_profile_sidebar(&mut chrome);
        chrome
    }

    pub(crate) fn tab_label(&self, index: usize) -> String {
        if self.tabs[index].pinned && self.config.appearance.tab_index {
            (index + 1).to_string()
        } else if self.tabs[index].pinned && self.config.appearance.tab_icon {
            String::new()
        } else if self.config.appearance.tab_index {
            format!("{}  {}", index + 1, self.tab_title(index))
        } else {
            self.tab_title(index)
        }
    }

    pub(crate) fn drag_tab(&mut self, mut index: usize, start: (f32, f32), grab: (f32, f32), moved: bool, x: f32, y: f32) {
        if !moved && (x - start.0).abs().max((y - start.1).abs()) <= 6.0 * self.ui() {
            return;
        }
        if self.chrome.tab_bar.contains(x, y) {
            let target = self.chrome.tabs.iter().position(|slot| {
                if self.chrome.vertical { slot.rect.contains(slot.rect.x, y) } else { slot.rect.contains(x, slot.rect.y) }
            });
            if let Some(target) = target.filter(|&target| index != target && self.can_move_tab(index, target)) {
                let active = self.tabs[self.active].key;
                let source = self.tabs.remove(index);
                self.tabs.insert(target, source);
                self.active = self.tabs.iter().position(|tab| tab.key == active).unwrap_or(target);
                index = target;
                self.chrome = self.tab_chrome();
            }
        }
        self.drag = Drag::Tab { index, start, grab, moved: true };
        self.dirty = true;
    }

    fn can_drop_tab(&self, index: usize) -> bool {
        index != self.active && self.files.is_none()
            && [index, self.active].into_iter().all(|i| self.tabs.get(i).is_some_and(|tab| self.panes.contains_key(&tab.focus)))
    }

    pub(crate) fn tab_drag_hover(&self) -> Option<(Rect, DropZone)> {
        let Drag::Tab { index, moved: true, .. } = self.drag else { return None };
        if !self.can_drop_tab(index) { return None; }
        self.pane_drop_at(None, self.mouse.0, self.mouse.1).map(|(_, rect, zone)| (rect, zone))
    }

    pub(crate) fn finish_tab_drag(&mut self, index: usize, moved: bool, x: f32, y: f32) {
        if moved && self.can_drop_tab(index) {
            if let Some((pane, _, zone)) = self.pane_drop_at(None, x, y) {
                if self.tabs[self.active].root.contains(pane) {
                    let side = match zone { DropZone::Side(side) => side, DropZone::Center => Side::Right };
                    let source = self.tabs.remove(index);
                    self.mru.remove(source.key);
                    self.active -= usize::from(index < self.active);
                    let destination = &mut self.tabs[self.active];
                    destination.root.split_tree(pane, side, source.root);
                    destination.zoomed = None;
                    destination.focus = source.focus;
                    self.stop_rearranging();
                    self.prompt_pending_serial();
                    self.dirty = true;
                    return;
                }
            }
        }
        let clicked = self.chrome.tabs.get(index).is_some_and(|slot| slot.rect.contains(x, y));
        if clicked || (moved && self.chrome.tab_bar.contains(x, y)) {
            self.activate(index);
        }
    }

    pub(crate) fn move_pane_to_tab(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let pane = tab.focus;
        let order = tab.root.panes();
        let Some(index) = order.iter().position(|&id| id == pane) else { return };
        if !tab.root.remove(pane) {
            return;
        }
        let remaining = order.into_iter().filter(|&id| id != pane).collect::<Vec<_>>();
        let origin = remaining[index.saturating_sub(1)];
        tab.focus = origin;
        tab.zoomed = None;
        self.remember_tab_origin(origin);
        let at = self.tab_insert_index(false);
        self.tabs.insert(at, Tab::new(Node::Leaf(pane), pane));
        self.active = at;
        self.stop_rearranging();
        self.prompt_pending_serial();
        self.dirty = true;
    }

    pub(crate) fn switch_mru(&mut self, action: crate::keymap::Action, held: bool) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        self.mru.visit(tab.key);
        let live: Vec<_> = self.tabs.iter().map(|tab| tab.key).collect();
        self.mru.step(&live, action == crate::keymap::Action::PreviousMruTab);
        if !held {
            self.finish_mru();
        }
        self.dirty = true;
    }

    pub(crate) fn finish_mru(&mut self) {
        if let Some(key) = self.mru.finish() {
            if let Some(index) = self.tabs.iter().position(|tab| tab.key == key) {
                self.activate(index);
            }
            self.dirty = true;
        }
    }

    pub(crate) fn tab_insert_index(&self, pinned: bool) -> usize {
        let boundary = self.tabs.partition_point(|tab| tab.pinned);
        if pinned { boundary } else { (self.active + 1).min(self.tabs.len()).max(boundary) }
    }

    pub(crate) fn can_move_tab(&self, from: usize, to: usize) -> bool {
        self.tabs.get(from).zip(self.tabs.get(to)).is_some_and(|(a, b)| a.pinned == b.pinned)
    }

    pub(crate) fn toggle_pin(&mut self) {
        if self.active >= self.tabs.len() {
            return;
        }
        self.commit_rename();
        let mut tab = self.tabs.remove(self.active);
        tab.pinned = !tab.pinned;
        self.active = self.tabs.partition_point(|t| t.pinned);
        self.tabs.insert(self.active, tab);
        self.dirty = true;
    }

    pub fn open_tab_at(&mut self, profile: crate::profile::Profile, cwd: Option<String>) {
        let cwd = cwd.filter(|_| profile.ssh.is_none() || self.config.ssh.client != crate::config::SshClient::Native);
        self.insert_profile_tab(profile, cwd);
    }

    pub(crate) fn insert_profile_tab(&mut self, profile: crate::profile::Profile, cwd: Option<String>) {
        let title = profile.layout.as_ref().map(|_| profile.name.clone());
        let Some(root) = self.profile_tree(profile, cwd) else { return };
        let focus = root.panes()[0];
        let mut tab = crate::app::Tab::new(root, focus);
        tab.title = title;
        let at = self.tab_insert_index(false);
        self.tabs.insert(at, tab);
        self.active = at;
        self.mru.visit(self.tabs[at].key);
        self.overlay = None;
        self.prompt_pending_serial();
        self.dirty = true;
    }

    pub fn open_named_profile(&mut self, name: &str) -> bool {
        let Some(profile) = self.find_profile(name) else { return false };
        self.note_recent(&profile.id);
        self.open_tab(profile);
        true
    }

    pub fn find_profile(&self, name: &str) -> Option<crate::profile::Profile> {
        let name = name.trim();
        self.profiles.iter().find(|p| p.id == name).or_else(|| self.profiles.iter().find(|p| p.name.eq_ignore_ascii_case(name))).cloned()
    }

    pub fn run_command(&mut self, command: &[String], cwd: Option<String>) {
        let Some((program, args)) = command.split_first() else { return };
        let mut profile = crate::profile::Profile::new("run", &command.join(" "), program, &[]);
        profile.args = args.to_vec();
        profile.hold = true;
        self.open_tab_at(profile, cwd);
    }

    pub fn open_delegated_console(&mut self, token: String) {
        if !crate::default_terminal::valid_token(&token) {
            return;
        }
        let mut profile = crate::profile::Profile::new("run", crate::i18n::t("Console application"), "cmd.exe", &[]);
        profile.hold = true;
        self.insert_profile_tab(profile, None);
        if let Some(launch) = self.focused_pane().and_then(|pane| self.panes.get_mut(&pane)).and_then(|pane| pane.pending.as_mut()) {
            launch.delegation = Some(token);
        }
    }

    pub fn renaming(&self) -> Option<&Rename> {
        self.rename.as_ref()
    }

    pub(crate) fn start_rename(&mut self, tab: usize) {
        if tab < self.tabs.len() {
            self.activate(tab);
            self.rename = Some(Rename { tab, text: self.tab_title(tab) });
            self.dirty = true;
        }
    }

    pub(crate) fn commit_rename(&mut self) {
        let Some(r) = self.rename.take() else { return };
        if let Some(tab) = self.tabs.get_mut(r.tab) {
            let text = r.text.trim();
            tab.title = (!text.is_empty()).then(|| text.to_owned());
        }
        self.dirty = true;
    }

    pub(crate) fn rename_key(&mut self, key: Key, mods: Mods) {
        let Some(r) = self.rename.as_mut() else { return };
        self.dirty = true;
        match key {
            Key::Enter | Key::Tab => self.commit_rename(),
            Key::Escape => self.rename = None,
            Key::Backspace if mods.ctrl => r.text.clear(),
            Key::Backspace => {
                r.text.pop();
            }
            Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
            _ => {}
        }
    }

    pub(crate) fn rename_text(&mut self, text: &str) {
        let Some(r) = self.rename.as_mut() else { return };
        r.text.extend(text.chars().filter(|c| !c.is_control()));
        self.dirty = true;
    }

    pub(crate) fn close_tabs_except(&mut self, keep: impl Fn(usize, usize) -> bool) {
        let active = self.active;
        let reps: Vec<_> = self.tabs.iter().enumerate().filter(|(i, tab)| !tab.pinned && !keep(*i, active)).map(|(_, t)| t.focus).collect();
        if !reps.is_empty() {
            self.request_close(crate::close::CloseTarget::Tabs(reps));
        }
    }
}

#[cfg(test)]
#[path = "../test/tabs.rs"]
mod tests;
