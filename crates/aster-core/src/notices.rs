use crate::motion::{eased, TOAST_IN, TOAST_OUT};
use std::time::{Duration, Instant};

pub(crate) struct Toast {
    pub(crate) text: String,
    pub(crate) opened_at: Instant,
    pub(crate) fade_at: Instant,
    pub(crate) tab_name: bool,
    pub(crate) checking: bool,
}

impl Toast {
    pub(crate) fn new(text: String, now: Instant, lasting: Duration, tab_name: bool) -> Self {
        Self { text, opened_at: now, fade_at: now + lasting, tab_name, checking: false }
    }

    pub(crate) fn expires(&self, animate: bool) -> Instant {
        self.fade_at + if animate { TOAST_OUT } else { Duration::ZERO }
    }

    pub(crate) fn deadline(&self, now: Instant, animate: bool) -> Instant {
        if now < self.fade_at { self.fade_at } else { self.expires(animate) }
    }

    pub(crate) fn appearance(&self, now: Instant, animate: bool) -> Option<(f32, f32, bool)> {
        if now >= self.expires(animate) {
            return None;
        }
        if !animate {
            return Some((1.0, 1.0, false));
        }
        let opening = eased(self.opened_at, now.min(self.fade_at), TOAST_IN);
        let alpha = 1.0 - eased(self.fade_at, now, TOAST_OUT);
        Some((opening, alpha, now < self.opened_at + TOAST_IN || now >= self.fade_at))
    }
}
