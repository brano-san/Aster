use crate::motion::SLIDE;
use crate::rect_motion::RectMotion;
use aster_gfx::render::{Color, Rect};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

pub(crate) struct StripFrame {
    pub bar: Rect,
    pub vertical: bool,
    pub plus: Rect,
    pub menu: Rect,
    pub animate: bool,
    pub now: Instant,
}

pub(crate) struct TabGhost {
    pub label: String,
    pub color: Option<Color>,
    pub badge: Option<(String, u32)>,
    pub pinned: bool,
}

struct Slot {
    rect: RectMotion,
    closing: bool,
    ghost: Option<TabGhost>,
}

#[derive(Default)]
pub(crate) struct TabStrip {
    bar: Option<(Rect, bool)>,
    slots: BTreeMap<u64, Slot>,
    indicator: Option<RectMotion>,
    plus: RectMotion,
    menu: RectMotion,
}

fn visible(r: Rect) -> bool {
    r.w > 0.0 && r.h > 0.0
}

fn start(r: Rect, vertical: bool) -> f32 {
    if vertical { r.y } else { r.x }
}

fn end(r: Rect, vertical: bool) -> f32 {
    if vertical { r.bottom() } else { r.right() }
}

fn collapsed(r: Rect, at: f32, vertical: bool) -> Rect {
    if vertical { Rect::new(r.x, at, r.w, 0.0) } else { Rect::new(at, r.y, 0.0, r.h) }
}

impl TabStrip {
    pub fn settle(&mut self, tabs: &[(u64, Rect)], active: Option<u64>, drag: Option<(u64, f32)>, frame: StripFrame) -> bool {
        let StripFrame { bar, vertical, plus, menu, animate, now } = frame;
        let animate = animate && self.bar == Some((bar, vertical));
        self.bar = Some((bar, vertical));
        let targets: HashMap<_, _> = tabs.iter().copied().collect();
        let mut moving = false;
        self.slots.retain(|key, slot| {
            if let Some(rect) = targets.get(key) { return visible(*rect); }
            if !animate { return false; }
            if !slot.closing {
                let r = slot.rect.to;
                let seam = tabs.iter().map(|(_, target)| *target).filter(|target| visible(*target))
                    .find(|target| start(*target, vertical) >= start(r, vertical))
                    .map(|target| start(target, vertical))
                    .or_else(|| tabs.iter().rev().map(|(_, target)| *target).find(|target| visible(*target)).map(|target| end(target, vertical)))
                    .unwrap_or(start(r, vertical));
                slot.rect.aim(collapsed(r, seam, vertical), true, now, SLIDE);
                slot.closing = true;
            }
            let still = slot.rect.aim(slot.rect.to, true, now, SLIDE);
            moving |= still;
            still
        });
        let mut previous = None;
        for (i, &(key, target)) in tabs.iter().enumerate().filter(|(_, (_, rect))| visible(*rect)) {
            let dragging = drag.filter(|(dragged, _)| *dragged == key);
            let rect = if let Some((_, at)) = dragging {
                let low = start(bar, vertical);
                let high = end(bar, vertical) - (end(target, vertical) - start(target, vertical));
                let at = at.round().clamp(low, high.max(low));
                if vertical { Rect::new(target.x, at, target.w, target.h) } else { Rect::new(at, target.y, target.w, target.h) }
            } else {
                target
            };
            let seam = previous.or_else(|| tabs[i + 1..].iter().find_map(|(next, _)| {
                self.rect(*next, now).map(|r| start(r, vertical))
            })).unwrap_or(start(target, vertical));
            let slot = self.slots.entry(key).or_insert_with(|| Slot {
                rect: RectMotion::at(collapsed(target, seam, vertical)), closing: false, ghost: None,
            });
            moving |= slot.rect.aim(rect, animate && dragging.is_none(), now, SLIDE);
            previous = Some(end(slot.rect.value(now, SLIDE), vertical));
        }
        if let Some((key, target)) = active.and_then(|key| targets.get(&key).filter(|&&r| visible(r)).map(|&r| (key, r))) {
            let dragging = drag.is_some_and(|(dragged, _)| dragged == key);
            let target = if dragging { self.rect(key, now).unwrap_or(target) } else { target };
            moving |= self.indicator.get_or_insert_with(|| RectMotion::at(target)).aim(target, animate && !dragging, now, SLIDE);
        } else {
            self.indicator = None;
        }
        moving |= self.plus.aim(plus, animate, now, SLIDE);
        moving |= self.menu.aim(menu, animate && visible(menu) && visible(self.menu.to), now, SLIDE);
        moving
    }

    pub fn rect(&self, key: u64, now: Instant) -> Option<Rect> {
        self.slots.get(&key).map(|slot| slot.rect.value(now, SLIDE))
    }

    pub fn indicator(&self, now: Instant) -> Option<Rect> {
        self.indicator.as_ref().map(|rect| rect.value(now, SLIDE))
    }

    pub fn buttons(&self, now: Instant) -> (Rect, Rect) {
        (self.plus.value(now, SLIDE), self.menu.value(now, SLIDE))
    }

    pub fn remember(&mut self, key: u64, ghost: TabGhost) {
        if let Some(slot) = self.slots.get_mut(&key) {
            slot.ghost = Some(ghost);
        }
    }

    pub fn ghosts(&self, now: Instant) -> impl Iterator<Item = (Rect, &TabGhost)> {
        self.slots.values().filter(|slot| slot.closing).filter_map(move |slot| {
            Some((slot.rect.value(now, SLIDE), slot.ghost.as_ref()?))
        })
    }
}

#[cfg(test)]
#[path = "../test/tab_motion.rs"]
mod tests;
