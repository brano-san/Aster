use super::*;

#[test]
fn switch_knob_and_expand_arrow_follow_turn() {
    let mut controls = Controls::default();
    let now = Instant::now();
    let rect = Rect::new(10.0, 0.0, 40.0, 22.0);
    let mut frame = controls.begin(now, true);
    assert_eq!(knob_x(rect, 16.0, 3.0, frame.switch("toggle", false)), 13.0);
    assert_eq!(frame.angle(Control::Section("section"), false), 0.0);
    assert!(!frame.finish());
    let mut frame = controls.begin(now, true);
    frame.switch("toggle", true);
    frame.angle(Control::Section("section"), true);
    assert!(frame.finish());
    let mut frame = controls.begin(now + TURN / 2, true);
    assert_eq!(knob_x(rect, 16.0, 3.0, frame.switch("toggle", true)), 29.0);
    assert_eq!(frame.angle(Control::Section("section"), true), 78.75);
    assert!(frame.finish());
    let mut frame = controls.begin(now + TURN / 2, false);
    assert_eq!(knob_x(rect, 16.0, 3.0, frame.switch("toggle", false)), 13.0);
    assert_eq!(frame.angle(Control::Section("section"), true), 90.0);
    assert!(!frame.finish());
}

#[test]
fn tab_indicator_slides_while_slider_drag_is_direct() {
    let mut controls = Controls::default();
    let now = Instant::now();
    let first = Rect::new(10.0, 20.0, 80.0, 30.0);
    let next = Rect::new(90.0, 20.0, 100.0, 30.0);
    let mut frame = controls.begin(now, true);
    assert_eq!(frame.indicator(Tabs::Profiles, first), first);
    assert_eq!(frame.slider("slider", 0.2, false), 0.2);
    assert_eq!(frame.check(0, 1, false), 0.0);
    assert!(!frame.finish());
    let mut frame = controls.begin(now, true);
    assert_eq!(frame.indicator(Tabs::Profiles, next), first);
    assert_eq!(frame.slider("slider", 0.8, false), 0.2);
    assert_eq!(frame.check(0, 1, true), 0.0);
    assert!(frame.finish());
    let mut frame = controls.begin(now + SLIDE / 2, true);
    assert_eq!(frame.indicator(Tabs::Profiles, next).x, 80.0);
    assert_eq!(frame.slider("slider", 0.3, true), 0.3);
    assert_eq!(frame.check(0, 1, true), 0.875);
    assert!(frame.finish());
    let mut frame = controls.begin(now + SLIDE, true);
    assert_eq!(frame.indicator(Tabs::Profiles, next), next);
    assert_eq!(frame.slider("slider", 0.3, false), 0.3);
    assert_eq!(frame.check(0, 1, true), 1.0);
    assert!(!frame.finish());
}
