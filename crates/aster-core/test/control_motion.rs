use super::*;

#[test]
fn profile_group_expand_arrow_follows_turn() {
    let mut controls = Controls::default();
    let now = Instant::now();
    let mut frame = controls.begin(now, true);
    assert_eq!(frame.angle(Control::Group(false, Header::Builtin), false), 0.0);
    assert!(!frame.finish());
    let mut frame = controls.begin(now, true);
    frame.angle(Control::Group(false, Header::Builtin), true);
    assert!(frame.finish());
    let mut frame = controls.begin(now + TURN / 2, true);
    assert_eq!(frame.angle(Control::Group(false, Header::Builtin), true), 78.75);
    assert!(frame.finish());
    let mut frame = controls.begin(now + TURN / 2, false);
    assert_eq!(frame.angle(Control::Group(false, Header::Builtin), true), 90.0);
    assert!(!frame.finish());
}

#[test]
fn tab_indicator_slides_and_checkbox_animates() {
    let mut controls = Controls::default();
    let now = Instant::now();
    let first = Rect::new(10.0, 20.0, 80.0, 30.0);
    let next = Rect::new(90.0, 20.0, 100.0, 30.0);
    let mut frame = controls.begin(now, true);
    assert_eq!(frame.indicator(Tabs::Profiles, first), first);
    assert_eq!(frame.check(0, 1, false), 0.0);
    assert!(!frame.finish());
    let mut frame = controls.begin(now, true);
    assert_eq!(frame.indicator(Tabs::Profiles, next), first);
    assert_eq!(frame.check(0, 1, true), 0.0);
    assert!(frame.finish());
    let mut frame = controls.begin(now + SLIDE / 2, true);
    assert_eq!(frame.indicator(Tabs::Profiles, next).x, 80.0);
    assert_eq!(frame.check(0, 1, true), 0.875);
    assert!(frame.finish());
    let mut frame = controls.begin(now + SLIDE, true);
    assert_eq!(frame.indicator(Tabs::Profiles, next), next);
    assert_eq!(frame.check(0, 1, true), 1.0);
    assert!(!frame.finish());
}
