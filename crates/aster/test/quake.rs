use super::*;

const ANCHOR: DockSlide = DockSlide { x: -1400, shown_y: 20, hidden_y: -500 };

#[test]
fn quake_slides_down_and_up_in_monitor_coordinates() {
    let now = Instant::now();
    let mut slide = Slide::new(ANCHOR, true, now);
    assert_eq!(slide.sample(true, now), (-500, true));
    assert_eq!(slide.sample(true, now + QUAKE_SPAN / 2), (-45, true));
    let shown = now + QUAKE_SPAN;
    assert_eq!(slide.sample(true, shown), (20, false));
    slide.aim(false, shown);
    assert_eq!(slide.sample(true, shown), (20, true));
    assert_eq!(slide.sample(true, shown + QUAKE_SPAN / 2), (-435, true));
    assert_eq!(slide.sample(true, shown + QUAKE_SPAN), (-500, false));
}

#[test]
fn quake_reverses_from_its_current_y_and_snaps_when_disabled() {
    let now = Instant::now();
    let mut slide = Slide::new(ANCHOR, false, now);
    let reverse = now + QUAKE_SPAN / 2;
    assert_eq!(slide.sample(true, reverse), (-435, true));
    slide.aim(true, reverse);
    assert_eq!(slide.sample(true, reverse), (-435, true));
    assert_eq!(slide.sample(true, reverse + QUAKE_SPAN / 2), (-37, true));
    assert_eq!(slide.sample(false, reverse + QUAKE_SPAN / 2), (20, false));
}

#[test]
fn quake_origin_is_above_the_physical_monitor_edge() {
    let anchor = DockSlide::top((-1800, -880, 1600, 400), -900);
    assert_eq!(anchor, DockSlide { x: -1800, shown_y: -880, hidden_y: -1300 });
    assert_eq!(QUAKE_SPAN, Duration::from_millis(210));
}
