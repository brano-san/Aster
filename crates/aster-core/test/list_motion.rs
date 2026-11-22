use super::*;

#[test]
fn wheel_targets_accumulate_and_retarget_from_the_visible_offset() {
    let mut scroll = Scroll::default();
    let now = Instant::now();
    scroll.retarget(0.0, 80.0, true, now);
    scroll.retarget(80.0, 160.0, true, now);
    let mid = now + SLIDE / 2;
    let mut motion = false;
    assert_eq!(scroll.sample(160.0, 500.0, true, mid, &mut motion), 140.0);
    assert!(motion);
    scroll.retarget(160.0, 240.0, true, mid);
    assert_eq!(scroll.visible(240.0, mid), 140.0);
    motion = false;
    assert_eq!(scroll.sample(240.0, 500.0, true, mid + SLIDE, &mut motion), 240.0);
    assert!(!motion);
}

#[test]
fn precise_scroll_and_disabled_animations_apply_immediately() {
    let mut scroll = Scroll::default();
    let now = Instant::now();
    scroll.retarget(0.0, 80.0, true, now);
    let mid = now + SLIDE / 2;
    let target = scroll.visible(80.0, mid) + 7.0;
    scroll.retarget(80.0, target, false, mid);
    let mut motion = false;
    assert_eq!(scroll.sample(target, 500.0, true, mid, &mut motion), 77.0);
    assert!(!motion);
    scroll.retarget(target, 200.0, true, mid);
    assert_eq!(scroll.sample(200.0, 500.0, false, mid, &mut motion), 200.0);
    assert!(!motion);
}
