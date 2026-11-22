use crate::app::{App, Drag};
use crate::input::{CursorIcon, Key, Mods};
use crate::panes::{PaneId, Side};
use aster_gfx::render::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropZone {
    Side(Side),
    Center,
}

pub(crate) fn drop_zones(rect: Rect) -> [(DropZone, Rect); 5] {
    let width = rect.w * 0.24;
    let height = rect.h * 0.24;
    let x = rect.x + width;
    let middle = rect.w - 2.0 * width;
    [
        (DropZone::Side(Side::Left), Rect::new(rect.x, rect.y, width, rect.h)),
        (DropZone::Side(Side::Right), Rect::new(rect.right() - width, rect.y, width, rect.h)),
        (DropZone::Side(Side::Top), Rect::new(x, rect.y, middle, height)),
        (DropZone::Side(Side::Bottom), Rect::new(x, rect.bottom() - height, middle, height)),
        (DropZone::Center, Rect::new(x, rect.y + height, middle, rect.h - 2.0 * height)),
    ]
}

fn drop_zone(rect: Rect, x: f32, y: f32) -> Option<DropZone> {
    drop_zones(rect).into_iter().find_map(|(zone, bounds)| bounds.contains(x, y).then_some(zone))
}

fn label_width(count: usize) -> usize {
    let mut index = count.saturating_sub(1);
    let mut width = 1;
    while index >= 26 {
        index /= 26;
        width += 1;
    }
    width
}

pub(crate) fn pane_label(mut index: usize, count: usize) -> String {
    let mut bytes = vec![b'A'; label_width(count)];
    for byte in bytes.iter_mut().rev() {
        *byte += (index % 26) as u8;
        index /= 26;
    }
    String::from_utf8(bytes).unwrap()
}

pub(crate) struct Rearrange {
    tab: u64,
    pub first: Option<PaneId>,
    prefix: Vec<u8>,
}

impl App {
    pub(crate) fn rearranging(&self) -> bool {
        self.rearrange.as_ref().is_some_and(|r| self.tabs.get(self.active).is_some_and(|t| t.key == r.tab))
    }

    pub(crate) fn toggle_rearrange(&mut self) {
        if self.rearranging() {
            self.stop_rearranging();
            return;
        }
        self.blur_sidebar();
        let Some(tab) = self.tabs.get_mut(self.active).filter(|t| self.panes.contains_key(&t.focus)) else { return };
        tab.zoomed = None;
        self.rearrange = Some(Rearrange { tab: tab.key, first: None, prefix: Vec::new() });
        self.dirty = true;
    }

    pub(crate) fn stop_rearranging(&mut self) {
        if self.rearrange.take().is_some() {
            if matches!(self.drag, Drag::Pane { .. }) {
                self.drag = Drag::None;
            }
            self.dirty = true;
        }
    }

    pub(crate) fn rearrange_key(&mut self, key: Key, mods: Mods) -> bool {
        if !self.rearranging() {
            return false;
        }
        match key {
            Key::Escape => self.stop_rearranging(),
            Key::Char(c) if !mods.ctrl && !mods.alt && !mods.meta && c.is_ascii_alphabetic() => self.rearrange_char(c),
            Key::Backspace => {
                if let Some(state) = &mut self.rearrange {
                    if state.prefix.pop().is_none() {
                        state.first = None;
                    }
                }
                self.dirty = true;
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn rearrange_char(&mut self, c: char) {
        if !c.is_ascii_alphabetic() {
            return;
        }
        let Some(state) = self.rearrange.as_mut() else { return };
        let Some(tab) = self.tabs.get_mut(self.active).filter(|tab| tab.key == state.tab) else { return };
        let panes = tab.root.panes();
        state.prefix.push(c.to_ascii_uppercase() as u8 - b'A');
        if state.prefix.len() < label_width(panes.len()) {
            return;
        }
        let index = std::mem::take(&mut state.prefix).into_iter().fold(0usize, |index, digit| index.saturating_mul(26).saturating_add(digit as usize));
        let Some(&pane) = panes.get(index) else { return };
        if let Some(from) = state.first.take() {
            tab.root.swap_panes(from, pane);
        } else {
            state.first = Some(pane);
        }
        self.dirty = true;
    }

    pub(crate) fn start_pane_drag(&mut self, pane: PaneId, x: f32, y: f32) {
        self.drag = Drag::Pane { pane, start: (x, y), moved: false };
    }

    pub(crate) fn drag_pane(&mut self, pane: PaneId, start: (f32, f32), moved: bool, x: f32, y: f32) {
        if moved || (x - start.0).abs().max((y - start.1).abs()) > 6.0 * self.ui() {
            self.drag = Drag::Pane { pane, start, moved: true };
            self.set_cursor(CursorIcon::Hand);
            self.dirty = true;
        }
    }

    pub(crate) fn pane_drop_at(&self, source: Option<PaneId>, x: f32, y: f32) -> Option<(PaneId, Rect, DropZone)> {
        let (target, rect) = self.pane_at(x, y)?;
        if source == Some(target) || !self.panes.contains_key(&target) {
            return None;
        }
        Some((target, rect, drop_zone(rect, x, y)?))
    }

    pub(crate) fn pane_drag_hover(&self) -> Option<(Rect, DropZone)> {
        let Drag::Pane { pane, moved: true, .. } = self.drag else { return None };
        self.pane_drop_at(Some(pane), self.mouse.0, self.mouse.1).map(|(_, rect, zone)| (rect, zone))
    }

    pub(crate) fn finish_pane_drag(&mut self, pane: PaneId, x: f32, y: f32) {
        let Some((target, _, zone)) = self.pane_drop_at(Some(pane), x, y) else { return };
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        match zone {
            DropZone::Side(side) => { tab.root.move_pane(pane, target, side); }
            DropZone::Center => { tab.root.swap_panes(pane, target); }
        }
        if let Some(state) = &mut self.rearrange {
            state.first = None;
            state.prefix.clear();
        }
        self.dirty = true;
    }
}

#[cfg(test)]
#[path = "../test/rearrange.rs"]
mod tests;