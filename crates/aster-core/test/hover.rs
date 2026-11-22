use super::*;
use crate::motion::HOVER_FADE;

#[test]
fn hover_hands_over_independently_on_each_surface() {
    let mut hover = HoverState::default();
    let now = Instant::now();
    let ink = Color::rgba(0x10203080);

    for glow in [&mut hover.menu, &mut hover.overlay, &mut hover.settings, &mut hover.profiles, &mut hover.files] {
        assert!(glow.aim(Some(1), true, now));
    }
    assert!(hover.tabs.aim(Some(TabHover::Tab(1)), true, now));
    assert!(hover.window.aim(Some(HitArea::Minimize), true, now));

    let handover = now + HOVER_FADE;
    let middle = handover + HOVER_FADE / 2;
    assert!(hover.tabs.aim(Some(TabHover::Plus), true, handover));
    assert!(hover.window.aim(Some(HitArea::Close), true, handover));
    assert_eq!(fill(&hover.tabs, TabHover::Tab(1), middle, ink), Some(ink.alpha(16)));
    assert_eq!(fill(&hover.tabs, TabHover::Plus, middle, ink), Some(ink.alpha(112)));
    assert_eq!(fill(&hover.window, HitArea::Minimize, middle, ink), Some(ink.alpha(16)));
    assert_eq!(fill(&hover.window, HitArea::Close, middle, ink), Some(ink.alpha(112)));

    for glow in [&mut hover.menu, &mut hover.overlay, &mut hover.settings, &mut hover.profiles, &mut hover.files] {
        assert_eq!(fill(glow, 1, handover, ink), Some(ink));
        assert!(glow.aim(Some(2), true, handover));
        assert_eq!(fill(glow, 1, middle, ink), Some(ink.alpha(16)));
        assert_eq!(fill(glow, 2, middle, ink), Some(ink.alpha(112)));
        assert!(!glow.aim(Some(2), true, handover + HOVER_FADE));
    }
}

#[test]
fn hover_commits_the_target_without_motion_when_disabled() {
    let mut glow = HoverState::default().files;
    let now = Instant::now();
    let ink = Color::rgba(0x10203080);
    assert!(glow.aim(Some(1), true, now));
    assert!(!glow.aim(Some(2), false, now + HOVER_FADE / 2));
    assert_eq!(fill(&glow, 1, now, ink), None);
    assert_eq!(fill(&glow, 2, now, ink), Some(ink));
}
