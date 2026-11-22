use crate::motion::eased;
use aster_gfx::render::Rect;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct RectMotion {
    from: Rect,
    pub to: Rect,
    at: Option<Instant>,
}

impl RectMotion {
    pub fn at(rect: Rect) -> Self {
        Self { from: rect, to: rect, at: None }
    }

    pub fn value(&self, now: Instant, span: Duration) -> Rect {
        let Some(at) = self.at else { return self.to };
        let t = eased(at, now, span);
        if t >= 1.0 { return self.to; }
        let lerp = |a, b| a + (b - a) * t;
        let x = lerp(self.from.x, self.to.x);
        let y = lerp(self.from.y, self.to.y);
        let right = lerp(self.from.right(), self.to.right());
        let bottom = lerp(self.from.bottom(), self.to.bottom());
        Rect::new(x, y, right - x, bottom - y).snap()
    }

    pub fn aim(&mut self, to: Rect, animate: bool, now: Instant, span: Duration) -> bool {
        if !animate {
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
