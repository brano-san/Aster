use aster_core::motion::{Tween, RESIZE};
use aster_platform::{DockSlide, DockSpec, Edge, Window};
use std::time::{Duration, Instant};

const QUAKE_SPAN: Duration = Duration::from_millis(RESIZE.as_millis() as u64 * 3 / 2);
pub(crate) const FRAME_MS: u32 = 16;

struct Slide {
    anchor: DockSlide,
    visible: bool,
    y: Tween,
    last_y: i32,
}

impl Slide {
    fn new(anchor: DockSlide, visible: bool, now: Instant) -> Self {
        let start = if visible { anchor.hidden_y } else { anchor.shown_y };
        let mut slide = Self { anchor, visible, y: Tween::at(start as f32), last_y: start };
        slide.aim(visible, now);
        slide
    }

    fn aim(&mut self, visible: bool, now: Instant) {
        self.visible = visible;
        let y = if visible { self.anchor.shown_y } else { self.anchor.hidden_y };
        self.y.aim(y as f32, true, now, QUAKE_SPAN);
    }

    fn sample(&mut self, animate: bool, now: Instant) -> (i32, bool) {
        let moving = self.y.aim(self.y.target(), animate, now, QUAKE_SPAN);
        (self.y.value(now, QUAKE_SPAN).round() as i32, moving)
    }
}

#[derive(Default)]
pub(crate) struct Quake {
    slide: Option<Slide>,
}

impl Quake {
    pub fn target(&self) -> Option<bool> {
        self.slide.as_ref().map(|slide| slide.visible)
    }

    pub fn set_visible(&mut self, window: &impl Window, visible: bool, spec: Option<DockSpec>, animate: bool, now: Instant) {
        let animate = animate && spec.is_some_and(|spec| spec.edge == Edge::Top);
        if let Some(slide) = &mut self.slide {
            if animate && window.dock_slide() == Some(slide.anchor) && window.is_visible() {
                slide.aim(visible, now);
                if visible { window.activate(); }
                self.advance(window, true, now);
                return;
            }
        }
        let canceled = self.slide.take();
        let was_visible = window.is_visible();
        if visible && !was_visible && animate {
            window.dock(spec);
        }
        let anchor = window.dock_slide().filter(|_| animate && was_visible != visible);
        let start = anchor.filter(|anchor| {
            if visible {
                window.set_position(anchor.x, anchor.hidden_y)
            } else {
                window.screen_position() == Some((anchor.x, anchor.shown_y))
            }
        });
        if let Some(anchor) = start {
            self.slide = Some(Slide::new(anchor, visible, now));
            if visible {
                window.set_visible(true);
                window.activate();
            }
            window.set_timer(FRAME_MS);
        } else {
            if let Some(slide) = canceled {
                Self::finish(window, &slide, visible);
            } else {
                window.set_visible(visible);
            }
            if visible && !was_visible && spec.is_some() {
                window.dock(spec);
            }
            if visible { window.activate(); }
        }
    }

    pub fn advance(&mut self, window: &impl Window, animate: bool, now: Instant) {
        let Some(slide) = &mut self.slide else { return };
        let track = animate && window.dock_slide() == Some(slide.anchor) && window.is_visible();
        let (y, moving) = slide.sample(track, now);
        if track && (y == slide.last_y || window.set_position(slide.anchor.x, y)) {
            if moving {
                slide.last_y = y;
                window.set_timer(FRAME_MS);
                return;
            }
            if slide.visible {
                self.slide = None;
                return;
            }
        }
        let slide = self.slide.take().unwrap();
        Self::finish(window, &slide, slide.visible);
    }

    fn finish(window: &impl Window, slide: &Slide, visible: bool) {
        if !visible {
            window.set_visible(false);
        }
        if window.dock_slide() == Some(slide.anchor) {
            let _ = window.set_position(slide.anchor.x, slide.anchor.shown_y);
        }
        if visible && !window.is_visible() {
            window.set_visible(true);
        }
    }
}

#[cfg(test)]
#[path = "../test/quake.rs"]
mod tests;
