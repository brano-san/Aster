use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use aster_gfx::render::Rect;

use crate::motion::{CHECK_IN, SLIDE, TURN, Tween};
use crate::profiles_page::Header;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Tabs {
    Schemes,
    Profiles,
    ProfileEditor,
    Extensions,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Control {
    Check(usize, usize),
    Group(bool, Header),
    Indicator(Tabs, bool),
}

#[derive(Default)]
pub(crate) struct Controls {
    tweens: BTreeMap<Control, (bool, Tween)>,
    frame: bool,
}

pub(crate) struct Frame<'a> {
    controls: &'a mut Controls,
    now: Instant,
    animate: bool,
    moving: bool,
}

impl Controls {
    pub fn begin(&mut self, now: Instant, animate: bool) -> Frame<'_> {
        self.frame = !self.frame;
        Frame { controls: self, now, animate, moving: false }
    }
}

impl Frame<'_> {
    fn value(&mut self, key: Control, target: f32, span: Duration) -> f32 {
        let (last_frame, tween) = self.controls.tweens.entry(key).or_insert_with(|| (!self.controls.frame, Tween::at(target)));
        *last_frame = self.controls.frame;
        self.moving |= tween.aim(target, self.animate, self.now, span);
        tween.value(self.now, span)
    }

    pub fn check(&mut self, tab: usize, field: usize, checked: bool) -> f32 {
        self.value(Control::Check(tab, field), u8::from(checked) as f32, CHECK_IN)
    }

    pub fn angle(&mut self, key: Control, open: bool) -> f32 {
        self.value(key, if open { 90.0 } else { 0.0 }, TURN)
    }

    pub fn indicator(&mut self, bar: Tabs, rect: Rect) -> Rect {
        let x = self.value(Control::Indicator(bar, false), rect.x, SLIDE);
        let width = self.value(Control::Indicator(bar, true), rect.w, SLIDE);
        Rect::new(x, rect.y, width, rect.h)
    }

    pub fn finish(self) -> bool {
        self.controls.tweens.retain(|_, (frame, _)| *frame == self.controls.frame);
        self.moving
    }
}

#[cfg(test)]
#[path = "../test/control_motion.rs"]
mod tests;
