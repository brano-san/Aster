use super::*;

#[test]
fn smooth_cursor_fades_at_both_edges_and_rests_between_timers() {
    let now = Instant::now();
    let mut blink = Blink::new(now);
    assert_eq!(blink.sample(true, true, now), (1.0, false));
    let off = blink.deadline();
    assert!(!blink.tick(off - Duration::from_millis(1)));
    assert!(blink.tick(off));
    assert_eq!(blink.sample(true, true, off), (1.0, true));
    assert_eq!(blink.sample(true, true, off + FADE / 2), (0.125, true));
    assert_eq!(blink.sample(true, true, off + FADE), (0.0, false));
    let on = blink.deadline();
    assert_eq!(blink.sample(true, true, on), (0.0, false));
    assert!(blink.tick(on));
    assert_eq!(blink.sample(true, true, on), (0.0, true));
    assert_eq!(blink.sample(true, true, on + FADE / 2), (0.875, true));
    assert_eq!(blink.sample(true, true, on + FADE), (1.0, false));
}

#[test]
fn cursor_step_disabled_and_typing_reset_are_immediate() {
    let now = Instant::now();
    let mut blink = Blink::new(now);
    let off = blink.deadline();
    blink.tick(off);
    assert_eq!(blink.sample(true, false, off), (0.0, false));
    assert_eq!(blink.sample(false, true, off), (1.0, false));
    let typed = off + FADE / 2;
    blink = Blink::new(typed);
    assert_eq!(blink.sample(true, true, typed), (1.0, false));
    assert_eq!(blink.deadline(), typed + BLINK);
}
