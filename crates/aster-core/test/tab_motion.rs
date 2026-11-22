use super::*;

fn tab(x: f32, width: f32) -> Rect {
    Rect::new(x, 6.0, width, 30.0)
}

fn frame(strip: &mut TabStrip, tabs: &[(u64, Rect)], active: Option<u64>, drag: Option<(u64, f32)>, animate: bool, now: Instant) -> bool {
    strip.settle(tabs, active, drag, StripFrame {
        bar: Rect::new(0.0, 0.0, 500.0, 36.0), vertical: false, plus: Rect::default(), menu: Rect::default(), animate, now,
    })
}

#[test]
fn inserting_a_tab_grows_it_from_the_old_seam() {
    let now = Instant::now();
    let mut strip = TabStrip::default();
    assert!(!frame(&mut strip, &[(1, tab(8.0, 100.0)), (3, tab(108.0, 100.0))], Some(1), None, true, now));
    let tabs = [(1, tab(8.0, 90.0)), (2, tab(98.0, 90.0)), (3, tab(188.0, 90.0))];
    assert!(frame(&mut strip, &tabs, Some(2), None, true, now));
    assert_eq!(strip.rect(2, now), Some(tab(108.0, 0.0)));
    assert_eq!(strip.rect(3, now), Some(tab(108.0, 100.0)));
    assert_eq!(strip.rect(2, now + SLIDE / 2), Some(tab(99.0, 79.0)));
    assert_eq!(strip.rect(3, now + SLIDE / 2), Some(tab(178.0, 91.0)));
    assert!(!frame(&mut strip, &tabs, Some(2), None, true, now + SLIDE));
    assert_eq!(strip.rect(2, now + SLIDE), Some(tab(98.0, 90.0)));
}

#[test]
fn closing_keeps_a_shrinking_ghost_until_neighbors_settle() {
    let now = Instant::now();
    let mut strip = TabStrip::default();
    let before = [(1, tab(8.0, 90.0)), (2, tab(98.0, 90.0)), (3, tab(188.0, 90.0))];
    frame(&mut strip, &before, Some(2), None, true, now);
    let after = [(1, tab(8.0, 100.0)), (3, tab(108.0, 100.0))];
    assert!(frame(&mut strip, &after, Some(3), None, true, now));
    assert_eq!(strip.rect(2, now), Some(tab(98.0, 90.0)));
    assert_eq!(strip.rect(2, now + SLIDE / 2), Some(tab(107.0, 11.0)));
    assert_eq!(strip.rect(3, now + SLIDE / 2), Some(tab(118.0, 99.0)));
    assert!(!frame(&mut strip, &after, Some(3), None, true, now + SLIDE));
    assert_eq!(strip.rect(2, now + SLIDE), None);
    assert_eq!(strip.rect(3, now + SLIDE), Some(tab(108.0, 100.0)));
}

#[test]
fn reordering_eases_neighbors_and_tracks_the_dragged_tab_immediately() {
    let now = Instant::now();
    let mut strip = TabStrip::default();
    frame(&mut strip, &[(1, tab(8.0, 100.0)), (2, tab(108.0, 100.0))], Some(1), None, true, now);
    let after = [(2, tab(8.0, 100.0)), (1, tab(108.0, 100.0))];
    assert!(frame(&mut strip, &after, Some(1), Some((1, 73.0)), true, now));
    assert_eq!(strip.rect(2, now), Some(tab(108.0, 100.0)));
    assert_eq!(strip.rect(1, now), Some(tab(73.0, 100.0)));
    let middle = now + SLIDE / 2;
    assert!(frame(&mut strip, &after, Some(1), Some((1, 86.0)), true, middle));
    assert_eq!(strip.rect(2, middle), Some(tab(21.0, 100.0)));
    assert_eq!(strip.indicator(middle), Some(tab(86.0, 100.0)));
    assert!(frame(&mut strip, &after, Some(1), None, true, middle));
    assert_eq!(strip.rect(1, middle), Some(tab(86.0, 100.0)));
    assert!(!frame(&mut strip, &after, Some(1), None, true, middle + SLIDE));
    assert_eq!(strip.rect(1, middle + SLIDE), Some(tab(108.0, 100.0)));
    assert_eq!(strip.rect(2, middle + SLIDE), Some(tab(8.0, 100.0)));
}

#[test]
fn active_indicator_retargets_from_current_position_and_snaps_when_disabled() {
    let now = Instant::now();
    let mut strip = TabStrip::default();
    let tabs = [(1, tab(8.0, 100.0)), (2, tab(108.0, 100.0)), (3, tab(208.0, 100.0))];
    frame(&mut strip, &tabs, Some(1), None, true, now);
    assert!(frame(&mut strip, &tabs, Some(2), None, true, now));
    let middle = now + SLIDE / 2;
    assert_eq!(strip.indicator(middle), Some(tab(96.0, 100.0)));
    assert!(frame(&mut strip, &tabs, Some(3), None, true, middle));
    assert_eq!(strip.indicator(middle), Some(tab(96.0, 100.0)));
    assert!(!frame(&mut strip, &tabs[1..], Some(3), None, false, middle));
    assert_eq!(strip.indicator(middle), Some(tab(208.0, 100.0)));
    assert_eq!(strip.rect(1, middle), None);
}
