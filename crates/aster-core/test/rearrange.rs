use super::*;
use crate::config::Config;
use crate::input::{Event, MouseButton};
use crate::keymap::Action;
use crate::profile::Profile;
use std::sync::Arc;

#[test]
fn drop_zones_cover_the_target_without_overlapping() {
    let rect = Rect::new(20.0, 30.0, 200.0, 100.0);
    for (x, y, expected) in [
        (21.0, 31.0, DropZone::Side(Side::Left)),
        (219.0, 129.0, DropZone::Side(Side::Right)),
        (120.0, 31.0, DropZone::Side(Side::Top)),
        (120.0, 129.0, DropZone::Side(Side::Bottom)),
        (120.0, 80.0, DropZone::Center),
    ] {
        assert_eq!(drop_zone(rect, x, y), Some(expected));
        assert_eq!(drop_zones(rect).iter().filter(|(_, r)| r.contains(x, y)).count(), 1);
    }
    assert_eq!(drop_zone(rect, rect.right(), rect.y), None);
}

#[test]
fn labels_have_a_fixed_width_without_ambiguous_prefixes() {
    assert_eq!((pane_label(0, 26), pane_label(25, 26)), ("A".into(), "Z".into()));
    assert_eq!((pane_label(0, 27), pane_label(25, 27), pane_label(26, 27)), ("AA".into(), "AZ".into(), "BA".into()));
}

#[test]
fn labels_and_alt_drag_rearrange_the_live_panes() {
    let mut a = App::new(Config::default(), vec![Profile::new("local", "Shell", "shell", &[])], true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a.open_default_tab();
    a.run(Action::SplitRight);
    a.chrome = a.tab_chrome();
    let original = a.tabs[0].root.panes();
    a.run(Action::RearrangePanes);
    a.on_event(Event::Char('a'));
    a.on_event(Event::Char('B'));
    assert_eq!(a.tabs[0].root.panes(), [original[1], original[0]]);
    a.layout = a.tabs[0].root.layout(a.chrome.content, 1.0);
    let (source, target) = (a.layout.panes[0].1, a.layout.panes[1].1);
    a.on_event(Event::Modifiers(Mods { alt: true, ..Mods::NONE }));
    a.on_event(Event::MouseDown { x: source.x + 30.0, y: source.y + 40.0, button: MouseButton::Left, clicks: 1 });
    let (x, y) = (target.x + target.w / 2.0, target.bottom() - 5.0);
    a.on_event(Event::MouseMove { x, y });
    assert_eq!(a.pane_drag_hover(), Some((target, DropZone::Side(Side::Bottom))));
    a.on_event(Event::MouseUp { x, y, button: MouseButton::Left });
    assert_eq!(a.tabs[0].root.panes(), original);
    assert!(matches!(a.tabs[0].root, crate::panes::Node::Split { axis: crate::panes::Axis::Vertical, .. }));
    assert_eq!(a.panes.len(), 2);
}