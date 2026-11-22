use std::time::Instant;

use crate::input::Key;
use crate::motion::{SLIDE, Tween};

#[derive(Default)]
pub(crate) struct Lists {
    pub overlay: Scroll,
    pub settings: Scroll,
    pub sidebar: Scroll,
    pub files: Scroll,
}

pub(crate) struct Scroll(Tween);

impl Default for Scroll {
    fn default() -> Self {
        Self(Tween::at(0.0))
    }
}

impl Scroll {
    pub fn wheel(&mut self, previous: f32, max: f32, steps: f32, pixels: Option<f32>, animate: bool, now: Instant) -> f32 {
        let target = if let Some(pixels) = pixels {
            self.visible(previous, now) - pixels
        } else {
            previous - steps
        }.clamp(0.0, max);
        self.retarget(previous, target, animate && pixels.is_none(), now);
        target
    }

    pub fn visible(&self, target: f32, now: Instant) -> f32 {
        if self.0.target() == target { self.0.value(now, SLIDE) } else { target }
    }

    pub fn retarget(&mut self, previous: f32, target: f32, animate: bool, now: Instant) {
        if self.0.target() != previous {
            self.0 = Tween::at(previous);
        }
        self.0.aim(target, animate, now, SLIDE);
    }

    pub fn sample(&mut self, target: f32, max: f32, animate: bool, now: Instant, motion: &mut bool) -> f32 {
        if self.0.target() == target {
            *motion |= self.0.aim(target, animate, now, SLIDE);
        } else {
            self.0 = Tween::at(target);
        }
        self.0.value(now, SLIDE).clamp(0.0, max)
    }
}

pub(crate) fn scroll_key(key: Key) -> bool {
    matches!(key, Key::Up | Key::Down | Key::Tab | Key::PageUp | Key::PageDown | Key::Home | Key::End)
}

#[cfg(test)]
#[path = "../test/list_motion.rs"]
mod tests;
