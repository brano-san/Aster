use super::*;

#[test]
fn panel_width_opens_closes_and_reverses_from_visible_width() {
    let mut panel = Slide::default();
    let start = Instant::now();
    let mut motion = false;
    assert_eq!(panel.update(Some((240.0, 1)), true, start, &mut motion), 0.0);
    assert!(motion);
    let middle = start + SLIDE / 2;
    assert_eq!(panel.update(Some((240.0, 2)), true, middle, &mut false), 210.0);
    assert_eq!(panel.update(None, true, middle, &mut false), 210.0);
    assert_eq!(panel.content, Some((240.0, 2)));
    let reverse = middle + SLIDE / 2;
    assert_eq!(panel.update(None, true, reverse, &mut false), 26.25);
    assert_eq!(panel.update(Some((240.0, 3)), true, reverse, &mut false), 26.25);
    motion = false;
    assert_eq!(panel.update(Some((240.0, 3)), true, reverse + SLIDE, &mut motion), 240.0);
    assert!(!motion);
}

#[test]
fn disabled_panel_motion_finishes_and_discards_closed_content() {
    let mut panel = Slide::default();
    let now = Instant::now();
    let mut motion = false;
    assert_eq!(panel.update(Some((240.0, ())), false, now, &mut motion), 240.0);
    assert!(!motion);
    assert_eq!(panel.update(None, false, now, &mut motion), 0.0);
    assert!(!motion);
    assert!(panel.content.is_none());
}
