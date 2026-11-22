use crate::motion::{Tween, POP, RESIZE};
use crate::panes::{Axis, Layout, PaneId, Splitter};
use crate::rect_motion::RectMotion;
use aster_gfx::render::Rect;
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Default)]
pub(crate) struct PaneMotion {
    scope: Option<(u64, Rect)>,
    zoom: Option<PaneId>,
    panes: BTreeMap<PaneId, RectMotion>,
    splitters: BTreeMap<Vec<bool>, RectMotion>,
}

fn contains(outer: Rect, inner: Rect) -> bool {
    outer.x <= inner.x && outer.right() >= inner.right() && outer.y <= inner.y && outer.bottom() >= inner.bottom()
}

fn leading(r: Rect, axis: Axis) -> f32 {
    match axis { Axis::Horizontal => r.x, Axis::Vertical => r.y }
}

fn trailing(r: Rect, axis: Axis) -> f32 {
    match axis { Axis::Horizontal => r.right(), Axis::Vertical => r.bottom() }
}

fn collapsed(r: Rect, axis: Axis, at: f32) -> Rect {
    match axis {
        Axis::Horizontal => Rect::new(at, r.y, 0.0, r.h),
        Axis::Vertical => Rect::new(r.x, at, r.w, 0.0),
    }
}

fn reveal(pane: Rect, zoomed: Rect) -> Rect {
    if pane.right() <= zoomed.x {
        collapsed(pane, Axis::Horizontal, pane.x)
    } else if pane.x >= zoomed.right() {
        collapsed(pane, Axis::Horizontal, pane.right())
    } else if pane.bottom() <= zoomed.y {
        collapsed(pane, Axis::Vertical, pane.y)
    } else {
        collapsed(pane, Axis::Vertical, pane.bottom())
    }
}

impl PaneMotion {
    pub fn settle(&mut self, tab: Option<u64>, bounds: Rect, layout: &Layout, zoom: Option<PaneId>, animate: bool, now: Instant) -> bool {
        let scope = tab.map(|key| (key, bounds));
        let same_panes = self.panes.len() == layout.panes.len() && layout.panes.iter().all(|(id, _)| self.panes.contains_key(id));
        let same_geometry = same_panes && layout.panes.iter().all(|(id, rect)| self.panes[id].to == *rect);
        let animate = animate && scope.is_some() && self.scope == scope && (!same_panes || same_geometry || self.zoom != zoom);
        let unzoom = self.zoom.filter(|_| zoom.is_none())
            .and_then(|id| layout.panes.iter().find(|(pane, _)| *pane == id).map(|(_, rect)| *rect));
        let sources: Vec<_> = if animate && !same_panes && unzoom.is_none() {
            layout.splitters.iter().filter_map(|splitter| self.split_source(layout, splitter, now).map(|source| (splitter, source))).collect()
        } else {
            Vec::new()
        };
        let mut moving = false;
        let mut panes = BTreeMap::new();
        for &(id, rect) in &layout.panes {
            let mut tween = self.panes.remove(&id).unwrap_or_else(|| {
                let from = if let Some(zoomed) = unzoom {
                    reveal(rect, zoomed)
                } else {
                    sources.iter().find(|(splitter, _)| contains(splitter.span, rect))
                        .map_or(rect, |(splitter, at)| collapsed(rect, splitter.axis, *at))
                };
                RectMotion::at(from)
            });
            moving |= tween.aim(rect, animate, now, RESIZE);
            panes.insert(id, tween);
        }
        let splitters = layout.splitters.iter().map(|splitter| {
            let mut tween = if let Some((_, at)) = sources.iter().find(|(sp, _)| sp.path == splitter.path) {
                RectMotion::at(collapsed(splitter.rect, splitter.axis, *at))
            } else if let Some(zoomed) = unzoom.filter(|zoomed| contains(splitter.span, *zoomed)) {
                let before = leading(zoomed, splitter.axis) < leading(splitter.rect, splitter.axis);
                let at = if before { trailing(splitter.span, splitter.axis) } else { leading(splitter.span, splitter.axis) };
                RectMotion::at(collapsed(splitter.rect, splitter.axis, at))
            } else {
                self.splitters.remove(&splitter.path).unwrap_or_else(|| RectMotion::at(splitter.rect))
            };
            moving |= tween.aim(splitter.rect, animate, now, RESIZE);
            (splitter.path.clone(), tween)
        }).collect();
        self.panes = panes;
        self.splitters = splitters;
        self.scope = scope;
        self.zoom = zoom;
        moving
    }

    fn split_source(&self, layout: &Layout, splitter: &Splitter, now: Instant) -> Option<f32> {
        let (id, old) = self.panes.iter().find(|(_, old)| old.to == splitter.span)?;
        let (_, remaining) = layout.panes.iter().find(|(pane, rect)| pane == id && contains(splitter.span, *rect))?;
        let axis = splitter.axis;
        let old = old.value(now, RESIZE);
        Some(if leading(*remaining, axis) < leading(splitter.rect, axis) { trailing(old, axis) } else { leading(old, axis) })
    }

    pub fn pane(&self, id: PaneId, now: Instant) -> Rect {
        self.panes[&id].value(now, RESIZE)
    }

    pub fn splitter(&self, path: &[bool], now: Instant) -> Rect {
        self.splitters[path].value(now, RESIZE)
    }
}

pub(crate) struct RearrangeMotion {
    labels: Tween,
    drop: Tween,
    target: Option<Rect>,
}

impl Default for RearrangeMotion {
    fn default() -> Self {
        Self { labels: Tween::at(0.0), drop: Tween::at(0.0), target: None }
    }
}

impl RearrangeMotion {
    pub fn settle(&mut self, labels: bool, target: Option<Rect>, animate: bool, now: Instant) -> (f32, f32, bool) {
        if target != self.target {
            self.drop = Tween::at(0.0);
            self.target = target;
        }
        let labels_moving = self.labels.aim(if labels { 1.0 } else { 0.0 }, animate && labels, now, POP);
        let drop_moving = self.drop.aim(if target.is_some() { 1.0 } else { 0.0 }, animate, now, POP);
        (self.labels.value(now, POP), self.drop.value(now, POP), labels_moving || drop_moving)
    }
}

#[cfg(test)]
#[path = "../test/pane_motion.rs"]
mod tests;
