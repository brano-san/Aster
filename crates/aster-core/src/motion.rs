use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const HOVER_FADE: Duration = Duration::from_millis(100);
pub const POP: Duration = Duration::from_millis(120);
pub const UNFOLD: Duration = Duration::from_millis(190);
pub const UNFOLD_STEP: Duration = Duration::from_millis(12);
pub const UNFOLD_STEP_MAX: Duration = Duration::from_millis(80);
pub const UNFOLD_ITEM: Duration = Duration::from_millis(110);
pub const SLIDE: Duration = Duration::from_millis(140);
pub const RESIZE: Duration = Duration::from_millis(140);
pub const TURN: Duration = Duration::from_millis(90);
pub const CHECK_IN: Duration = Duration::from_millis(140);
pub const TOAST_IN: Duration = Duration::from_millis(120);
pub const TOAST_OUT: Duration = Duration::from_millis(200);
pub const SPIN: Duration = Duration::from_millis(900);

pub fn ease_out(p: f32) -> f32 {
    1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3)
}

pub fn eased(since: Instant, now: Instant, span: Duration) -> f32 {
    let elapsed = now.saturating_duration_since(since);
    if elapsed >= span {
        return 1.0;
    }
    ease_out(elapsed.as_secs_f32() / span.as_secs_f32())
}

pub struct Tween {
    from: f32,
    to: f32,
    at: Option<Instant>,
}

impl Tween {
    pub fn at(value: f32) -> Self {
        Self { from: value, to: value, at: None }
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn value(&self, now: Instant, span: Duration) -> f32 {
        match self.at {
            Some(at) => {
                let t = eased(at, now, span);
                if t >= 1.0 { self.to } else { self.from + (self.to - self.from) * t }
            }
            None => self.to,
        }
    }

    pub fn aim(&mut self, to: f32, animate: bool, now: Instant, span: Duration) -> bool {
        if !animate || span.is_zero() {
            *self = Self::at(to);
        } else if self.to != to {
            let from = self.value(now, span);
            *self = Self { from, to, at: (from != to).then_some(now) };
        } else if self.at.is_some_and(|at| now.saturating_duration_since(at) >= span) {
            *self = Self::at(to);
        }
        self.at.is_some()
    }
}

pub struct Glow<K> {
    cur: Option<(K, Option<Instant>)>,
    prev: Option<(K, f32, Instant)>,
}

impl<K> Default for Glow<K> {
    fn default() -> Self {
        Self { cur: None, prev: None }
    }
}

impl<K: Copy + Eq> Glow<K> {
    pub fn aim(&mut self, key: Option<K>, animate: bool, now: Instant) -> bool {
        if !animate {
            self.cur = key.map(|key| (key, None));
            self.prev = None;
            return false;
        }

        let current = self.cur.map(|(key, _)| key);
        if current != key {
            let next = key.map(|key| {
                let alpha = self.alpha(key, now);
                let elapsed = HOVER_FADE.mul_f32(1.0 - (1.0 - alpha).cbrt());
                (key, now.checked_sub(elapsed))
            });
            let outgoing = current.and_then(|current| {
                let alpha = self.alpha(current, now);
                (alpha > 0.0).then_some((current, alpha, now))
            });
            self.prev = outgoing.or_else(|| self.prev.filter(|(previous, _, _)| Some(*previous) != key));
            self.cur = next;
        }

        if let Some((_, at)) = &mut self.cur {
            if at.is_some_and(|at| now.saturating_duration_since(at) >= HOVER_FADE) {
                *at = None;
            }
        }
        if self.prev.is_some_and(|(_, _, at)| now.saturating_duration_since(at) >= HOVER_FADE) {
            self.prev = None;
        }
        self.cur.is_some_and(|(_, at)| at.is_some()) || self.prev.is_some()
    }

    pub fn alpha(&self, key: K, now: Instant) -> f32 {
        if let Some((_, at)) = self.cur.filter(|(current, _)| *current == key) {
            return at.map_or(1.0, |at| eased(at, now, HOVER_FADE));
        }
        self.prev.filter(|(previous, _, _)| *previous == key)
            .map_or(0.0, |(_, alpha, at)| alpha * (1.0 - eased(at, now, HOVER_FADE)))
    }

    pub fn touches(&self, mut f: impl FnMut(K)) {
        if let Some((key, _, _)) = self.prev {
            f(key);
        }
        if let Some((key, _)) = self.cur {
            f(key);
        }
    }
}

#[derive(Default)]
pub struct Arrivals {
    owner: Option<u64>,
    seen: HashMap<u64, Option<Instant>>,
    next: HashMap<u64, Option<Instant>>,
}

impl Arrivals {
    pub fn settle(&mut self, owner: u64, drawn: &[u64], animate: bool, now: Instant) -> bool {
        let track = animate && self.owner == Some(owner);
        self.owner = Some(owner);
        let mut moving = false;
        for &key in drawn {
            let born = if track {
                self.seen.get(&key).copied().unwrap_or(Some(now))
                    .filter(|&at| now.saturating_duration_since(at) < POP)
            } else {
                None
            };
            moving |= born.is_some();
            self.next.insert(key, born);
        }
        std::mem::swap(&mut self.seen, &mut self.next);
        self.next.clear();
        moving
    }

    pub fn shown(&self, key: u64, now: Instant) -> f32 {
        self.seen.get(&key).and_then(|&at| at).map_or(1.0, |at| eased(at, now, POP))
    }
}

pub fn spin_phase(now: Instant, animate: bool) -> f32 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    if !animate {
        return 0.0;
    }
    let elapsed = now.saturating_duration_since(*EPOCH.get_or_init(|| now));
    let period = SPIN.as_nanos();
    ((elapsed.as_nanos() % period) as f32 / period as f32).min(1.0 - f32::EPSILON / 2.0)
}

#[cfg(test)]
#[path = "../test/motion.rs"]
mod tests;
