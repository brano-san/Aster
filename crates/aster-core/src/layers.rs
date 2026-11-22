use crate::motion::eased;
use crate::panes::PaneId;
use aster_gfx::render::{DrawList, Instance, Rect};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Layer {
    Overlay,
    Menu,
    Toast,
    RestartNotice,
    UpdateNotice,
    Close,
    Paste,
    Vault,
    SettingsVault,
    Password(PaneId),
    Broadcast(PaneId),
    Transfer(PaneId),
    SshAuth(PaneId),
}

struct Ghost {
    since: Instant,
    instances: Vec<Instance>,
}

#[derive(Default)]
struct Slot {
    opened: Option<Instant>,
    settled: bool,
    snapshot: Vec<Instance>,
    ghost: Option<Ghost>,
    seen: bool,
}

pub(crate) struct Layers {
    slots: HashMap<Layer, Slot>,
    atlas: u64,
    now: Instant,
    enabled: bool,
    moving: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self { slots: HashMap::new(), atlas: 0, now: Instant::now(), enabled: false, moving: false }
    }
}

impl Layers {
    pub(crate) fn begin(&mut self, atlas: u64, now: Instant, enabled: bool) {
        if self.atlas != atlas {
            self.slots.clear();
            self.atlas = atlas;
        }
        self.now = now;
        self.enabled = enabled;
        self.moving = false;
        for slot in self.slots.values_mut() {
            slot.seen = false;
            if !enabled {
                slot.settled = true;
                slot.snapshot.clear();
                slot.ghost = None;
            }
        }
    }

    pub(crate) fn retained(&self) -> impl Iterator<Item = &Instance> {
        self.slots.values().flat_map(|slot| slot.snapshot.iter().chain(
            slot.ghost.iter().flat_map(|ghost| ghost.instances.iter())
        ))
    }

    pub(crate) fn opened(&self, layer: Layer, visible: bool) -> Option<Instant> {
        visible.then(|| self.slots.get(&layer).and_then(|slot| slot.opened).unwrap_or(self.now))
    }

    pub(crate) fn start(&mut self, dl: &mut DrawList, layer: Layer, opened: Option<Instant>, fade: Duration) -> usize {
        let from = dl.instances.len();
        if opened.is_none() && !self.slots.contains_key(&layer) {
            return from;
        }
        let slot = self.slots.entry(layer).or_default();
        slot.seen = true;
        if slot.opened != opened {
            if !slot.snapshot.is_empty() {
                slot.ghost = Some(Ghost { since: self.now, instances: std::mem::take(&mut slot.snapshot) });
            }
            slot.opened = opened;
            slot.settled = !self.enabled;
        }
        if let Some(ghost) = &slot.ghost {
            let alpha = 1.0 - eased(ghost.since, self.now, fade);
            if alpha > 0.0 {
                dl.instances.extend_from_slice(&ghost.instances);
                dl.fade(from, alpha);
                self.moving = true;
            } else {
                slot.ghost = None;
            }
        }
        if opened.is_none() && slot.ghost.is_none() {
            self.slots.remove(&layer);
        }
        dl.instances.len()
    }

    pub(crate) fn progress(&mut self, layer: Layer, duration: Duration) -> f32 {
        let Some(slot) = self.slots.get_mut(&layer) else { return 1.0 };
        if !self.enabled || slot.settled {
            return 1.0;
        }
        let t = slot.opened.map_or(1.0, |at| eased(at, self.now, duration));
        slot.settled = t >= 1.0;
        self.moving |= !slot.settled;
        t
    }

    pub(crate) fn pop(&mut self, dl: &mut DrawList, layer: Layer, from: usize, rect: Rect, scrims: usize, duration: Duration) {
        let t = self.progress(layer, duration);
        dl.fade_range(from..from + scrims, t);
        dl.pop_in(from + scrims, t, (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
        self.save(dl, layer, from);
    }

    pub(crate) fn save(&mut self, dl: &DrawList, layer: Layer, from: usize) {
        if self.enabled {
            let slot = self.slots.get_mut(&layer).expect("active layer");
            slot.snapshot.clear();
            let instances = &dl.instances[from..];
            if instances.iter().any(|i| i.color >> 24 != 0) {
                slot.snapshot.extend_from_slice(instances);
            }
        }
    }

    pub(crate) fn forget_snapshot(&mut self, layer: Layer) {
        if let Some(slot) = self.slots.get_mut(&layer) {
            slot.snapshot.clear();
        }
    }

    pub(crate) fn finish(&mut self) -> bool {
        self.slots.retain(|_, slot| slot.seen);
        self.moving
    }
}

#[cfg(test)]
#[path = "../test/layers.rs"]
mod tests;
