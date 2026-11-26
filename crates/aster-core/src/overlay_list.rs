use aster_gfx::render::Rect;

use crate::app::App;
use crate::input::Key;
use crate::overlay::{Overlay, VISIBLE};
use crate::view::OverlayGeometry;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Position {
    offset: f32,
    viewport: (f32, f32),
}

pub(crate) struct OverlayList<'a> {
    pub frame: Rect,
    pub input: Rect,
    pub list: Rect,
    pub max: f32,
    item_height: f32,
    items: Vec<(f32, Rect)>,
    headers: Vec<(&'a str, Rect)>,
    dividers: Vec<Rect>,
}

impl<'a> OverlayList<'a> {
    pub fn new(content: Rect, s: f32, overlay: &'a Overlay, anchor: Option<Rect>) -> Self {
        let profiles = overlay.kind.is_profile_picker();
        let width = (if profiles { 520.0 } else { 560.0 } * s).min((content.w - 32.0 * s).max(200.0 * s)).min(content.w).round();
        let x = anchor.map_or(content.x + (content.w - width) / 2.0, |button| {
            let margin = ((content.w - width) / 2.0).clamp(0.0, 8.0 * s);
            button.x.clamp(content.x + margin, (content.right() - width - margin).max(content.x + margin))
        }).round();
        let inset = (10.0 * s).round();
        let y = content.y + inset;
        let input = Rect::new(x, y, width, (if profiles { 44.0 } else { 40.0 } * s).round());
        let item_height = (if profiles { 34.0 } else { 30.0 } * s).round();
        let header_height = (30.0 * s).round();
        let pad = (6.0 * s).round();
        let row = |y, h| Rect::new(x + pad, y, (width - 2.0 * pad).max(0.0), h);
        let top = input.bottom() + pad;
        let mut y = top;
        let mut first_page = 0.0;
        let mut previous = "";
        let mut items = Vec::with_capacity(overlay.shown.len());
        let mut headers = Vec::new();
        let mut dividers = Vec::new();
        for (position, (index, _)) in overlay.shown.iter().enumerate() {
            let group = overlay.items[*index].group.as_str();
            let block = y;
            if group != previous {
                if group.is_empty() {
                    let gap = (12.0 * s).round();
                    dividers.push(row(y + (gap * 0.5).round(), s.round().max(1.0)));
                    y += gap;
                } else {
                    headers.push((group, row(y, header_height)));
                    y += header_height;
                }
            }
            items.push((block, row(y, item_height)));
            y += item_height;
            previous = group;
            if position < VISIBLE {
                first_page = y - top;
            }
        }
        let available = (content.bottom() - top - pad).max(0.0).floor();
        let list = Rect::new(x, top, width, first_page.max(item_height).min(available));
        let frame = Rect::new(input.x, input.y, width, list.bottom() + pad - input.y);
        let max = (y - list.bottom()).max(0.0);
        let mut layout = Self { frame, input, list, max, item_height, items, headers, dividers };
        if let Some(button) = anchor {
            let lowest = (content.bottom() - inset - frame.h).max(input.y);
            let target = if button.y >= content.bottom() { lowest } else { button.y.clamp(input.y, lowest) };
            let dy = (target - input.y).round();
            layout.frame.y += dy;
            layout.input.y += dy;
            layout.list.y += dy;
            for (top, rect) in &mut layout.items {
                *top += dy;
                rect.y += dy;
            }
            for (_, rect) in &mut layout.headers { rect.y += dy; }
            for rect in &mut layout.dividers { rect.y += dy; }
        }
        layout
    }

    pub fn target(&self, overlay: &Overlay) -> f32 {
        if let Some(position) = overlay.scroll_offset {
            let offset = position.offset.clamp(0.0, self.max);
            return if position.viewport == self.viewport() { offset } else { self.reveal(overlay.selected, offset) };
        }
        let offset = self.items.get(overlay.scroll).map_or(0.0, |(top, _)| top - self.list.y);
        self.reveal(overlay.selected, offset)
    }

    fn viewport(&self) -> (f32, f32) {
        (self.list.h, self.item_height)
    }

    fn position(&self, offset: f32) -> Position {
        Position { offset, viewport: self.viewport() }
    }

    pub fn reveal(&self, selected: usize, scroll: f32) -> f32 {
        let Some(&(start, rect)) = self.items.get(selected) else { return 0.0 };
        let start = if rect.bottom() - start <= self.list.h { start } else { rect.y };
        if start < self.list.y + scroll {
            start - self.list.y
        } else if rect.bottom() > self.list.bottom() + scroll {
            rect.bottom() - self.list.bottom()
        } else {
            scroll
        }.clamp(0.0, self.max)
    }

    fn limit_selection(&self, selected: usize, scroll: f32) -> usize {
        let range = self.visible_rows(scroll);
        if range.is_empty() { selected } else { selected.clamp(range.start, range.end - 1) }
    }

    fn visible_rows(&self, scroll: f32) -> std::ops::Range<usize> {
        let start = self.items.partition_point(|(_, rect)| rect.bottom() <= self.list.y + scroll);
        let end = self.items.partition_point(|(_, rect)| rect.y < self.list.bottom() + scroll);
        start..end.max(start)
    }

    pub fn hit_row(&self, scroll: f32, (x, y): (f32, f32)) -> Option<usize> {
        if !self.list.contains(x, y) { return None; }
        let y = y + scroll.round();
        let row = self.items.partition_point(|(_, rect)| rect.bottom() <= y);
        self.items.get(row).filter(|(_, rect)| rect.contains(x, y)).map(|_| row)
    }

    pub fn geometry(&self, scroll: f32) -> OverlayGeometry {
        let scroll = scroll.round();
        let dress = |r: Rect| Rect::new(r.x, r.y - scroll, r.w, r.h);
        let visible = |r: Rect| r.bottom() > self.list.y + scroll && r.y < self.list.bottom() + scroll;
        let items = self.visible_rows(scroll).map(|i| (i, dress(self.items[i].1))).collect();
        let headers = self.headers.iter().filter(|(_, r)| visible(*r)).map(|&(title, r)| (title.to_owned(), dress(r))).collect();
        let dividers = self.dividers.iter().copied().filter(|&r| visible(r)).map(dress).collect();
        OverlayGeometry { frame: self.frame, input: self.input, list: self.list, items, headers, dividers }
    }
}

impl App {
    pub(crate) fn scroll_overlay_key(&mut self, key: Key) -> bool {
        let delta = match key {
            Key::Up => -1,
            Key::Down => 1,
            Key::PageUp => -8,
            Key::PageDown => 8,
            Key::Home => isize::MIN / 2,
            Key::End => isize::MAX / 2,
            _ => return false,
        };
        let (animate, now, scale) = (self.animations(), self.clock(), self.ui());
        let Some(overlay) = self.overlay.as_mut().filter(|o| o.kind.scrolls_smoothly()) else { return false };
        let layout = OverlayList::new(self.chrome.content, scale, overlay, overlay.tab_anchor(self.chrome.tab_menu));
        let previous = layout.target(overlay);
        let target = layout.reveal(overlay.selection_after(delta), previous);
        let position = layout.position(target);
        overlay.step(delta);
        overlay.scroll_offset = Some(position);
        self.list_motion.overlay.retarget(previous, target, animate, now);
        self.dirty = true;
        true
    }

    pub(crate) fn scroll_overlay_wheel(&mut self, dy: f32, pixels: Option<f32>) -> bool {
        let (animate, now, scale) = (self.animations(), self.clock(), self.ui());
        let Some(overlay) = self.overlay.as_mut().filter(|o| o.kind.scrolls_smoothly()) else { return false };
        let layout = OverlayList::new(self.chrome.content, scale, overlay, overlay.tab_anchor(self.chrome.tab_menu));
        let previous = layout.target(overlay);
        let steps = dy * 3.0;
        let target = self.list_motion.overlay.wheel(previous, layout.max, steps * layout.item_height, pixels, animate, now);
        let selected = (overlay.selected as isize).saturating_sub(steps.round() as isize).clamp(0, overlay.shown.len().saturating_sub(1) as isize) as usize;
        let next = layout.limit_selection(if pixels.is_some() { overlay.selected } else { selected }, target);
        let position = layout.position(target);
        overlay.selected = next;
        overlay.scroll_offset = Some(position);
        self.dirty = true;
        true
    }
}

#[cfg(test)]
#[path = "../test/overlay_list.rs"]
mod tests;
