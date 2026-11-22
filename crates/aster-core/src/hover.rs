use crate::input::HitArea;
use crate::motion::Glow;
use aster_gfx::render::Color;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabHover {
    Tab(u64),
    Close(u64),
    Plus,
    Menu,
}

#[derive(Default)]
pub(crate) struct HoverState {
    pub tabs: Glow<TabHover>,
    pub window: Glow<HitArea>,
    pub menu: Glow<usize>,
    pub overlay: Glow<usize>,
    pub settings: Glow<usize>,
    pub profiles: Glow<usize>,
    pub files: Glow<usize>,
}

pub(crate) fn opacity(ink: Color, alpha: f32) -> Option<Color> {
    let alpha = ((ink.0 >> 24) as f32 * alpha.clamp(0.0, 1.0)).round() as u8;
    (alpha > 0).then(|| ink.alpha(alpha))
}

pub(crate) fn fill<K: Copy + Eq>(glow: &Glow<K>, key: K, now: Instant, ink: Color) -> Option<Color> {
    opacity(ink, glow.alpha(key, now))
}

#[cfg(test)]
#[path = "../test/hover.rs"]
mod tests;
