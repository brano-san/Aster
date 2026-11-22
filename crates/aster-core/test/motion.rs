use super::*;

#[test]
fn easing_clamps_and_reaches_both_endpoints() {
    let now = Instant::now();
    assert_eq!(ease_out(-1.0), 0.0);
    assert_eq!(eased(now + POP, now, POP), 0.0);
    assert_eq!(eased(now, now, POP), 0.0);
    assert_eq!(eased(now, now + POP / 2, POP), 0.875);
    assert_eq!(eased(now, now + POP, POP), 1.0);
    assert_eq!(eased(now, now + POP * 2, POP), 1.0);
    assert_eq!(eased(now, now, Duration::ZERO), 1.0);
}

#[test]
fn tween_tracks_time_and_settles() {
    let now = Instant::now();
    let mut tween = Tween::at(4.0);
    assert!(!tween.aim(4.0, true, now, POP));
    assert!(tween.aim(12.0, true, now, POP));
    assert_eq!(tween.target(), 12.0);
    assert_eq!(tween.value(now, POP), 4.0);
    assert!(tween.aim(12.0, true, now + POP / 2, POP));
    assert_eq!(tween.value(now + POP / 2, POP), 11.0);
    assert!(!tween.aim(12.0, true, now + POP, POP));
    assert_eq!(tween.value(now + POP, POP), 12.0);
    assert!(tween.at.is_none());
}

#[test]
fn tween_retargets_from_visible_value_and_jumps_when_disabled() {
    let now = Instant::now();
    let mut tween = Tween::at(0.0);
    tween.aim(16.0, true, now, SLIDE);
    let middle = now + SLIDE / 2;
    assert_eq!(tween.value(middle, SLIDE), 14.0);
    assert!(tween.aim(6.0, true, middle, SLIDE));
    assert_eq!(tween.value(middle, SLIDE), 14.0);
    assert_eq!(tween.value(middle + SLIDE / 2, SLIDE), 7.0);
    assert!(!tween.aim(6.0, false, middle + SLIDE / 2, SLIDE));
    assert_eq!(tween.value(middle + SLIDE / 2, SLIDE), 6.0);
    assert!(!tween.aim(1.0, true, middle, Duration::ZERO));
    assert_eq!(tween.value(middle, SLIDE), 1.0);
}

#[test]
fn glow_hands_over_and_preserves_alpha_on_reversal() {
    let now = Instant::now();
    let mut glow = Glow::default();
    assert!(glow.aim(Some(1), true, now));
    assert_eq!(glow.alpha(1, now), 0.0);
    let handover = now + HOVER_FADE / 2;
    assert!(glow.aim(Some(2), true, handover));
    assert_eq!(glow.alpha(1, handover), 0.875);
    assert_eq!(glow.alpha(2, handover), 0.0);
    let mut touched = Vec::new();
    glow.touches(|key| touched.push(key));
    touched.sort_unstable();
    assert_eq!(touched, [1, 2]);
    let reversal = handover + HOVER_FADE / 2;
    assert_eq!(glow.alpha(1, reversal), 0.109375);
    assert_eq!(glow.alpha(2, reversal), 0.875);
    assert!(glow.aim(Some(1), true, reversal));
    assert!((glow.alpha(1, reversal) - 0.109375).abs() < 0.000001);
    assert_eq!(glow.alpha(2, reversal), 0.875);
    assert!(!glow.aim(Some(1), true, reversal + HOVER_FADE));
    assert_eq!(glow.alpha(1, reversal + HOVER_FADE), 1.0);
    assert_eq!(glow.alpha(2, reversal + HOVER_FADE), 0.0);
    touched.clear();
    glow.touches(|key| touched.push(key));
    assert_eq!(touched, [1]);
}

#[test]
fn glow_clears_immediately_when_disabled() {
    let now = Instant::now();
    let mut glow = Glow::default();
    assert!(glow.aim(Some(1), true, now));
    assert!(!glow.aim(Some(2), false, now));
    assert_eq!(glow.alpha(1, now), 0.0);
    assert_eq!(glow.alpha(2, now), 1.0);
    assert!(!glow.aim(None, false, now));
    assert_eq!(glow.alpha(2, now), 0.0);
}

#[test]
fn arrivals_track_new_and_returning_items_within_an_owner() {
    let now = Instant::now();
    let mut arrivals = Arrivals::default();
    assert!(!arrivals.settle(0, &[1], true, now));
    assert!(arrivals.settle(0, &[1, 2], true, now));
    assert_eq!(arrivals.shown(1, now), 1.0);
    assert_eq!(arrivals.shown(2, now), 0.0);
    let middle = now + POP / 2;
    assert!(arrivals.settle(0, &[2], true, middle));
    assert_eq!(arrivals.shown(2, middle), 0.875);
    assert!(arrivals.settle(0, &[1, 2], true, middle));
    assert_eq!(arrivals.shown(1, middle), 0.0);
    assert!(!arrivals.settle(0, &[1, 2], true, middle + POP));
    assert_eq!(arrivals.shown(1, middle + POP), 1.0);
}

#[test]
fn arrivals_reset_on_owner_change_or_when_disabled() {
    let now = Instant::now();
    let mut arrivals = Arrivals::default();
    assert!(!arrivals.settle(1, &[1], true, now));
    assert!(arrivals.settle(1, &[1, 2], true, now));
    assert!(!arrivals.settle(2, &[2, 3], true, now));
    assert_eq!(arrivals.shown(2, now), 1.0);
    assert_eq!(arrivals.shown(3, now), 1.0);
    assert!(arrivals.settle(2, &[2, 3, 4], true, now));
    assert!(!arrivals.settle(2, &[2, 3, 4, 5], false, now));
    assert_eq!(arrivals.shown(4, now), 1.0);
    assert!(!arrivals.settle(2, &[2, 3, 4, 5], true, now));
}

#[test]
fn spin_uses_shared_phase_and_stops_when_disabled() {
    let now = Instant::now();
    assert_eq!(spin_phase(now, false), 0.0);
    assert_eq!(spin_phase(now, true), 0.0);
    assert_eq!(spin_phase(now + SPIN / 2, true), 0.5);
    assert_eq!(spin_phase(now + SPIN, true), 0.0);
}
