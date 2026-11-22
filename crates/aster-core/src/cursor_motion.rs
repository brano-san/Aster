use std::time::{Duration, Instant};

use crate::motion::eased;

pub const BLINK: Duration = Duration::from_millis(530);
const FADE: Duration = Duration::from_millis(150);

pub(crate) struct Blink {
    on: bool,
    since: Instant,
    edge: Option<Instant>,
}

impl Blink {
    pub fn new(now: Instant) -> Self {
        Self { on: true, since: now, edge: None }
    }

    pub fn deadline(&self) -> Instant {
        self.since + BLINK
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        if now < self.deadline() { return false; }
        self.on = !self.on;
        self.since = now;
        self.edge = Some(now);
        true
    }

    pub fn sample(&self, enabled: bool, smooth: bool, now: Instant) -> (f32, bool) {
        if !enabled { return (1.0, false); }
        let target = f32::from(u8::from(self.on));
        let Some(edge) = self.edge.filter(|_| smooth) else { return (target, false) };
        let progress = eased(edge, now, FADE);
        let alpha = if self.on { progress } else { 1.0 - progress };
        (alpha, progress < 1.0)
    }
}

#[cfg(test)]
#[path = "../test/cursor_motion.rs"]
mod tests;
