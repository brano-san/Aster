use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use aster_gfx::render::{Color, Rect};

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
    Switch(&'static str),
    Slider(&'static str),
    Check(usize, usize),
    Group(bool, Header),
    Section(&'static str),
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
    fn value(&mut self, key: Control, target: f32, span: Duration, direct: bool) -> f32 {
        let (last_frame, tween) = self.controls.tweens.entry(key).or_insert_with(|| (!self.controls.frame, Tween::at(target)));
        *last_frame = self.controls.frame;
        self.moving |= tween.aim(target, self.animate && !direct, self.now, span);
        tween.value(self.now, span)
    }

    pub fn switch(&mut self, key: &'static str, on: bool) -> f32 {
        self.value(Control::Switch(key), u8::from(on) as f32, TURN, false)
    }

    pub fn slider(&mut self, key: &'static str, fraction: f32, dragging: bool) -> f32 {
        self.value(Control::Slider(key), fraction, TURN, dragging)
    }

    pub fn check(&mut self, tab: usize, field: usize, checked: bool) -> f32 {
        self.value(Control::Check(tab, field), u8::from(checked) as f32, CHECK_IN, false)
    }

    pub fn angle(&mut self, key: Control, open: bool) -> f32 {
        self.value(key, if open { 90.0 } else { 0.0 }, TURN, false)
    }

    pub fn indicator(&mut self, bar: Tabs, rect: Rect) -> Rect {
        let x = self.value(Control::Indicator(bar, false), rect.x, SLIDE, false);
        let width = self.value(Control::Indicator(bar, true), rect.w, SLIDE, false);
        Rect::new(x, rect.y, width, rect.h)
    }

    pub fn finish(self) -> bool {
        self.controls.tweens.retain(|_, (frame, _)| *frame == self.controls.frame);
        self.moving
    }
}

pub(crate) fn knob_x(track: Rect, knob: f32, inset: f32, progress: f32) -> f32 {
    (track.x + inset + (track.w - knob - 2.0 * inset) * progress).round()
}

pub(crate) fn blend(from: Color, to: Color, progress: f32) -> Color {
    if progress == 0.0 { return from; }
    if progress == 1.0 { return to; }
    let from = from.0.to_le_bytes();
    let to = to.0.to_le_bytes();
    Color(u32::from_le_bytes(std::array::from_fn(|i| {
        (f32::from(from[i]) + (f32::from(to[i]) - f32::from(from[i])) * progress).round() as u8
    })))
}

#[cfg(test)]
#[path = "../test/control_motion.rs"]
mod tests;
