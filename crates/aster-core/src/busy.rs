use crate::motion::{spin_phase, Tween, SLIDE};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ProgressBar {
    Tab(u64),
    Transfer(u64),
    Update(Instant),
}

pub(crate) struct Busy {
    bars: HashMap<ProgressBar, (Tween, bool)>,
    now: Instant,
    enabled: bool,
    moving: bool,
}

impl Default for Busy {
    fn default() -> Self {
        Self { bars: HashMap::new(), now: Instant::now(), enabled: false, moving: false }
    }
}

impl Busy {
    pub(crate) fn begin(&mut self, now: Instant, enabled: bool) {
        self.now = now;
        self.enabled = enabled;
        self.moving = false;
        for (_, seen) in self.bars.values_mut() {
            *seen = false;
        }
    }

    pub(crate) fn progress(&mut self, bar: ProgressBar, target: f32) -> f32 {
        let (value, seen) = self.bars.entry(bar).or_insert_with(|| (Tween::at(0.0), false));
        *seen = true;
        self.moving |= value.aim(target, self.enabled, self.now, SLIDE);
        value.value(self.now, SLIDE)
    }

    pub(crate) fn phase(&mut self) -> f32 {
        self.moving |= self.enabled;
        spin_phase(self.now, self.enabled)
    }

    pub(crate) fn finish(&mut self) -> bool {
        self.bars.retain(|_, (_, seen)| *seen);
        self.moving
    }
}

#[cfg(test)]
#[path = "../test/busy.rs"]
mod tests;
